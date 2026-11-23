//! `aether-powerd` — power policy daemon entry point.
//!
//! Usage: aether-powerd [--once] [--simulate] [--dry-run] [--config PATH]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aether_powerd::daemon::{epoch_secs, system_daemon, write_status_file};
use aether_powerd::types::{BatteryStatus, LowBatteryAction, PowerSource};
use aether_powerd::PowerdConfig;

fn usage() -> ! {
    eprintln!("usage: aether-powerd [--once] [--simulate] [--dry-run] [--config PATH]");
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
            "--config" => config_path = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "-h" | "--help" => usage(),
            other => {
                eprintln!("unknown argument: {other}");
                usage();
            }
        }
    }

    let mut config = PowerdConfig::default();
    config.dry_run |= dry_run;
    if let Some(path) = config_path {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = toml::from_str::<toml::Value>(&text) {
                if let Some(dir) = parsed.get("state_dir").and_then(|v| v.as_str()) {
                    config.state_dir = PathBuf::from(dir);
                }
                if let Some(dry) = parsed.get("dry_run").and_then(|v| v.as_bool()) {
                    config.dry_run = dry;
                }
                if let Some(limit) = parsed.get("charge_limit").and_then(|v| v.as_integer()) {
                    config.charge_limit = Some(limit.clamp(0, 100) as u8);
                }
                if let Some(action) = parsed.get("low_battery_action").and_then(|v| v.as_str()) {
                    config.policy.low_action = match action {
                        "hibernate" => LowBatteryAction::Hibernate,
                        _ => LowBatteryAction::Suspend,
                    };
                }
                if let Some(swap) = parsed.get("swap_gib").and_then(|v| v.as_integer()) {
                    config.policy.swap_available = swap > 0;
                }
                if let Some(ram) = parsed.get("ram_gib").and_then(|v| v.as_integer()) {
                    let _ = ram;
                }
                if let Some(sthz) = parsed
                    .get("suspend_then_hibernate")
                    .and_then(|v| v.as_bool())
                {
                    config.policy.suspend_then_hibernate = sthz;
                }
            }
        }
    }

    if simulate {
        run_simulation();
        return;
    }

    let mut daemon = system_daemon(config);
    if once {
        let epoch = epoch_secs();
        daemon.tick(Instant::now(), epoch, true);
        println!(
            "{}",
            serde_json::to_string_pretty(&daemon.status()).unwrap_or_else(|_| "{}".into())
        );
        let _ = write_status_file(&daemon);
        return;
    }

    let interval = Duration::from_secs(daemon.config().sample_interval_secs.max(1));
    loop {
        let epoch = epoch_secs();
        daemon.tick(Instant::now(), epoch, true);
        let _ = write_status_file(&daemon);
        std::thread::sleep(interval);
    }
}

/// Deterministic rule walk-through for CI and dev hosts without a battery.
fn run_simulation() {
    use aether_powerd::policy::PowerPolicyImpl;
    use aether_powerd::types::{PowerEvent, PowerPolicy};

    println!("aether-powerd simulation: policy rules (D5 section 5)");
    let mut policy = PowerPolicyImpl::new(PowerdConfig::default().policy);
    let scenarios: Vec<(&str, PowerEvent)> = vec![
        ("source=battery", PowerEvent::SourceChanged(PowerSource::Battery)),
        (
            "battery=80",
            PowerEvent::BatteryUpdated(BatteryStatus {
                percent: 80.0,
                source: PowerSource::Battery,
                ..Default::default()
            }),
        ),
        (
            "battery=19",
            PowerEvent::BatteryUpdated(BatteryStatus {
                percent: 19.0,
                source: PowerSource::Battery,
                ..Default::default()
            }),
        ),
        (
            "battery=9",
            PowerEvent::BatteryUpdated(BatteryStatus {
                percent: 9.0,
                source: PowerSource::Battery,
                ..Default::default()
            }),
        ),
        (
            "battery=4",
            PowerEvent::BatteryUpdated(BatteryStatus {
                percent: 4.0,
                source: PowerSource::Battery,
                ..Default::default()
            }),
        ),
        (
            "battery=2",
            PowerEvent::BatteryUpdated(BatteryStatus {
                percent: 2.0,
                source: PowerSource::Battery,
                ..Default::default()
            }),
        ),
        ("source=ac", PowerEvent::SourceChanged(PowerSource::Ac)),
        (
            "user=performance",
            PowerEvent::UserProfileRequest(aether_powerd::types::PowerProfile::Performance),
        ),
        (
            "thermal=hot",
            PowerEvent::ThermalChanged(aether_powerd::types::ThermalLevel::Hot),
        ),
    ];
    for (label, event) in scenarios {
        let actions = policy.on_event(event);
        println!(
            "{label}: profile={:?} low={:?} actions={}",
            policy.profile(),
            policy.low_state(),
            actions.len()
        );
        for action in actions {
            println!("    {action:?}");
        }
    }
}
