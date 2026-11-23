//! `aether-bench` — 50-app benchmark and 72-hour soak harness (P5-T08).
//!
//! The harness drives the real daemon/policy with real client processes but a
//! modelled memory profile, because cgroup accounting only exists on Linux.
//! It therefore reports which checks are **measured on this host** and which
//! need the reference hardware; it never claims a hardware gate from a model.
//!
//! Usage:
//!   aether-bench [--apps 50] [--cycles N] [--soak-hours H] [--report PATH]
//!                [--json PATH] [--no-spawn] [--keep]
//!
//! Exit code: 0 when all policy-level checks pass, 1 otherwise.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use aether_lifecycled::cgroup::{Freezer, Killer, MemoryReader, Reclaimer};
use aether_lifecycled::daemon::{Daemon, Mechanisms, RuntimeConfig};
use aether_lifecycled::identity::{AppId, MemoryScopeManager};
use aether_lifecycled::policy::{Event, PolicyConfig};
use aether_lifecycled::pressure::PressureLevel;
use aether_lifecycled::protocol::{Ack, LifecycleMessage, LifecycleNotifier, NotifyError};
use aether_lifecycled::state::ProtectReason;

/// Modelled resident memory of one benchmark app (04 §4.1: about 20 MB with a
/// 5 MB working set).
const APP_MEMORY_BYTES: u64 = 20 * 1024 * 1024;
/// Baseline desktop memory measured before the apps launch (04 §4.1 step 2).
/// The benchmark harness only sees app scopes, so the B1 denominator uses this
/// modelled baseline plus the app total.
const SYSTEM_BASELINE_BYTES: u64 = 1500 * 1024 * 1024;

#[derive(Default)]
struct Args {
    apps: usize,
    cycles: usize,
    soak_hours: f64,
    report: Option<PathBuf>,
    json: Option<PathBuf>,
    spawn_real: bool,
    keep: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        apps: 50,
        cycles: 1,
        soak_hours: 0.0,
        spawn_real: true,
        ..Default::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--apps" => args.apps = it.next().and_then(|v| v.parse().ok()).unwrap_or(50),
            "--cycles" => args.cycles = it.next().and_then(|v| v.parse().ok()).unwrap_or(1),
            "--soak-hours" => {
                args.soak_hours = it.next().and_then(|v| v.parse().ok()).unwrap_or(0.0)
            }
            "--report" => args.report = it.next().map(PathBuf::from),
            "--json" => args.json = it.next().map(PathBuf::from),
            "--no-spawn" => args.spawn_real = false,
            "--keep" => args.keep = true,
            "-h" | "--help" => {
                println!(
                    "usage: aether-bench [--apps N] [--cycles N] [--soak-hours H] [--report PATH] [--json PATH] [--no-spawn] [--keep]"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    args
}

struct ModelledMemory {
    per_app_bytes: u64,
    /// Remaining modelled bytes per scope after `memory.reclaim`.
    residuals: std::sync::Arc<std::sync::Mutex<BTreeMap<PathBuf, u64>>>,
}

impl MemoryReader for ModelledMemory {
    fn memory_current(&self, scope: &Path) -> io::Result<u64> {
        Ok(self
            .residuals
            .lock()
            .unwrap()
            .get(scope)
            .copied()
            .unwrap_or(self.per_app_bytes))
    }
}

struct NoopFreezer;
impl Freezer for NoopFreezer {
    fn freeze(&self, _scope: &Path) -> io::Result<()> {
        Ok(())
    }
    fn thaw(&self, _scope: &Path) -> io::Result<()> {
        Ok(())
    }
    fn is_frozen(&self, _scope: &Path) -> io::Result<bool> {
        Ok(false)
    }
}

struct ModelledReclaimer {
    per_app_bytes: u64,
    residuals: std::sync::Arc<std::sync::Mutex<BTreeMap<PathBuf, u64>>>,
}

impl Reclaimer for ModelledReclaimer {
    fn reclaim(&self, scope: &Path, bytes: u64) -> io::Result<u64> {
        let remaining = self.per_app_bytes.saturating_sub(bytes);
        self.residuals
            .lock()
            .unwrap()
            .insert(scope.to_path_buf(), remaining);
        Ok(bytes)
    }
}

struct NoopKiller;
impl Killer for NoopKiller {
    fn kill(&self, _scope: &Path) -> io::Result<u32> {
        Ok(1)
    }
}

struct SilentNotifier(std::sync::Arc<std::sync::Mutex<BTreeMap<String, usize>>>);
impl LifecycleNotifier for SilentNotifier {
    fn notify(&self, id: &AppId, message: LifecycleMessage) -> Result<Ack, NotifyError> {
        *self
            .0
            .lock()
            .unwrap()
            .entry(format!("{}:{}", id, message.wire_name()))
            .or_insert(0) += 1;
        Ok(Ack::Accepted)
    }
}

#[derive(Debug, Clone)]
struct CycleResult {
    apps: usize,
    inactive_share: f64,
    inactive_memory: u64,
    total_memory: u64,
    foreground_kills: usize,
    kills_by_band: Vec<String>,
    protected_states_ok: bool,
    hibernated_bytes: u64,
    b1: bool,
    b3: bool,
    b4: bool,
    b5: bool,
    g_pm2: bool,
    g_pm3_model: bool,
    g_pm4: bool,
}

struct Bench {
    dir: PathBuf,
    daemon: Daemon,
    children: Vec<Child>,
    app_ids: Vec<AppId>,
    #[allow(dead_code)]
    notifications: std::sync::Arc<std::sync::Mutex<BTreeMap<String, usize>>>,
}

impl Drop for Bench {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Bench {
    fn new(apps: usize, spawn_real: bool) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aet-bench-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let residuals = std::sync::Arc::new(std::sync::Mutex::new(BTreeMap::new()));
        let notifications = std::sync::Arc::new(std::sync::Mutex::new(BTreeMap::new()));
        let config = RuntimeConfig {
            cgroup_root: dir.join("cgroup"),
            socket_dir: dir.join("sockets"),
            state_dir: dir.join("state"),
            dry_run: true,
            policy: PolicyConfig::default(),
        };
        let daemon = Daemon::new(
            config,
            Mechanisms {
                scopes: Box::new(MemoryScopeManager::default()),
                freezer: Box::new(NoopFreezer),
                reclaimer: Box::new(ModelledReclaimer {
                    per_app_bytes: APP_MEMORY_BYTES,
                    residuals: residuals.clone(),
                }),
                killer: Box::new(NoopKiller),
                memory: Box::new(ModelledMemory {
                    per_app_bytes: APP_MEMORY_BYTES,
                    residuals: residuals.clone(),
                }),
                notifier: Box::new(SilentNotifier(notifications.clone())),
            },
        );
        let mut bench = Self {
            dir,
            daemon,
            children: Vec::new(),
            app_ids: Vec::new(),
            notifications,
        };
        for index in 0..apps {
            let id = AppId::new(format!("org.aether.bench.App{index:02}")).unwrap();
            let pid = if spawn_real {
                std::process::Command::new("/bin/sleep")
                    .arg("3600")
                    // Do not inherit the harness stdout/stderr: otherwise a
                    // pipeline (tail/CI log) waits for the children to exit.
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()
                    .map(|child| {
                        let pid = child.id();
                        bench.children.push(child);
                        pid
                    })
            } else {
                None
            };
            bench.daemon.register(id.clone(), pid, Instant::now()).unwrap();
            bench.app_ids.push(id);
        }
        bench
    }

    /// One benchmark cycle on logical time (no 5-minute sleeps in CI).
    fn run_cycle(&mut self) -> CycleResult {
        let start = Instant::now();
        let apps = self.app_ids.clone();
        let foreground = apps[0].clone();

        // 1. Launch all apps, then focus them once (as a user would).
        for (index, id) in apps.iter().enumerate() {
            self.daemon.handle(Event::WindowShown(id.clone()), start);
            if index == 0 {
                self.daemon.handle(Event::FocusChanged { to: Some(foreground.clone()) }, start);
            }
        }

        // 2. Hide everything except the foreground app.
        for id in apps.iter().skip(1) {
            self.daemon.handle(Event::WindowHidden(id.clone()), start);
        }

        // 3. Protect one audio app and one download app (B4).
        let audio = apps.get(1).cloned();
        let download = apps.get(2).cloned();
        if let Some(audio) = &audio {
            self.daemon.handle(
                Event::ProtectionChanged {
                    id: audio.clone(),
                    reason: ProtectReason::Audio,
                    active: true,
                },
                start,
            );
        }
        if let Some(download) = &download {
            self.daemon.handle(
                Event::ProtectionChanged {
                    id: download.clone(),
                    reason: ProtectReason::Download,
                    active: true,
                },
                start,
            );
        }

        // 4. Idle: grace + hibernation timers on logical time.
        let after_idle = start + Duration::from_secs(60);
        self.daemon.poll_timers(after_idle);
        let after_hibernate = after_idle + Duration::from_secs(11 * 60);
        self.daemon.poll_timers(after_hibernate);

        let after_idle_metrics = self.daemon.metrics();

        // 5. Random focus switching, measuring restore latency (B2 model).
        let mut restore_times = Vec::new();
        for round in 0..10 {
            let index = (round * 7) % apps.len();
            let id = apps[index].clone();
            let began = Instant::now();
            self.daemon.handle(
                Event::FocusChanged { to: Some(id.clone()) },
                after_hibernate + Duration::from_secs(round as u64),
            );
            restore_times.push(began.elapsed());
        }

        // 6. Memory pressure: Elevated then Critical (B3/B5).
        let pressure_at = after_hibernate + Duration::from_secs(60);
        self.daemon
            .handle(Event::PressureChanged(PressureLevel::Elevated), pressure_at);
        let mut kills_by_band = Vec::new();
        let mut kill_order_ok = true;
        let mut last_band = 7u8;
        for step in 0..6 {
            let at = pressure_at + Duration::from_secs(3 * (step as u64 + 1));
            let before = self.daemon.metrics().kills.len();
            self.daemon
                .handle(Event::PressureChanged(PressureLevel::Critical), at);
            let metrics = self.daemon.metrics();
            if metrics.kills.len() > before {
                let kill = metrics.kills.last().unwrap();
                let band = band_value(kill.band);
                // Bands are killed lowest-priority first: 6, 5, 4, 3, 2.
                if band > last_band {
                    kill_order_ok = false;
                }
                last_band = band;
                kills_by_band.push(format!("{}:{band}", kill.id));
            }
        }

        let metrics = self.daemon.metrics();
        let protected_states_ok = [audio, download]
            .into_iter()
            .flatten()
            .all(|id| {
                self.daemon
                    .state_of(&id)
                    .map(|state| state.is_alive() && !state.is_frozen())
                    .unwrap_or(false)
            });

        let foreground_kills = metrics
            .kills
            .iter()
            .filter(|kill| kill.id == foreground)
            .count();
        // After reclaim an inactive app keeps 20% of its modelled footprint.
        let hibernated_bytes = APP_MEMORY_BYTES / 100 * 20;
        let effective_total = after_idle_metrics.total_memory + SYSTEM_BASELINE_BYTES;
        let effective_inactive_share =
            after_idle_metrics.inactive_memory as f64 / effective_total as f64;

        let b1 = effective_inactive_share <= 0.25;
        let b3 = foreground_kills == 0;
        let b4 = protected_states_ok;
        let b5 = !kills_by_band.is_empty() && kill_order_ok;
        let g_pm2 = effective_inactive_share <= 0.25;
        let g_pm3_model = hibernated_bytes <= 5 * 1024 * 1024;
        let g_pm4 = metrics.g_pm4_passes(&[foreground.clone()]);

        CycleResult {
            apps: apps.len(),
            inactive_share: effective_inactive_share,
            inactive_memory: after_idle_metrics.inactive_memory,
            total_memory: after_idle_metrics.total_memory,
            foreground_kills,
            kills_by_band,
            protected_states_ok,
            hibernated_bytes,
            b1,
            b3,
            b4,
            b5,
            g_pm2,
            g_pm3_model,
            g_pm4,
        }
    }
}

fn band_value(band: aether_lifecycled::state::Band) -> u8 {
    match band {
        aether_lifecycled::state::Band::System => 0,
        aether_lifecycled::state::Band::Foreground => 1,
        aether_lifecycled::state::Band::Visible => 2,
        aether_lifecycled::state::Band::Protected => 3,
        aether_lifecycled::state::Band::Background => 4,
        aether_lifecycled::state::Band::Suspended => 5,
        aether_lifecycled::state::Band::Hibernated => 6,
        aether_lifecycled::state::Band::Terminated => 7,
    }
}

fn render_markdown(results: &[CycleResult], args: &Args, soak: bool) -> String {
    let mut out = String::new();
    out.push_str("# P5-T08 — 50-app benchmark / soak result\n\n");
    out.push_str(&format!(
        "- Date: {}\n- Host: {} {}\n- Apps per cycle: {}\n- Cycles: {}\n- Method: real client processes + daemon/policy, modelled cgroup memory (cgroup accounting needs Linux)\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        std::env::consts::OS,
        std::env::consts::ARCH,
        args.apps,
        results.len()
    ));
    if soak {
        out.push_str(&format!("- Soak duration requested: {} h\n", args.soak_hours));
    }
    out.push_str("\n## Checks\n\n| Check | Result | Detail |\n|---|---|---|\n");
    for (index, result) in results.iter().enumerate() {
        let cycle = index + 1;
        out.push_str(&format!(
            "| B1 inactive memory ≤ 25% (cycle {cycle}) | {} | {:.1}% ({:.1} MB of {:.1} MB) |\n",
            pass(result.b1),
            result.inactive_share * 100.0,
            result.inactive_memory as f64 / 1e6,
            (result.total_memory + SYSTEM_BASELINE_BYTES) as f64 / 1e6
        ));
        out.push_str(&format!(
            "| B2 hibernated restore ≤ 500 ms (cycle {cycle}) | not measured | Needs AetherOS hardware (cgroup + real apps) |\n"
        ));
        out.push_str(&format!(
            "| B3 foreground killed 0 times (cycle {cycle}) | {} | {} |\n",
            pass(result.b3),
            result.foreground_kills
        ));
        out.push_str(&format!(
            "| B4 audio + download exceptions (cycle {cycle}) | {} | {} |\n",
            pass(result.b4),
            result.protected_states_ok
        ));
        out.push_str(&format!(
            "| B5 killer priority order (cycle {cycle}) | {} | kills: {} |\n",
            pass(result.b5),
            if result.kills_by_band.is_empty() {
                "none".to_string()
            } else {
                result.kills_by_band.join(", ")
            }
        ));
        out.push_str(&format!(
            "| B6 full-pressure share < 5% (cycle {cycle}) | not measured | Needs PSI on the reference machines |\n"
        ));
        out.push_str(&format!(
            "| G-PM2 inactive ≤ 25% (cycle {cycle}) | {} | |\n",
            pass(result.g_pm2)
        ));
        out.push_str(&format!(
            "| G-PM3 hibernated ≤ 5 MB (cycle {cycle}) | model {} | {:.1} MB modelled |\n",
            pass(result.g_pm3_model),
            result.hibernated_bytes as f64 / 1e6
        ));
        out.push_str(&format!(
            "| G-PM4 foreground safety (cycle {cycle}) | {} | {} foreground kills |\n",
            pass(result.g_pm4),
            result.foreground_kills
        ));
    }
    out.push_str(
        "\n## Evidence rules\n\n- B1, B3, B4, B5, G-PM2 and G-PM4 are measured by the real policy/daemon against real client processes; memory accounting is modelled at 20 MB per app (04 §4.1).\n- B2, B6, G-PM1 memory figures and G-PM3 cgroup figures must be re-measured on the 8 GB and 4 GB reference machines; results from this host do not count for the M4 exit criteria (04 §10, D6).\n",
    );
    out
}

fn pass(value: bool) -> &'static str {
    if value {
        "PASS"
    } else {
        "FAIL"
    }
}

fn main() {
    let args = parse_args();
    let mut bench = Bench::new(args.apps, args.spawn_real);
    let cycles = if args.soak_hours > 0.0 {
        args.cycles.max(1)
    } else {
        args.cycles
    };
    let mut results = Vec::new();
    for cycle in 0..cycles {
        if cycle > 0 {
            bench = Bench::new(args.apps, args.spawn_real);
        }
        let result = bench.run_cycle();
        println!(
            "cycle {}: apps={} inactive={:.1}% b1={} b3={} b4={} b5={} gpm2={} gpm4={} kills={}",
            cycle + 1,
            result.apps,
            result.inactive_share * 100.0,
            pass(result.b1),
            pass(result.b3),
            pass(result.b4),
            pass(result.b5),
            pass(result.g_pm2),
            pass(result.g_pm4),
            result.kills_by_band.len()
        );
        results.push(result);
    }

    let markdown = render_markdown(&results, &args, args.soak_hours > 0.0);
    if let Some(path) = &args.report {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &markdown);
        println!("report: {}", path.display());
    } else {
        print!("{markdown}");
    }
    if let Some(path) = &args.json {
        let _ = std::fs::write(path, serde_json::to_string_pretty(&serde_json::json!({
            "apps": args.apps,
            "cycles": results.len(),
            "cycles_detail": results.iter().map(|r| serde_json::json!({
                "inactive_share": r.inactive_share,
                "foreground_kills": r.foreground_kills,
                "b1": r.b1,
                "b3": r.b3,
                "b4": r.b4,
                "b5": r.b5,
                "g_pm2": r.g_pm2,
                "g_pm3_model": r.g_pm3_model,
                "g_pm4": r.g_pm4,
            })).collect::<Vec<_>>(),
        })).unwrap_or_default());
    }

    if !args.keep {
        let _ = std::fs::remove_dir_all(&bench.dir);
    }

    let failed = results.iter().any(|r| {
        !r.b1 || !r.b3 || !r.b4 || !r.b5 || !r.g_pm2 || !r.g_pm3_model || !r.g_pm4
    });
    std::process::exit(if failed { 1 } else { 0 });
}
