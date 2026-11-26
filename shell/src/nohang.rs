//! G-NH2 no-hang shell surface (P3-T08 ↔ P4, S3).
//!
//! The compositor never waits for an app (P3-T08). It reports the health of each
//! shell client in its journal, and the shell reacts; no new protocol is
//! needed:
//!
//! * `AETHER_COMPOSITOR_APP_STATE: client=… state=not_responding after_ms=…`
//!   — show "Not responding" with Wait / Force Quit.
//! * `AETHER_COMPOSITOR_APP_STATE: client=… state=responsive recovered=true`
//!   — the app answered a late ping: close the dialog.
//! * `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout|ok …` — the shell stopped
//!   committing its layer surfaces; the service manager restarts it.
//!
//! Two pieces live here, both pure state machines so they unit-test without a
//! display server:
//!
//! 1. [`NoHangDialog`] — one dialog at a time, Wait waives it for
//!    [`DEFAULT_WAIT_GRACE`] and re-offers while the app stays hung, Force Quit
//!    returns the client the host must terminate, and a recovery closes it on
//!    its own. Every decision prints an `AETHER_SHELL_NOHANG_*` marker so the
//!    acceptance harness can prove which button was pressed.
//! 2. [`StartLedger`] + [`WatchdogReport`] — the shell restart flow. A start
//!    that never reaches "session ready" is counted; three consecutive failed
//!    starts (or three compositor shell-watchdog timeouts) enter safe mode,
//!    which resolves the glass tier to Minimal and shows the safe-mode banner
//!    until the user resets it.
//!
//! `aether-shell --nohang-check`, `--start-begin`, `--start-confirm` and
//! `--safe-mode-reset` are the stable interface for the session launcher and
//! the on-device acceptance harness.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aether_ui::theme::Tier;

use crate::config::SettingsStore;
use crate::host::field;

/// Consecutive failed shell starts before safe mode (G-NH2, F11 section 3).
pub const MAX_FAILED_STARTS: u32 = 3;
/// How long "Wait" suppresses the dialog before it is offered again.
pub const DEFAULT_WAIT_GRACE: Duration = Duration::from_secs(15);
/// G-NH2 requires "Not responding" within 6 s of the app going quiet.
pub const MAX_DIALOG_DELAY_MS: u64 = 6_000;
/// Shell start ledger, relative to the settings root.
pub const SESSION_FILE: &str = "shell/session.json";

// --------------------------------------------------------------- dialog ---

/// A health transition parsed out of the compositor journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HangEvent {
    /// The client missed its Wayland ping timeout.
    NotResponding { client: u64, after_ms: u64 },
    /// A late pong: the client is responsive again.
    Recovered { client: u64 },
}

/// The app the dialog is currently about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HungApp {
    pub client: u64,
    /// `.desktop` id or executable name (no spaces, marker-safe).
    pub id: String,
    /// Human name shown in the dialog title.
    pub name: String,
    /// Time from the unanswered ping to the "not responding" decision.
    pub after_ms: u64,
}

/// What the user chose (or what the shell can offer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    Wait,
    ForceQuit,
}

impl DialogAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::ForceQuit => "force_quit",
        }
    }
}

/// What a state transition did, for logging and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEvent {
    /// The dialog is (now) on screen for this client.
    Shown(u64),
    /// Another app is hung, but the dialog stays on the current client.
    Queued(u64),
    /// The dialog closed for this client because it recovered.
    Recovered(u64),
    /// Nothing on screen changed.
    None,
}

#[derive(Debug, Clone)]
struct Pending {
    after_ms: u64,
    /// Set by Wait: re-offer only once this instant has passed.
    waived_until: Option<Instant>,
}

impl Pending {
    fn is_offerable(&self, now: Instant) -> bool {
        self.waived_until.map(|until| now >= until).unwrap_or(true)
    }
}

/// The "Not responding" dialog: one client at a time, FIFO by client id.
#[derive(Debug)]
pub struct NoHangDialog {
    pending: BTreeMap<u64, Pending>,
    current: Option<u64>,
    grace: Duration,
    names: BTreeMap<u64, (String, String)>,
    /// Dialogs raised (a Wait re-offer that expires counts again).
    pub prompts: u32,
    pub waits: u32,
    pub force_quits: u32,
    pub recoveries: u32,
}

impl NoHangDialog {
    pub fn new(grace: Duration) -> Self {
        Self {
            pending: BTreeMap::new(),
            current: None,
            grace,
            names: BTreeMap::new(),
            prompts: 0,
            waits: 0,
            force_quits: 0,
            recoveries: 0,
        }
    }

    pub fn with_default_grace() -> Self {
        Self::new(DEFAULT_WAIT_GRACE)
    }

    /// Register the app identity a client id maps to (the compositor only
    /// knows client ids).
    pub fn register_name(&mut self, client: u64, id: &str, name: &str) {
        self.names
            .insert(client, (id.to_string(), name.to_string()));
    }

    pub fn grace(&self) -> Duration {
        self.grace
    }

    pub fn is_open(&self) -> bool {
        self.current.is_some()
    }

    /// The app the dialog is about, if it is on screen.
    pub fn current(&self) -> Option<HungApp> {
        let client = self.current?;
        let (id, name) = self
            .names
            .get(&client)
            .cloned()
            .unwrap_or_else(|| (format!("client-{client}"), format!("Application {client}")));
        Some(HungApp {
            client,
            id,
            name,
            after_ms: self.pending.get(&client).map(|p| p.after_ms).unwrap_or(0),
        })
    }

    /// Clients still waiting for the dialog (including the visible one).
    pub fn pending_clients(&self) -> Vec<u64> {
        self.pending.keys().copied().collect()
    }

    /// Apply one compositor health marker.
    pub fn observe(&mut self, event: HangEvent, now: Instant) -> DialogEvent {
        match event {
            HangEvent::NotResponding { client, after_ms } => {
                self.pending.insert(
                    client,
                    Pending {
                        after_ms,
                        waived_until: None,
                    },
                );
                if self.current.is_none() {
                    self.raise(client);
                    DialogEvent::Shown(client)
                } else {
                    DialogEvent::Queued(client)
                }
            }
            HangEvent::Recovered { client } => {
                if self.pending.remove(&client).is_none() {
                    return DialogEvent::None;
                }
                self.recoveries += 1;
                if self.current == Some(client) {
                    self.current = None;
                    self.promote(now);
                }
                DialogEvent::Recovered(client)
            }
        }
    }

    /// Feed the dialog clock: re-offer a waived app whose grace has elapsed, or
    /// show the next queued app.
    pub fn tick(&mut self, now: Instant) -> Option<u64> {
        if self.current.is_some() {
            return None;
        }
        let client = self
            .pending
            .iter()
            .find(|(_, pending)| pending.is_offerable(now))
            .map(|(client, _)| *client)?;
        self.raise(client);
        Some(client)
    }

    /// Apply the user's choice to the visible dialog. Returns the client the
    /// action applied to (`ForceQuit` means the caller must terminate it).
    pub fn act(&mut self, action: DialogAction, now: Instant) -> Option<(DialogAction, u64)> {
        let client = self.current?;
        match action {
            DialogAction::Wait => {
                if let Some(pending) = self.pending.get_mut(&client) {
                    pending.waived_until = Some(now + self.grace);
                }
                self.waits += 1;
                self.current = None;
                self.promote(now);
                Some((action, client))
            }
            DialogAction::ForceQuit => {
                self.pending.remove(&client);
                self.force_quits += 1;
                self.current = None;
                self.promote(now);
                Some((action, client))
            }
        }
    }

    fn raise(&mut self, client: u64) {
        if let Some(pending) = self.pending.get_mut(&client) {
            pending.waived_until = None;
        }
        self.current = Some(client);
        self.prompts += 1;
    }

    fn promote(&mut self, now: Instant) {
        let _ = self.tick(now);
    }

    fn app_id(&self, client: u64) -> String {
        self.names
            .get(&client)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| format!("client-{client}"))
    }

    /// `AETHER_SHELL_NOHANG_DIALOG: client=7 app=aether-text after_ms=5100`
    pub fn dialog_line(&self, client: u64) -> String {
        let after_ms = self.pending.get(&client).map(|p| p.after_ms).unwrap_or(0);
        format!(
            "AETHER_SHELL_NOHANG_DIALOG: client={client} app={} after_ms={after_ms}",
            self.app_id(client)
        )
    }

    /// `AETHER_SHELL_NOHANG_ACTION: client=7 app=aether-text action=force_quit`
    pub fn action_line(&self, client: u64, action: DialogAction) -> String {
        let grace = match action {
            DialogAction::Wait => format!(" grace_ms={}", self.grace.as_millis()),
            DialogAction::ForceQuit => String::new(),
        };
        format!(
            "AETHER_SHELL_NOHANG_ACTION: client={client} app={} action={}{grace}",
            self.app_id(client),
            action.as_str()
        )
    }

    /// `AETHER_SHELL_NOHANG_RECOVERED: client=7 app=aether-text`
    pub fn recovered_line(&self, client: u64) -> String {
        format!(
            "AETHER_SHELL_NOHANG_RECOVERED: client={client} app={}",
            self.app_id(client)
        )
    }

    /// `AETHER_SHELL_NOHANG_SUMMARY: prompts=… waits=… force_quits=… recoveries=… open=…`
    pub fn summary_line(&self) -> String {
        format!(
            "AETHER_SHELL_NOHANG_SUMMARY: prompts={} waits={} force_quits={} recoveries={} open={}",
            self.prompts,
            self.waits,
            self.force_quits,
            self.recoveries,
            u8::from(self.is_open())
        )
    }
}

// ------------------------------------------------------------- watchdog ---

/// The compositor's shell-commit watchdog, read back from its journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogReport {
    pub timeouts: u32,
    /// Last reported state (`timeout` or `ok`).
    pub last_state: Option<String>,
    pub recovered: bool,
    pub last_idle_ms: Option<u64>,
    pub timeout_ms: Option<u64>,
}

impl Default for WatchdogReport {
    fn default() -> Self {
        WatchdogReport {
            timeouts: 0,
            last_state: None,
            recovered: false,
            last_idle_ms: None,
            timeout_ms: None,
        }
    }
}

impl WatchdogReport {
    pub fn scan(log: &str) -> WatchdogReport {
        let mut report = WatchdogReport::default();
        for line in log.lines() {
            let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_SHELL_WATCHDOG:") else {
                continue;
            };
            let seen = field(rest, "timeouts").and_then(|value| value.parse().ok());
            match field(rest, "state") {
                Some("timeout") => {
                    report.last_state = Some("timeout".to_string());
                    report.recovered = false;
                    report.timeouts = seen.unwrap_or(report.timeouts + 1);
                    if let Some(idle) = field(rest, "idle_ms").and_then(|value| value.parse().ok())
                    {
                        report.last_idle_ms = Some(idle);
                    }
                }
                Some("ok") => {
                    report.last_state = Some("ok".to_string());
                    report.recovered = true;
                    if let Some(timeouts) = seen {
                        report.timeouts = timeouts;
                    }
                }
                _ => continue,
            }
            if let Some(timeout) = field(rest, "timeout_ms").and_then(|value| value.parse().ok()) {
                report.timeout_ms = Some(timeout);
            }
        }
        report
    }

    /// The shell stopped committing and has not reported a recovery since: the
    /// service manager has to restart it (G-NH2: within 5 s).
    pub fn needs_restart(&self) -> bool {
        self.last_state.as_deref() == Some("timeout")
    }

    /// Three stalls arm safe mode on the next start, like three failed starts.
    pub fn escalates(&self) -> bool {
        self.timeouts >= MAX_FAILED_STARTS
    }

    fn fields(&self) -> String {
        format!(
            "timeouts={} last_state={} recovered={} idle_ms={} timeout_ms={} restart_required={}",
            self.timeouts,
            self.last_state.as_deref().unwrap_or("none"),
            self.recovered,
            self.last_idle_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.timeout_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.needs_restart()
        )
    }
}

// --------------------------------------------------------- start ledger ---

/// What `begin_start` recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOutcome {
    pub attempt: u32,
    pub safe_mode: bool,
    /// The previous start never reached "session ready".
    pub previous_failed: bool,
}

/// Consecutive start attempts, persisted so a restart loop is visible across
/// processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartLedger {
    /// Starts since the last confirmed one (`0` after a good start).
    pub attempts: u32,
    /// The previous start reached "session ready".
    pub confirmed: bool,
    pub safe_mode: bool,
    /// Watchdog timeouts the previous compositor run reported.
    pub watchdog_timeouts: u32,
}

impl Default for StartLedger {
    fn default() -> Self {
        StartLedger {
            attempts: 0,
            confirmed: true,
            safe_mode: false,
            watchdog_timeouts: 0,
        }
    }
}

impl StartLedger {
    /// `<settings root>/shell/session.json`.
    pub fn path(store: &SettingsStore) -> PathBuf {
        store.root().join(SESSION_FILE)
    }

    /// Read the ledger; a missing or corrupt file is a clean first start.
    pub fn load(path: &Path) -> StartLedger {
        let Ok(text) = std::fs::read_to_string(path) else {
            return StartLedger::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return StartLedger::default();
        };
        StartLedger {
            attempts: value
                .get("attempts")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
            confirmed: value
                .get("confirmed")
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
            safe_mode: value
                .get("safe_mode")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            watchdog_timeouts: value
                .get("watchdog_timeouts")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32,
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let text = serde_json::json!({
            "attempts": self.attempts,
            "confirmed": self.confirmed,
            "safe_mode": self.safe_mode,
            "watchdog_timeouts": self.watchdog_timeouts,
        })
        .to_string();
        std::fs::write(path, format!("{text}\n"))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    /// Record a start before the session binds its surfaces. A previous start
    /// that never confirmed counts as failed; three in a row enter safe mode.
    pub fn begin_start(&mut self) -> StartOutcome {
        let previous_failed = !self.confirmed;
        self.attempts = if previous_failed {
            self.attempts.saturating_add(1)
        } else {
            1
        };
        self.confirmed = false;
        if self.attempts >= MAX_FAILED_STARTS {
            self.safe_mode = true;
        }
        StartOutcome {
            attempt: self.attempts,
            safe_mode: self.safe_mode,
            previous_failed,
        }
    }

    /// The session bound its surfaces: the start is good, clear the loop.
    /// Safe mode stays on until an explicit reset, so its banner is not lost
    /// just because the reduced session started fine.
    pub fn confirm_start(&mut self) {
        self.attempts = 0;
        self.confirmed = true;
    }

    /// Fold the compositor's watchdog markers into the ledger. Returns `true`
    /// when this newly armed safe mode.
    pub fn observe_watchdog(&mut self, report: &WatchdogReport) -> bool {
        self.watchdog_timeouts = report.timeouts;
        if report.escalates() && !self.safe_mode {
            self.safe_mode = true;
            return true;
        }
        false
    }

    /// Leave safe mode and start counting again (the banner's reset action).
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.confirmed = true;
        self.safe_mode = false;
        self.watchdog_timeouts = 0;
    }

    /// Why safe mode is on, in banner wording.
    pub fn safe_mode_reason(&self) -> String {
        if self.attempts >= MAX_FAILED_STARTS {
            format!("{MAX_FAILED_STARTS} starts failed in a row")
        } else if self.watchdog_timeouts >= MAX_FAILED_STARTS {
            format!("the shell stalled {MAX_FAILED_STARTS} times")
        } else {
            "safe mode is on".to_string()
        }
    }

    /// `AETHER_SHELL_SAFE_MODE: attempts=3 timeouts=0 reason=… effects=minimal`
    pub fn safe_mode_line(&self) -> String {
        format!(
            "AETHER_SHELL_SAFE_MODE: attempts={} timeouts={} reason={} effects=minimal\n",
            self.attempts,
            self.watchdog_timeouts,
            self.safe_mode_reason()
        )
    }

    /// `AETHER_SHELL_START: attempt=2 safe_mode=false previous=unconfirmed`
    pub fn start_line(&self, outcome: &StartOutcome) -> String {
        format!(
            "AETHER_SHELL_START: attempt={} safe_mode={} previous={}\n",
            outcome.attempt,
            outcome.safe_mode,
            if outcome.previous_failed {
                "unconfirmed"
            } else {
                "clean"
            }
        )
    }
}

/// Safe mode runs the reduced session: Minimal glass, no effect pipeline.
pub fn resolve_tier(safe_mode: bool, tier: Tier) -> Tier {
    if safe_mode {
        Tier::Minimal
    } else {
        tier
    }
}

// ------------------------------------------------------------ log check ---

/// Everything `--nohang-check` reads out of a compositor journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoHangReport {
    pub events: Vec<HangEvent>,
    pub watchdog: WatchdogReport,
}

impl NoHangReport {
    pub fn scan(log: &str) -> NoHangReport {
        NoHangReport {
            events: health_events(log),
            watchdog: WatchdogReport::scan(log),
        }
    }

    /// Clients that were marked not responding, with their detection delay.
    pub fn hangs(&self) -> Vec<(u64, u64)> {
        self.events
            .iter()
            .filter_map(|event| match event {
                HangEvent::NotResponding { client, after_ms } => Some((*client, *after_ms)),
                HangEvent::Recovered { .. } => None,
            })
            .collect()
    }

    pub fn recovered_clients(&self) -> Vec<u64> {
        self.events
            .iter()
            .filter_map(|event| match event {
                HangEvent::Recovered { client } => Some(*client),
                HangEvent::NotResponding { .. } => None,
            })
            .collect()
    }

    /// Harness-readable reasons; empty means the compositor reported a hang
    /// inside the G-NH2 bound and the shell is committing (or recovered).
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let hangs = self.hangs();
        if hangs.is_empty() {
            problems.push(
                "no AETHER_COMPOSITOR_APP_STATE marker: no app was detected as hung (wrong journal?)"
                    .to_string(),
            );
        }
        for (client, after_ms) in &hangs {
            if *after_ms > MAX_DIALOG_DELAY_MS {
                problems.push(format!(
                    "client={client} was marked not responding after {after_ms} ms; G-NH2 allows {MAX_DIALOG_DELAY_MS} ms"
                ));
            }
        }
        if self.watchdog.needs_restart() {
            problems.push(
                "AETHER_COMPOSITOR_SHELL_WATCHDOG last state=timeout: the shell stopped committing and the service manager must restart it"
                    .to_string(),
            );
        }
        problems
    }

    pub fn is_complete(&self) -> bool {
        self.problems().is_empty()
    }

    /// Marker lines plus a verdict for `--nohang-check`.
    pub fn summary(&self, ledger: &StartLedger) -> String {
        let mut out = String::new();
        for event in &self.events {
            match event {
                HangEvent::NotResponding { client, after_ms } => out.push_str(&format!(
                    "NOHANG_HANG client={client} after_ms={after_ms} within_limit={}\n",
                    *after_ms <= MAX_DIALOG_DELAY_MS
                )),
                HangEvent::Recovered { client } => {
                    out.push_str(&format!("NOHANG_RECOVERED client={client}\n"))
                }
            }
        }
        out.push_str(&format!("NOHANG_WATCHDOG {}\n", self.watchdog.fields()));
        out.push_str(&format!(
            "NOHANG_START attempts={} confirmed={} safe_mode={}\n",
            ledger.attempts, ledger.confirmed, ledger.safe_mode
        ));
        if ledger.safe_mode {
            out.push_str(&ledger.safe_mode_line());
        }
        let problems = self.problems();
        if problems.is_empty() {
            out.push_str("AETHER_SHELL_NOHANG_OK\n");
        } else {
            for problem in &problems {
                out.push_str(&format!("AETHER_SHELL_NOHANG_PROBLEM: {problem}\n"));
            }
            out.push_str("AETHER_SHELL_NOHANG_FAILED\n");
        }
        out
    }
}

/// Parse the compositor's health markers (`PING`/`PONG` and `ping_pending` are
/// ignored: only a decision matters to the dialog).
pub fn health_events(log: &str) -> Vec<HangEvent> {
    let mut events = Vec::new();
    for line in log.lines() {
        let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_APP_STATE:") else {
            continue;
        };
        let Some(client) = field(rest, "client").and_then(|value| value.parse().ok()) else {
            continue;
        };
        match field(rest, "state") {
            Some("not_responding") => events.push(HangEvent::NotResponding {
                client,
                after_ms: field(rest, "after_ms")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            }),
            Some("responsive") if field(rest, "recovered") == Some("true") => {
                events.push(HangEvent::Recovered { client })
            }
            _ => {}
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> NoHangDialog {
        let mut dialog = NoHangDialog::with_default_grace();
        dialog.register_name(7, "aether-text", "Aether Text");
        dialog.register_name(9, "aether-images", "Images");
        dialog
    }

    #[test]
    fn a_hang_raises_the_dialog_and_a_late_pong_closes_it() {
        let now = Instant::now();
        let mut dialog = dialog();

        let event = dialog.observe(
            HangEvent::NotResponding {
                client: 7,
                after_ms: 5100,
            },
            now,
        );
        assert_eq!(event, DialogEvent::Shown(7));
        assert!(dialog.is_open());
        let app = dialog.current().expect("dialog app");
        assert_eq!(app.name, "Aether Text");
        assert_eq!(app.id, "aether-text");
        assert_eq!(app.after_ms, 5100);
        assert_eq!(dialog.prompts, 1);

        // A repeated marker while the dialog is up does not stack prompts.
        assert_eq!(
            dialog.observe(
                HangEvent::NotResponding {
                    client: 7,
                    after_ms: 5100
                },
                now + Duration::from_millis(10)
            ),
            DialogEvent::Queued(7)
        );
        assert_eq!(dialog.prompts, 1);

        assert_eq!(
            dialog.observe(
                HangEvent::Recovered { client: 7 },
                now + Duration::from_millis(200)
            ),
            DialogEvent::Recovered(7)
        );
        assert!(!dialog.is_open());
        assert!(dialog.pending_clients().is_empty());
        assert_eq!(dialog.recoveries, 1);
        assert!(dialog.summary_line().contains("recoveries=1"));
    }

    #[test]
    fn wait_waives_the_dialog_and_reoffers_after_the_grace() {
        let now = Instant::now();
        let mut dialog = dialog();
        dialog.observe(
            HangEvent::NotResponding {
                client: 7,
                after_ms: 5000,
            },
            now,
        );

        let acted = dialog.act(DialogAction::Wait, now);
        assert_eq!(acted, Some((DialogAction::Wait, 7)));
        assert!(!dialog.is_open());
        assert_eq!(dialog.waits, 1);
        let line = dialog.action_line(7, DialogAction::Wait);
        assert!(line.contains("action=wait grace_ms=15000"), "{line}");

        // Inside the grace nothing is offered.
        assert_eq!(dialog.tick(now + Duration::from_secs(14)), None);
        assert!(!dialog.is_open());
        // The app is still hung: the dialog comes back on its own.
        assert_eq!(dialog.tick(now + Duration::from_secs(15)), Some(7));
        assert_eq!(dialog.prompts, 2);
        let app = dialog.current().unwrap();
        assert_eq!(app.after_ms, 5000, "the detection delay is preserved");
    }

    #[test]
    fn force_quit_returns_the_client_and_shows_the_next_hang() {
        let now = Instant::now();
        let mut dialog = dialog();
        dialog.observe(
            HangEvent::NotResponding {
                client: 7,
                after_ms: 5200,
            },
            now,
        );
        assert_eq!(
            dialog.observe(
                HangEvent::NotResponding {
                    client: 9,
                    after_ms: 5300
                },
                now
            ),
            DialogEvent::Queued(9)
        );
        assert_eq!(dialog.current().unwrap().client, 7);

        let acted = dialog.act(DialogAction::ForceQuit, now);
        assert_eq!(acted, Some((DialogAction::ForceQuit, 7)));
        assert_eq!(dialog.force_quits, 1);
        assert_eq!(
            dialog.current().unwrap().client,
            9,
            "the next hang is shown"
        );
        assert_eq!(dialog.pending_clients(), vec![9]);

        // Force quit removed the app; a stray recovery for it is ignored.
        assert_eq!(
            dialog.observe(HangEvent::Recovered { client: 7 }, now),
            DialogEvent::None
        );
        assert!(dialog
            .action_line(9, DialogAction::ForceQuit)
            .contains("action=force_quit"));
    }

    #[test]
    fn an_unknown_client_still_gets_a_readable_label() {
        let now = Instant::now();
        let mut dialog = NoHangDialog::with_default_grace();
        dialog.observe(
            HangEvent::NotResponding {
                client: 42,
                after_ms: 6100,
            },
            now,
        );
        let app = dialog.current().unwrap();
        assert_eq!(app.id, "client-42");
        assert_eq!(app.name, "Application 42");
        assert!(dialog.dialog_line(42).contains("app=client-42"));
    }

    #[test]
    fn only_decisions_are_parsed_out_of_the_journal() {
        let log = "\
AETHER_COMPOSITOR_PING: client=7 serial=3
AETHER_COMPOSITOR_PONG: client=7 serial=3 rtt_ms=12 recovered=false
AETHER_COMPOSITOR_APP_STATE: client=7 state=ping_pending after_ms=0
AETHER_COMPOSITOR_APP_STATE: client=7 state=responsive recovered=false
AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100
AETHER_COMPOSITOR_APP_STATE: client=7 state=responsive recovered=true
AETHER_COMPOSITOR_APP_STATE: bad line without a state
";
        assert_eq!(
            health_events(log),
            vec![
                HangEvent::NotResponding {
                    client: 7,
                    after_ms: 5100
                },
                HangEvent::Recovered { client: 7 },
            ]
        );
    }

    #[test]
    fn the_watchdog_report_names_a_stall_and_a_recovery() {
        let stalled = WatchdogReport::scan(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6200 timeout_ms=5000 timeouts=1\n",
        );
        assert_eq!(stalled.timeouts, 1);
        assert!(stalled.needs_restart());
        assert!(!stalled.escalates());
        assert_eq!(stalled.last_idle_ms, Some(6200));
        assert_eq!(stalled.timeout_ms, Some(5000));

        let recovered = WatchdogReport::scan(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6200 timeout_ms=5000 timeouts=1\n\
             AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true timeouts=1\n",
        );
        assert_eq!(recovered.timeouts, 1);
        assert!(!recovered.needs_restart());
        assert!(recovered.recovered);

        let none = WatchdogReport::scan("nothing\n");
        assert!(!none.needs_restart());
        assert_eq!(none.last_state, None);
    }

    #[test]
    fn three_unconfirmed_starts_enter_safe_mode_and_a_confirmation_clears_the_count() {
        let path = std::env::temp_dir().join(format!(
            "aether-nohang-ledger-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = StartLedger::load(&path);
        assert_eq!(
            ledger,
            StartLedger::default(),
            "missing file is a clean start"
        );

        let first = ledger.begin_start();
        assert_eq!(
            first,
            StartOutcome {
                attempt: 1,
                safe_mode: false,
                previous_failed: false
            }
        );
        assert!(!ledger.confirmed);

        // The shell died before confirming: the next start is attempt 2.
        let second = ledger.begin_start();
        assert_eq!(second.attempt, 2);
        assert!(second.previous_failed);
        let third = ledger.begin_start();
        assert_eq!(third.attempt, 3);
        assert!(third.safe_mode, "three failed starts enter safe mode");
        assert!(ledger.safe_mode_line().contains("effects=minimal"));

        // Persist and read back: a new process sees the same state.
        ledger.save(&path).unwrap();
        let reloaded = StartLedger::load(&path);
        assert_eq!(reloaded, ledger);
        assert!(reloaded.safe_mode);

        // A good start clears the attempt count but keeps safe mode on.
        let mut confirmed = reloaded.clone();
        confirmed.confirm_start();
        assert_eq!(confirmed.attempts, 0);
        assert!(confirmed.safe_mode, "safe mode needs an explicit reset");
        assert_eq!(
            confirmed.begin_start(),
            StartOutcome {
                attempt: 1,
                safe_mode: true,
                previous_failed: false
            }
        );

        // The banner's reset leaves safe mode.
        let mut reset = reloaded;
        reset.reset();
        assert!(!reset.safe_mode);
        assert_eq!(reset.attempts, 0);
        reset.save(&path).unwrap();
        assert_eq!(StartLedger::load(&path), StartLedger::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn three_watchdog_timeouts_arm_safe_mode() {
        let mut ledger = StartLedger::default();
        let one = WatchdogReport::scan(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6000 timeout_ms=5000 timeouts=1\n",
        );
        assert!(!ledger.observe_watchdog(&one));
        assert!(!ledger.safe_mode);
        assert_eq!(ledger.watchdog_timeouts, 1);

        let three = WatchdogReport::scan(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6000 timeout_ms=5000 timeouts=3\n",
        );
        assert!(ledger.observe_watchdog(&three), "this armed safe mode");
        assert!(ledger.safe_mode);
        assert!(ledger.safe_mode_reason().contains("stalled"));
        assert!(
            !ledger.observe_watchdog(&three),
            "armed once, not every check"
        );
    }

    #[test]
    fn safe_mode_resolves_the_minimal_tier() {
        assert_eq!(resolve_tier(true, Tier::Full), Tier::Minimal);
        assert_eq!(resolve_tier(true, Tier::Reduced), Tier::Minimal);
        assert_eq!(resolve_tier(false, Tier::Full), Tier::Full);
    }

    #[test]
    fn the_check_accepts_a_hang_inside_the_limit_and_a_healthy_shell() {
        let log = "\
AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100
AETHER_COMPOSITOR_APP_STATE: client=7 state=responsive recovered=true
AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6100 timeout_ms=5000 timeouts=1
AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true timeouts=1
";
        let report = NoHangReport::scan(log);
        assert!(report.problems().is_empty(), "{:?}", report.problems());
        assert_eq!(report.hangs(), vec![(7, 5100)]);
        assert_eq!(report.recovered_clients(), vec![7]);
        let summary = report.summary(&StartLedger::default());
        assert!(summary.contains("NOHANG_HANG client=7 after_ms=5100 within_limit=true"));
        assert!(summary.contains("restart_required=false"));
        assert!(summary.ends_with("AETHER_SHELL_NOHANG_OK\n"), "{summary}");
    }

    #[test]
    fn the_check_rejects_a_late_detection_and_a_stalled_shell() {
        let late = NoHangReport::scan(
            "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=8000\n",
        );
        assert!(late
            .problems()
            .iter()
            .any(|problem| problem.contains("after 8000 ms; G-NH2 allows 6000 ms")));

        let stalled = NoHangReport::scan(
            "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100\n\
             AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=9000 timeout_ms=5000 timeouts=2\n",
        );
        let problems = stalled.problems();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("must restart it")),
            "{problems:?}"
        );
        assert!(stalled
            .summary(&StartLedger::default())
            .ends_with("AETHER_SHELL_NOHANG_FAILED\n"));

        let empty = NoHangReport::scan("hello\n");
        assert!(empty
            .problems()
            .iter()
            .any(|problem| problem.contains("no AETHER_COMPOSITOR_APP_STATE marker")));
    }

    #[test]
    fn the_check_prints_safe_mode_when_the_ledger_is_in_it() {
        let ledger = StartLedger {
            attempts: 3,
            confirmed: false,
            safe_mode: true,
            watchdog_timeouts: 0,
        };
        let report = NoHangReport::scan(
            "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5000\n",
        );
        let summary = report.summary(&ledger);
        assert!(summary.contains("NOHANG_START attempts=3 confirmed=false safe_mode=true"));
        assert!(
            summary.contains("AETHER_SHELL_SAFE_MODE: attempts=3"),
            "{summary}"
        );
        assert!(summary.ends_with("AETHER_SHELL_NOHANG_OK\n"), "{summary}");
    }
}
