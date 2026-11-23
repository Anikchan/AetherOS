//! `aether-lifecycled` — lifecycle daemon entry point.
//!
//! Usage:
//!   aether-lifecycled [--once] [--simulate] [--config PATH] [--dry-run]
//!
//! `--simulate` runs the full event flow (launch, hide, suspend, hibernate,
//! pressure kill) with in-memory mechanisms. It is the reproducible smoke test
//! for hosts that are not AetherOS (macOS dev machines, CI).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aether_lifecycled::cgroup::{CgroupFs, MemoryReader};
use aether_lifecycled::daemon::{system_daemon, Daemon, Mechanisms, RuntimeConfig};
use aether_lifecycled::identity::{AppId, MemoryScopeManager};
use aether_lifecycled::policy::{Event, PolicyConfig};
use aether_lifecycled::pressure::PressureLevel;
use aether_lifecycled::protocol::SocketNotifier;

fn usage() -> ! {
    eprintln!(
        "usage: aether-lifecycled [--once] [--simulate] [--dry-run] [--config PATH]"
    );
    std::process::exit(2);
}

fn main() {
    let mut once = false;
    let mut simulate = false;
    let mut dry_run = false;
    let mut config_path: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--once" => once = true,
            "--simulate" => simulate = true,
            "--dry-run" => dry_run = true,
            "--config" => {
                config_path = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument: {other}");
                usage();
            }
        }
    }

    let mut config = RuntimeConfig::load();
    if let Some(path) = config_path {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                if let Ok(parsed) = toml::from_str::<toml::Value>(&text) {
                    if let Some(root) = parsed.get("cgroup_root").and_then(|v| v.as_str()) {
                        config.cgroup_root = PathBuf::from(root);
                    }
                    if let Some(state) = parsed.get("state_dir").and_then(|v| v.as_str()) {
                        config.state_dir = PathBuf::from(state);
                    }
                    if let Some(sockets) = parsed.get("socket_dir").and_then(|v| v.as_str()) {
                        config.socket_dir = PathBuf::from(sockets);
                    }
                    if let Some(dry) = parsed.get("dry_run").and_then(|v| v.as_bool()) {
                        config.dry_run = dry;
                    }
                }
            }
            Err(e) => eprintln!("config {path:?}: {e}"),
        }
    }
    config.dry_run |= dry_run;

    if simulate {
        run_simulation();
        return;
    }

    let mut daemon = build_daemon(config);
    if let Err(e) = daemon.restore(Instant::now()) {
        eprintln!("restore failed: {e}");
    }

    if once {
        let metrics = daemon.metrics();
        println!(
            "{}",
            serde_json::to_string_pretty(&metrics).unwrap_or_else(|_| "{}".into())
        );
        return;
    }

    run_loop(&mut daemon);
}

/// Build a daemon honoring the `dry_run` flag.
fn build_daemon(config: RuntimeConfig) -> Daemon {
    if !config.dry_run {
        return system_daemon(config);
    }
    let cgroup = CgroupFs::dry_run(config.cgroup_root.clone());
    Daemon::new(
        config.clone(),
        Mechanisms {
            scopes: Box::new(MemoryScopeManager::default()),
            freezer: Box::new(cgroup.clone()),
            reclaimer: Box::new(cgroup.clone()),
            killer: Box::new(cgroup.clone()),
            memory: Box::new(cgroup),
            notifier: Box::new(SocketNotifier::at(config.socket_dir.clone())),
        },
    )
}

/// Event loop. The compositor/portal event source (S2/S3) feeds
/// `daemon.handle`; until that socket exists, the loop only advances timers and
/// refreshes the status file.
fn run_loop(daemon: &mut Daemon) -> ! {
    let mut last_pressure = PressureLevel::Normal;
    let psi = aether_lifecycled::pressure::PsiReader::system();
    loop {
        let now = Instant::now();
        daemon.poll_timers(now);

        let level = psi.level();
        if level != last_pressure {
            daemon.handle(Event::PressureChanged(level), now);
            last_pressure = level;
        }

        write_status(daemon);

        let sleep = daemon
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(Duration::from_secs(5))
            .min(Duration::from_secs(5))
            .max(Duration::from_millis(100));
        std::thread::sleep(sleep);
    }
}

fn write_status(daemon: &mut Daemon) {
    let path = daemon.config().state_dir.join("status.json");
    if std::fs::create_dir_all(path.parent().unwrap()).is_err() {
        return;
    }
    let metrics = daemon.metrics();
    if let Ok(text) = serde_json::to_string_pretty(&metrics) {
        let _ = std::fs::write(path, text);
    }
}

/// Deterministic demo of the whole flow (P5-T01..T06 acceptance smoke).
fn run_simulation() {
    // Short path: the lifecycle SDK socket lives in this directory and
    // AF_UNIX paths are limited to 104 bytes.
    let dir = PathBuf::from("/tmp").join(format!(
        "aet-sim-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let cgroup = CgroupFs::dry_run(dir.join("cgroup"));
    let scopes = MemoryScopeManager::default();
    let memory = std::sync::Arc::new(std::sync::Mutex::new(0u64));

    struct SimMemory(std::sync::Arc<std::sync::Mutex<u64>>);
    impl MemoryReader for SimMemory {
        fn memory_current(&self, _scope: &std::path::Path) -> std::io::Result<u64> {
            Ok(*self.0.lock().unwrap())
        }
    }

    let config = RuntimeConfig {
        cgroup_root: dir.join("cgroup"),
        socket_dir: dir.join("sockets"),
        state_dir: dir.join("state"),
        dry_run: true,
        policy: PolicyConfig::default(),
    };
    let mut daemon = Daemon::new(
        config,
        Mechanisms {
            scopes: Box::new(scopes),
            freezer: Box::new(cgroup.clone()),
            reclaimer: Box::new(cgroup.clone()),
            killer: Box::new(cgroup.clone()),
            memory: Box::new(SimMemory(memory.clone())),
            notifier: Box::new(SocketNotifier::at(dir.join("sockets"))),
        },
    );

    let now = Instant::now();
    let fg = AppId::new("org.example.Fg").unwrap();
    let mail = AppId::new("org.example.Mail").unwrap();
    let music = AppId::new("org.example.Music").unwrap();
    for (id, mem) in [(&fg, 120u64), (&mail, 30u64), (&music, 20u64)] {
        daemon.register(id.clone(), Some(100), now).unwrap();
        *memory.lock().unwrap() = mem * 1024 * 1024;
    }
    *memory.lock().unwrap() = 30 * 1024 * 1024;

    daemon.handle(Event::FocusChanged { to: Some(fg.clone()) }, now);
    daemon.handle(Event::WindowShown(mail.clone()), now);
    daemon.handle(Event::WindowShown(music.clone()), now);
    daemon.handle(
        Event::ProtectionChanged {
            id: music.clone(),
            reason: aether_lifecycled::state::ProtectReason::Audio,
            active: true,
        },
        now,
    );
    daemon.handle(Event::WindowHidden(mail.clone()), now);
    daemon.handle(Event::WindowHidden(music.clone()), now);

    let after_grace = now + Duration::from_secs(31);
    daemon.poll_timers(after_grace);
    println!("after grace: mail={:?} music={:?}", daemon.state_of(&mail), daemon.state_of(&music));

    let after_hibernate = after_grace + Duration::from_secs(11 * 60);
    daemon.poll_timers(after_hibernate);
    println!(
        "after hibernate: mail={:?} music={:?}",
        daemon.state_of(&mail),
        daemon.state_of(&music)
    );

    daemon.handle(
        Event::PressureChanged(PressureLevel::Critical),
        after_hibernate + Duration::from_secs(1),
    );
    println!(
        "after critical pressure: fg={:?} mail={:?} music={:?}",
        daemon.state_of(&fg),
        daemon.state_of(&mail),
        daemon.state_of(&music)
    );

    let metrics = daemon.metrics();
    println!(
        "metrics: total={} inactive={} share={:.2} kills={}",
        metrics.total_memory,
        metrics.inactive_memory,
        metrics.inactive_share,
        metrics.kills.len()
    );
    for entry in daemon.log().iter().rev().take(8).rev() {
        println!("{}", entry.line());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
