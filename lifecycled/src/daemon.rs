//! Daemon wiring: policy decisions become kernel writes (P5-T01..T06).
//!
//! The daemon owns the tracked app registry, the timer set and the log. All
//! kernel I/O goes through the trait objects in [`Mechanisms`], so the full
//! event flow can be tested with fakes on any host.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cgroup::{Freezer, Killer, MemoryReader, Reclaimer};
use crate::identity::{AppId, AppScope, ScopeManager};
use crate::policy::{
    transition_is_legal, Action, Event, KillReason, LifecyclePolicy, PolicyConfig, PowerSourceKind,
    StateReason, TimerKind,
};
use crate::protocol::{Ack, LifecycleNotifier, NotifyError};
use crate::state::{AppProcess, AppState, Band};

/// All kernel-facing and app-facing mechanisms the daemon needs.
pub struct Mechanisms {
    pub scopes: Box<dyn ScopeManager>,
    pub freezer: Box<dyn Freezer>,
    pub reclaimer: Box<dyn Reclaimer>,
    pub killer: Box<dyn Killer>,
    pub memory: Box<dyn MemoryReader>,
    pub notifier: Box<dyn LifecycleNotifier>,
}

/// Runtime configuration (loaded from TOML by [`RuntimeConfig::load`]).
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub cgroup_root: PathBuf,
    pub socket_dir: PathBuf,
    pub state_dir: PathBuf,
    pub dry_run: bool,
    pub policy: PolicyConfig,
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    #[serde(default = "default_cgroup_root")]
    cgroup_root: String,
    #[serde(default = "default_socket_dir")]
    socket_dir: String,
    #[serde(default = "default_state_dir")]
    state_dir: String,
    #[serde(default)]
    dry_run: bool,
    #[serde(default = "default_grace_ac")]
    grace_ac_seconds: u64,
    #[serde(default = "default_grace_battery")]
    grace_battery_seconds: u64,
    #[serde(default = "default_hibernate_minutes")]
    hibernate_after_minutes: u64,
}

fn default_cgroup_root() -> String {
    "/sys/fs/cgroup".into()
}
fn default_socket_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/run/user/1000".into())
        .trim_end_matches('/')
        .to_string()
        + "/aether/lifecycle"
}
fn default_state_dir() -> String {
    std::env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            format!("{home}/.local/state")
        })
        .trim_end_matches('/')
        .to_string()
        + "/aether/lifecycled"
}
fn default_grace_ac() -> u64 {
    30
}
fn default_grace_battery() -> u64 {
    10
}
fn default_hibernate_minutes() -> u64 {
    10
}

impl RuntimeConfig {
    pub fn defaults() -> Self {
        let file = FileConfig {
            cgroup_root: default_cgroup_root(),
            socket_dir: default_socket_dir(),
            state_dir: default_state_dir(),
            dry_run: false,
            grace_ac_seconds: default_grace_ac(),
            grace_battery_seconds: default_grace_battery(),
            hibernate_after_minutes: default_hibernate_minutes(),
        };
        Self::from_file(file)
    }

    fn from_file(file: FileConfig) -> Self {
        let mut policy = PolicyConfig::default();
        policy.grace_period_ac = Duration::from_secs(file.grace_ac_seconds);
        policy.grace_period_battery = Duration::from_secs(file.grace_battery_seconds);
        policy.hibernate_after = Duration::from_secs(file.hibernate_after_minutes * 60);
        Self {
            cgroup_root: PathBuf::from(file.cgroup_root),
            socket_dir: PathBuf::from(file.socket_dir),
            state_dir: PathBuf::from(file.state_dir),
            dry_run: file.dry_run,
            policy,
        }
    }

    /// Load `/etc/aether/lifecycle.toml` then `$XDG_CONFIG_HOME/aether/lifecycle.toml`
    /// (later wins), falling back to defaults for every missing value.
    pub fn load() -> Self {
        let system = PathBuf::from("/etc/aether/lifecycle.toml");
        let user = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("aether/lifecycle.toml"));
        let mut merged = FileConfig {
            cgroup_root: default_cgroup_root(),
            socket_dir: default_socket_dir(),
            state_dir: default_state_dir(),
            dry_run: false,
            grace_ac_seconds: default_grace_ac(),
            grace_battery_seconds: default_grace_battery(),
            hibernate_after_minutes: default_hibernate_minutes(),
        };
        for candidate in [Some(system), user].into_iter().flatten() {
            if let Ok(text) = fs::read_to_string(&candidate) {
                if let Ok(parsed) = toml::from_str::<FileConfig>(&text) {
                    merged = parsed;
                }
            }
        }
        Self::from_file(merged)
    }
}

/// One logged decision (D3 section 9 format).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub epoch_secs: u64,
    pub app: Option<String>,
    pub text: String,
}

impl LogEntry {
    pub fn line(&self) -> String {
        match &self.app {
            Some(app) => format!("{} lifecycled: app={app} {}", self.epoch_secs, self.text),
            None => format!("{} lifecycled: {}", self.epoch_secs, self.text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillRecord {
    pub id: AppId,
    pub reason: KillReason,
    pub epoch_secs: u64,
    pub memory_current: u64,
    /// Priority band the app had when it was killed (B5 evidence).
    pub band: Band,
}

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("app {0} is not registered")]
    UnknownApp(AppId),
    #[error("invalid transition for {id}: {source}")]
    InvalidTransition {
        id: AppId,
        #[source]
        source: crate::state::TransitionError,
    },
    #[error("scope operation failed: {0}")]
    Scope(#[from] io::Error),
}

/// The lifecycle daemon.
pub struct Daemon {
    config: RuntimeConfig,
    policy: LifecyclePolicy,
    scopes: Box<dyn ScopeManager>,
    freezer: Box<dyn Freezer>,
    reclaimer: Box<dyn Reclaimer>,
    killer: Box<dyn Killer>,
    memory: Box<dyn MemoryReader>,
    notifier: Box<dyn LifecycleNotifier>,
    apps: BTreeMap<AppId, AppProcess>,
    timers: BTreeMap<(AppId, TimerKind), Instant>,
    log: Vec<LogEntry>,
    kills: Vec<KillRecord>,
    now: Instant,
    control_offset: u64,
}

impl Daemon {
    pub fn new(config: RuntimeConfig, mechanisms: Mechanisms) -> Self {
        let policy = LifecyclePolicy::new(config.policy);
        Self {
            config,
            policy,
            scopes: mechanisms.scopes,
            freezer: mechanisms.freezer,
            reclaimer: mechanisms.reclaimer,
            killer: mechanisms.killer,
            memory: mechanisms.memory,
            notifier: mechanisms.notifier,
            apps: BTreeMap::new(),
            timers: BTreeMap::new(),
            log: Vec::new(),
            kills: Vec::new(),
            now: Instant::now(),
            control_offset: 0,
        }
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub fn apps(&self) -> impl Iterator<Item = &AppProcess> {
        self.apps.values()
    }

    pub fn log(&self) -> &[LogEntry] {
        &self.log
    }

    pub fn kills(&self) -> &[KillRecord] {
        &self.kills
    }

    pub fn state_of(&self, id: &AppId) -> Option<AppState> {
        self.apps.get(id).map(|a| a.state)
    }

    pub fn scope_path(&self, id: &AppId) -> PathBuf {
        AppScope::new(id.clone()).path(&self.config.cgroup_root)
    }

    /// Register a launched app: one scope, one entry.
    pub fn register(&mut self, id: AppId, pid: Option<u32>, now: Instant) -> io::Result<()> {
        let scope = self.scopes.ensure_scope(&id)?;
        if let Some(pid) = pid {
            self.scopes.attach(&id, pid)?;
        }
        self.apps
            .entry(id.clone())
            .or_insert_with(|| AppProcess::new(id, scope.path(&self.config.cgroup_root), now));
        self.now = now;
        self.persist();
        Ok(())
    }

    fn refresh_memory(&mut self, id: &AppId) {
        let path = self.scope_path(id);
        let current = self.memory.memory_current(&path).unwrap_or(0);
        if let Some(app) = self.apps.get_mut(id) {
            app.memory_current = current;
        }
    }

    fn snapshot(&mut self) -> Vec<AppProcess> {
        let ids: Vec<AppId> = self.apps.keys().cloned().collect();
        for id in ids {
            self.refresh_memory(&id);
        }
        self.apps.values().cloned().collect()
    }

    /// Feed one event through the policy and apply the resulting actions.
    pub fn handle(&mut self, event: Event, now: Instant) -> Vec<Action> {
        self.now = now;
        // Protection reasons are part of the registry state, not an action:
        // apply them before the policy takes its snapshot.
        if let Event::ProtectionChanged { id, reason, active } = &event {
            if let Some(app) = self.apps.get_mut(id) {
                if *active {
                    app.add_protection(*reason);
                } else {
                    app.remove_protection(*reason);
                }
            }
        }
        let apps = self.snapshot();
        let actions = self.policy.on_event_at(event, &apps, now);
        for action in &actions {
            self.apply(action.clone());
        }
        actions
    }

    /// Fire timers whose deadline has passed. Returns the number fired.
    pub fn poll_timers(&mut self, now: Instant) -> usize {
        self.now = now;
        let due: Vec<(AppId, TimerKind)> = self
            .timers
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for (id, timer) in &due {
            self.timers.remove(&(id.clone(), *timer));
        }
        let count = due.len();
        for (id, timer) in due {
            self.handle(Event::TimerElapsed { id, timer }, now);
        }
        count
    }

    /// Next timer deadline, used by the event loop to sleep exactly as long
    /// as needed.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.timers.values().copied().min()
    }

    pub fn set_power_source(&mut self, source: PowerSourceKind, now: Instant) {
        self.handle(Event::PowerSourceChanged(source), now);
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::SetState { id, to, reason } => self.set_state(&id, to, reason),
            Action::Reclaim { id, bytes } => {
                let path = self.scope_path(&id);
                match self.reclaimer.reclaim(&path, bytes) {
                    Ok(requested) => {
                        self.push_log(Some(&id), format!(
                            "reclaim requested={}B reason=hibernate",
                            requested
                        ));
                    }
                    Err(e) => self.push_log(Some(&id), format!("reclaim failed: {e}")),
                }
            }
            Action::Kill { id, reason } => {
                let path = self.scope_path(&id);
                let (memory, band) = self
                    .apps
                    .get(&id)
                    .map(|a| (a.memory_current, a.band()))
                    .unwrap_or((0, Band::Terminated));
                match self.killer.kill(&path) {
                    Ok(killed) => {
                        self.kills.push(KillRecord {
                            id: id.clone(),
                            reason,
                            epoch_secs: epoch_secs(),
                            memory_current: memory,
                            band,
                        });
                        self.push_log(Some(&id), format!("kill reason={reason:?} pids={killed}"));
                    }
                    Err(e) => self.push_log(Some(&id), format!("kill failed: {e}")),
                }
            }
            Action::Notify { id, message } => match self.notifier.notify(&id, message) {
                Ok(ack) => {
                    if ack == Ack::Timeout {
                        self.push_log(Some(&id), format!(
                            "lifecycle {} ack=timeout",
                            message.wire_name()
                        ));
                    }
                }
                Err(NotifyError::NoClient) => {}
                Err(e) => self.push_log(Some(&id), format!("notify failed: {e}")),
            },
            Action::Schedule { id, timer, after } => {
                let deadline = self.now + after;
                self.timers.insert((id.clone(), timer), deadline);
                self.push_log(Some(&id), format!(
                    "timer {} scheduled after {:?}",
                    timer_name(timer),
                    after
                ));
            }
            Action::CancelTimers { id } => {
                self.timers.retain(|(app, _), _| app != &id);
            }
            Action::SetCpuWeight { id, weight } => {
                if !self.config.dry_run {
                    let path = self.scope_path(&id).join("cpu.weight");
                    let _ = fs::write(path, weight.to_string());
                }
                self.push_log(Some(&id), format!("cpu.weight={weight}"));
            }
            Action::Log { id, text } => {
                self.push_log(id.as_ref(), text);
            }
        }
    }

    fn set_state(&mut self, id: &AppId, to: AppState, reason: StateReason) {
        let Some((from, memory)) = self.apps.get(id).map(|a| (a.state, a.memory_current)) else {
            self.push_log(Some(id), "state change for unknown app ignored".into());
            return;
        };
        if from == to {
            return;
        }
        if let Err(error) = transition_is_legal(from, to) {
            self.push_log(Some(id), format!("invalid transition {}->{}: {error}", from, to));
            return;
        }

        let scope = self.scope_path(id);
        match to {
            AppState::Suspended => {
                // Policy already asked for WillSuspend before this action.
                if let Err(e) = self.freezer.freeze(&scope) {
                    self.push_log(Some(id), format!("freeze failed: {e}"));
                    return;
                }
            }
            AppState::Hibernated | AppState::Foreground | AppState::Visible
                if from.is_frozen() =>
            {
                if let Err(e) = self.freezer.thaw(&scope) {
                    self.push_log(Some(id), format!("thaw failed: {e}"));
                }
            }
            AppState::Terminated => {
                let _ = self.scopes.remove_scope(id);
            }
            _ => {}
        }

        if let Some(app) = self.apps.get_mut(id) {
            app.state = to;
            if to == AppState::Foreground {
                app.last_focused = self.now;
            }
        }
        self.push_log(Some(id), format!(
            "state {from}->{to} reason={} memory={memory}",
            reason.as_str()
        ));

        if to == AppState::Terminated {
            self.timers.retain(|(app, _), _| app != id);
            self.apps.remove(id);
        }
        self.persist();
    }

    fn push_log(&mut self, id: Option<&AppId>, text: String) {
        self.log.push(LogEntry {
            epoch_secs: epoch_secs(),
            app: id.map(|i| i.as_str().to_string()),
            text,
        });
    }

    fn registry_path(&self) -> PathBuf {
        self.config.state_dir.join("apps.json")
    }

    /// Persist identities so a daemon restart can rebuild its state (D3 §8).
    pub fn persist(&self) {
        if fs::create_dir_all(&self.config.state_dir).is_err() {
            return;
        }
        let entries: Vec<(String, String)> = self
            .apps
            .values()
            .map(|a| (a.id.as_str().to_string(), a.state.as_str().to_string()))
            .collect();
        if let Ok(text) = serde_json::to_string_pretty(&entries) {
            let _ = fs::write(self.registry_path(), text);
        }
    }

    /// Restore identities after a crash/restart. Processes stay in whatever
    /// freezing state the kernel kept them in; running apps return to
    /// Background until the compositor reports focus.
    pub fn restore(&mut self, now: Instant) -> io::Result<usize> {
        let path = self.registry_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Ok(0);
        };
        let Ok(entries) = serde_json::from_str::<Vec<(String, String)>>(&text) else {
            return Ok(0);
        };
        let mut restored = 0;
        for (raw, state) in entries {
            let Ok(id) = AppId::new(raw) else { continue };
            if state == AppState::Terminated.as_str() {
                continue;
            }
            let scope = self.scopes.ensure_scope(&id)?;
            let frozen = self.freezer.is_frozen(&scope.path(&self.config.cgroup_root)).unwrap_or(false);
            let mut app = AppProcess::new(id.clone(), scope.path(&self.config.cgroup_root), now);
            app.state = if frozen { AppState::Suspended } else { AppState::Background };
            app.memory_current = self
                .memory
                .memory_current(&scope.path(&self.config.cgroup_root))
                .unwrap_or(0);
            self.apps.insert(id, app);
            restored += 1;
        }
        self.push_log(None, format!("restored {restored} app(s) from registry"));
        Ok(restored)
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    pub fn policy_mut(&mut self) -> &mut LifecyclePolicy {
        &mut self.policy
    }

    /// Apply the background policy written by `aether-powerd`.
    pub fn apply_background_policy(
        &mut self,
        grace: Duration,
        hibernate_after: Duration,
        source: PowerSourceKind,
    ) {
        self.policy.set_source(source);
        self.policy.set_background_policy(grace, hibernate_after);
        self.push_log(
            None,
            format!(
                "background policy applied: grace={:?} hibernate={:?} source={source:?}",
                grace, hibernate_after
            ),
        );
    }

    /// User-requested suspend (`aetherctl apps suspend`).
    pub fn suspend_app(&mut self, id: &AppId, now: Instant) -> bool {
        self.now = now;
        let Some(state) = self.state_of(id) else {
            return false;
        };
        if !state.is_alive() {
            return false;
        }
        if state.is_on_screen() {
            self.set_state(id, AppState::Background, StateReason::WindowHidden);
        }
        self.set_state(id, AppState::Suspended, StateReason::UserQuit);
        self.state_of(id) == Some(AppState::Suspended)
    }

    /// User-requested quit (`aetherctl apps quit`).
    pub fn quit_app(&mut self, id: &AppId, now: Instant) -> bool {
        self.now = now;
        if self.state_of(id).is_none() {
            return false;
        }
        self.handle(Event::UserQuit(id.clone()), now);
        self.state_of(id).is_none()
    }

    /// Read append-only control commands written by `aetherctl`.
    /// Format: one JSON object per line, `{"command":"suspend","app":"..."}`.
    pub fn process_control_file(&mut self, path: &Path, now: Instant) -> usize {
        let Ok(text) = fs::read_to_string(path) else {
            return 0;
        };
        let mut handled = 0;
        let mut cursor = 0u64;
        for line in text.lines() {
            let start = cursor;
            cursor += line.len() as u64 + 1;
            if start < self.control_offset {
                continue;
            }
            #[derive(Deserialize)]
            struct Control {
                command: String,
                app: String,
            }
            let Ok(command) = serde_json::from_str::<Control>(line) else {
                continue;
            };
            let Ok(id) = AppId::new(command.app) else {
                continue;
            };
            let ok = match command.command.as_str() {
                "suspend" => self.suspend_app(&id, now),
                "quit" => self.quit_app(&id, now),
                _ => false,
            };
            self.push_log(Some(&id), format!("control {} ok={ok}", command.command));
            handled += 1;
        }
        self.control_offset = cursor;
        handled
    }

    pub fn timers(&self) -> impl Iterator<Item = ((&AppId, TimerKind), &Instant)> {
        self.timers.iter().map(|(k, v)| ((&k.0, k.1), v))
    }

    /// Metrics snapshot used by Activity Manager, benchmarks and powerd.
    pub fn metrics(&mut self) -> MetricsSnapshot {
        let apps = self.snapshot();
        let mut by_state: BTreeMap<String, usize> = BTreeMap::new();
        let mut inactive_memory = 0u64;
        let mut total_memory = 0u64;
        let mut rows = Vec::new();
        for app in &apps {
            *by_state.entry(app.state.as_str().to_string()).or_insert(0) += 1;
            total_memory += app.memory_current;
            if app.state.is_inactive() {
                inactive_memory += app.memory_current;
            }
            rows.push(AppRow {
                id: app.id.clone(),
                state: app.state,
                band: app.band(),
                memory_current: app.memory_current,
                pids: app.pids.clone(),
                protect: app.protect.clone(),
                idle_secs: self.now.saturating_duration_since(app.last_focused).as_secs(),
            });
        }
        MetricsSnapshot {
            rows,
            by_state,
            total_memory,
            inactive_memory,
            inactive_share: if total_memory == 0 {
                0.0
            } else {
                inactive_memory as f64 / total_memory as f64
            },
            kills: self.kills.clone(),
        }
    }
}

fn timer_name(timer: TimerKind) -> &'static str {
    match timer {
        TimerKind::Grace => "grace",
        TimerKind::Hibernate => "hibernate",
    }
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One app row for the Activity Manager and benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRow {
    pub id: AppId,
    pub state: AppState,
    pub band: Band,
    pub memory_current: u64,
    pub pids: Vec<u32>,
    pub protect: Vec<crate::state::ProtectReason>,
    pub idle_secs: u64,
}

/// Process-management metrics (D3 section 9, G-PM1/G-PM2 input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub rows: Vec<AppRow>,
    pub by_state: BTreeMap<String, usize>,
    pub total_memory: u64,
    pub inactive_memory: u64,
    pub inactive_share: f64,
    pub kills: Vec<KillRecord>,
}

impl MetricsSnapshot {
    /// G-PM2: inactive apps hold ≤ 25% of used memory (B1).
    pub fn inactive_share_limit() -> f64 {
        0.25
    }

    pub fn g_pm2_passes(&self) -> bool {
        self.inactive_share <= Self::inactive_share_limit()
    }

    /// G-PM4: foreground app killed zero times.
    pub fn g_pm4_passes(&self, foreground: &[AppId]) -> bool {
        !self
            .kills
            .iter()
            .any(|kill| foreground.contains(&kill.id) && kill.reason == KillReason::MemoryPressure)
    }
}

/// Convenience: build a fully wired daemon for a real system.
pub fn system_daemon(config: RuntimeConfig) -> Daemon {
    use crate::cgroup::CgroupFs;
    use crate::identity::CgroupScopeManager;
    use crate::protocol::SocketNotifier;

    let cgroup = CgroupFs::at(config.cgroup_root.clone());
    Daemon::new(
        config.clone(),
        Mechanisms {
            scopes: Box::new(CgroupScopeManager::at(config.cgroup_root.clone())),
            freezer: Box::new(cgroup.clone()),
            reclaimer: Box::new(cgroup.clone()),
            killer: Box::new(cgroup.clone()),
            memory: Box::new(cgroup),
            notifier: Box::new(SocketNotifier::at(config.socket_dir.clone())),
        },
    )
}

/// True when `path` is inside the configured cgroup root (kill safety rule).
pub fn is_managed_scope(root: &Path, scope: &Path) -> bool {
    scope.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgroup::{MemoryReader, ZramTuner};
    use crate::identity::MemoryScopeManager;
    use crate::policy::Policy;
    use crate::protocol::{Ack, LifecycleMessage};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder {
        freezes: AtomicU32,
        thaws: AtomicU32,
        reclaims: Mutex<Vec<u64>>,
        kills: Mutex<Vec<PathBuf>>,
        notifications: Mutex<Vec<LifecycleMessage>>,
    }

    struct FakeMem(Arc<Mutex<u64>>);

    impl MemoryReader for FakeMem {
        fn memory_current(&self, _scope: &Path) -> io::Result<u64> {
            Ok(*self.0.lock().unwrap())
        }
    }

    struct FakeFreezer(Arc<Recorder>);
    impl Freezer for FakeFreezer {
        fn freeze(&self, _scope: &Path) -> io::Result<()> {
            self.0.freezes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn thaw(&self, _scope: &Path) -> io::Result<()> {
            self.0.thaws.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn is_frozen(&self, _scope: &Path) -> io::Result<bool> {
            Ok(false)
        }
    }

    struct FakeReclaimer(Arc<Recorder>);
    impl Reclaimer for FakeReclaimer {
        fn reclaim(&self, _scope: &Path, bytes: u64) -> io::Result<u64> {
            self.0.reclaims.lock().unwrap().push(bytes);
            Ok(bytes)
        }
    }

    struct FakeKiller(Arc<Recorder>);
    impl Killer for FakeKiller {
        fn kill(&self, scope: &Path) -> io::Result<u32> {
            self.0.kills.lock().unwrap().push(scope.to_path_buf());
            Ok(1)
        }
    }

    struct FakeNotifier(Arc<Recorder>);
    impl LifecycleNotifier for FakeNotifier {
        fn notify(&self, _id: &AppId, message: LifecycleMessage) -> Result<Ack, NotifyError> {
            self.0.notifications.lock().unwrap().push(message);
            Ok(Ack::Accepted)
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-daemon-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn harness(tag: &str) -> (Daemon, Arc<Recorder>, PathBuf) {
        let dir = temp_dir(tag);
        let recorder = Arc::new(Recorder::default());
        let memory = Arc::new(Mutex::new(0u64));
        let config = RuntimeConfig {
            cgroup_root: dir.join("cgroup"),
            socket_dir: dir.join("sockets"),
            state_dir: dir.clone(),
            dry_run: true,
            policy: PolicyConfig::default(),
        };
        let daemon = Daemon::new(
            config,
            Mechanisms {
                scopes: Box::new(MemoryScopeManager::default()),
                freezer: Box::new(FakeFreezer(recorder.clone())),
                reclaimer: Box::new(FakeReclaimer(recorder.clone())),
                killer: Box::new(FakeKiller(recorder.clone())),
                memory: Box::new(FakeMem(memory)),
                notifier: Box::new(FakeNotifier(recorder.clone())),
            },
        );
        (daemon, recorder, dir)
    }

    fn id() -> AppId {
        AppId::new("org.example.Mail").unwrap()
    }

    #[test]
    fn app_gets_exactly_one_scope() {
        let (mut daemon, _rec, dir) = harness("scope");
        let now = Instant::now();
        daemon.register(id(), Some(42), now).unwrap();
        daemon.register(id(), Some(43), now).unwrap();
        assert_eq!(daemon.apps().count(), 1);
        assert!(dir.join("apps.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hidden_app_is_suspended_after_grace_then_hibernated() {
        let (mut daemon, rec, dir) = harness("lifecycle");
        let now = Instant::now();
        daemon.register(id(), Some(42), now).unwrap();
        daemon.handle(Event::WindowShown(id()), now);
        daemon.handle(Event::WindowHidden(id()), now);
        assert_eq!(daemon.state_of(&id()), Some(AppState::Background));

        let after_grace = now + Duration::from_secs(31);
        assert_eq!(daemon.poll_timers(after_grace), 1);
        assert_eq!(daemon.state_of(&id()), Some(AppState::Suspended));
        assert_eq!(rec.freezes.load(Ordering::SeqCst), 1);
        assert!(rec
            .notifications
            .lock()
            .unwrap()
            .contains(&LifecycleMessage::WillSuspend));

        let after_hibernate = after_grace + Duration::from_secs(11 * 60);
        assert_eq!(daemon.poll_timers(after_hibernate), 1);
        assert_eq!(daemon.state_of(&id()), Some(AppState::Hibernated));
        assert_eq!(rec.reclaims.lock().unwrap().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn focus_cancels_the_grace_timer() {
        let (mut daemon, rec, dir) = harness("cancel");
        let now = Instant::now();
        daemon.register(id(), Some(42), now).unwrap();
        daemon.handle(Event::WindowShown(id()), now);
        daemon.handle(Event::WindowHidden(id()), now);
        daemon.handle(Event::FocusChanged { to: Some(id()) }, now);

        assert_eq!(daemon.poll_timers(now + Duration::from_secs(120)), 0);
        assert_eq!(daemon.state_of(&id()), Some(AppState::Foreground));
        assert_eq!(rec.freezes.load(Ordering::SeqCst), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn critical_pressure_kills_background_not_foreground() {
        let (mut daemon, rec, dir) = harness("kill");
        let now = Instant::now();
        let fg = AppId::new("org.example.Fg").unwrap();
        daemon.register(fg.clone(), Some(1), now).unwrap();
        daemon.register(id(), Some(2), now).unwrap();
        daemon.handle(Event::FocusChanged { to: Some(fg.clone()) }, now);
        daemon.handle(Event::WindowShown(id()), now);

        let actions = daemon.handle(
            Event::PressureChanged(crate::pressure::PressureLevel::Critical),
            now,
        );
        assert!(actions.iter().any(|a| matches!(a, Action::Kill { .. })));
        let kills = rec.kills.lock().unwrap();
        assert_eq!(kills.len(), 1);
        assert!(kills[0].to_string_lossy().contains("org.example.Mail"));
        assert_eq!(daemon.state_of(&id()), None);
        assert_eq!(daemon.state_of(&fg), Some(AppState::Foreground));
        assert!(daemon.metrics().g_pm4_passes(&[fg]));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn metrics_report_inactive_share() {
        let (mut daemon, _rec, dir) = harness("metrics");
        let now = Instant::now();
        daemon.register(id(), Some(42), now).unwrap();
        daemon.handle(Event::WindowShown(id()), now);
        daemon.handle(Event::WindowHidden(id()), now);
        daemon.poll_timers(now + Duration::from_secs(31));
        let metrics = daemon.metrics();
        assert_eq!(metrics.by_state.get("Suspended"), Some(&1));
        assert!(metrics.g_pm2_passes());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn registry_restore_rebuilds_apps() {
        let (mut daemon, _rec, dir) = harness("restore");
        let now = Instant::now();
        daemon.register(id(), Some(42), now).unwrap();
        daemon.handle(Event::WindowShown(id()), now);
        daemon.handle(Event::WindowHidden(id()), now);
        daemon.persist();

        let (mut restarted, _rec2, _dir2) = harness("restore2");
        // Point the restarted daemon at the same state directory.
        restarted.config.state_dir = dir.clone();
        let restored = restarted.restore(now).unwrap();
        assert_eq!(restored, 1);
        assert_eq!(restarted.state_of(&id()), Some(AppState::Background));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn illegal_state_change_is_logged_and_ignored() {
        let (mut daemon, rec, dir) = harness("illegal");
        let now = Instant::now();
        daemon.register(id(), Some(42), now).unwrap();
        // Background -> Hibernated is not in the transition table.
        daemon.apply(Action::SetState {
            id: id(),
            to: AppState::Hibernated,
            reason: StateReason::PressureElevated,
        });
        assert_eq!(daemon.state_of(&id()), Some(AppState::Background));
        assert!(daemon
            .log()
            .iter()
            .any(|l| l.text.contains("invalid transition")));
        assert_eq!(rec.reclaims.lock().unwrap().len(), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn zram_tuner_is_wired_into_policy_defaults() {
        let config = PolicyConfig::default();
        assert_eq!(config.reclaim_percent, 80);
        assert_eq!(config.grace_period_ac, Duration::from_secs(30));
        assert_eq!(config.grace_period_battery, Duration::from_secs(10));
        let tuner = ZramTuner::dry_run();
        assert!(tuner.tune(None).is_ok());
    }

    #[test]
    fn managed_scope_check_blocks_foreign_paths() {
        let root = Path::new("/sys/fs/cgroup");
        assert!(is_managed_scope(
            root,
            Path::new("/sys/fs/cgroup/aether.slice/aether-apps.slice/x.scope")
        ));
        assert!(!is_managed_scope(root, Path::new("/proc/1")));
    }

    #[test]
    fn policy_is_exposed_for_daemon_loop() {
        let (mut daemon, _rec, dir) = harness("policy");
        let id = id();
        daemon.register(id.clone(), None, Instant::now()).unwrap();
        let _ = daemon.policy_mut();
        let _ = Policy::on_event(
            &mut LifecyclePolicy::new(PolicyConfig::default()),
            Event::AppExited(id),
            &[],
        );
        let _ = fs::remove_dir_all(dir);
    }
}
