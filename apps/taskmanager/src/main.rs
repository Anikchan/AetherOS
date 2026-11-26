//! `aether-taskmanager` — Activity Manager core CLI (P5-T07, F13).
//!
//! The Slint window is a thin binding over this CLI: every metric, action and
//! export below is the same code the window calls.
//!
//! Usage:
//!   aether-taskmanager --once [--json] [--limit N]
//!   aether-taskmanager --tui [--interval SECS]
//!   aether-taskmanager --spike-s7 [--sys-root PATH] [--report PATH]
//!   aether-taskmanager --end-task PID [--force] [--yes]
//!   aether-taskmanager --efficiency PID on|off [--yes]
//!   aether-taskmanager --export PATH [--format csv|json|text]
//!   aether-taskmanager --startup-list | --startup-disable FILE | --startup-enable FILE
//!   aether-taskmanager --services | --users
//!   aether-taskmanager --energy-contract
//!   aether-taskmanager --shortcut [--windows-profile]
//!   aether-taskmanager --tab processes|performance|battery|sensors
//!   aether-taskmanager --render DIR
//!
//! `--yes` is required for real signals and nice changes; without it actions
//! run in dry-run mode and print what they would do.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aether_lifecycled::daemon::MetricsSnapshot;
use aether_powerd::analytics::BatteryAnalytics;

use aether_taskmanager::collect::ProcFs;
use aether_taskmanager::manager::{ActivityManager, EndTaskOutcome};
use aether_taskmanager::model::ExportFormat;
use aether_taskmanager::s7;
use aether_taskmanager::window::{ActivityManagerWindow, RENDER_HEIGHT, RENDER_WIDTH};

#[derive(Default)]
struct Args {
    once: bool,
    json: bool,
    tui: bool,
    spike_s7: bool,
    energy_contract: bool,
    shortcut: bool,
    windows_profile: bool,
    tab: Option<String>,
    limit: Option<usize>,
    interval: u64,
    end_task: Option<u32>,
    efficiency: Option<(u32, bool)>,
    force: bool,
    yes: bool,
    export: Option<PathBuf>,
    format: Option<String>,
    startup_list: bool,
    startup_disable: Option<String>,
    startup_enable: Option<String>,
    services: bool,
    users: bool,
    sys_root: Option<PathBuf>,
    report: Option<PathBuf>,
    lifecycle_status: Option<PathBuf>,
    battery_status: Option<PathBuf>,
    power_history: Option<PathBuf>,
    render: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args {
        interval: 1,
        ..Default::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next().unwrap_or_else(|| {
                eprintln!("{name} needs a value");
                std::process::exit(2);
            })
        };
        match arg.as_str() {
            "--once" => args.once = true,
            "--json" => args.json = true,
            "--tui" => args.tui = true,
            "--spike-s7" => args.spike_s7 = true,
            "--energy-contract" => args.energy_contract = true,
            "--shortcut" => args.shortcut = true,
            "--windows-profile" => args.windows_profile = true,
            "--tab" => args.tab = Some(value("--tab")),
            "--limit" => args.limit = value("--limit").parse().ok(),
            "--interval" => args.interval = value("--interval").parse().unwrap_or(1),
            "--end-task" => args.end_task = value("--end-task").parse().ok(),
            "--efficiency" => {
                let pid = value("--efficiency").parse().unwrap_or(0);
                let mode = value("--efficiency");
                args.efficiency = Some((pid, mode == "on"));
            }
            "--force" => args.force = true,
            "--dry-run" => args.yes = false,
            "--yes" => args.yes = true,
            "--export" => args.export = Some(PathBuf::from(value("--export"))),
            "--format" => args.format = Some(value("--format")),
            "--startup-list" => args.startup_list = true,
            "--startup-disable" => args.startup_disable = Some(value("--startup-disable")),
            "--startup-enable" => args.startup_enable = Some(value("--startup-enable")),
            "--services" => args.services = true,
            "--users" => args.users = true,
            "--sys-root" => args.sys_root = Some(PathBuf::from(value("--sys-root"))),
            "--report" => args.report = Some(PathBuf::from(value("--report"))),
            "--lifecycle-status" => {
                args.lifecycle_status = Some(PathBuf::from(value("--lifecycle-status")))
            }
            "--battery-status" => {
                args.battery_status = Some(PathBuf::from(value("--battery-status")))
            }
            "--power-history" => {
                args.power_history = Some(PathBuf::from(value("--power-history")))
            }
            "--render" => args.render = Some(PathBuf::from(value("--render"))),
            "-h" | "--help" => {
                println!("usage: aether-taskmanager [--once|--tui|--spike-s7|--end-task PID|--efficiency PID on|off|--render DIR] [--json] [--limit N] [--dry-run]");
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

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn main() {
    let args = parse_args();
    if args.energy_contract {
        let contract = aether_taskmanager::EnergyImpact::contract();
        println!("{}", serde_json::to_string_pretty(&contract).unwrap_or_default());
        return;
    }
    if args.shortcut {
        let binding = aether_taskmanager::ShortcutBinding::default();
        let mut payload = serde_json::to_value(&binding).unwrap_or_default();
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "active_accel".into(),
                serde_json::Value::String(binding.accel(args.windows_profile).to_string()),
            );
            object.insert(
                "profile".into(),
                serde_json::Value::String(
                    if args.windows_profile { "windows" } else { "default" }.into(),
                ),
            );
        }
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
        return;
    }
    let roots = ProcFs::system();
    let mut manager = ActivityManager::new(roots.clone(), !args.yes);

    if let Some(dir) = &args.render {
        render_window(dir, &args, &mut manager);
        return;
    }

    if args.spike_s7 {
        let report = s7::run(args.sys_root.clone().unwrap_or(roots.sys_root.clone()));
        let markdown = s7::to_markdown(&report);
        if let Some(path) = &args.report {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, &markdown);
            println!("spike report: {}", path.display());
        } else {
            print!("{markdown}");
        }
        println!("SPIKE_S7_{:?}", report.verdict);
        return;
    }

    if let Some(pid) = args.end_task {
        let outcome = manager.end_task(pid, args.force);
        match outcome {
            EndTaskOutcome::Signalled { pid, force } => println!(
                "Sent {} to {pid}. Unsaved work may be lost; apps using the lifecycle SDK saved state first.",
                if force { "SIGKILL" } else { "SIGTERM" }
            ),
            EndTaskOutcome::DryRun { pid, force } => println!(
                "Dry run: would send {} to {pid} (pass --yes to act)",
                if force { "SIGKILL" } else { "SIGTERM" }
            ),
            EndTaskOutcome::Refused { pid, reason } => {
                eprintln!("Refused to end {pid}: {reason}");
                std::process::exit(1);
            }
        }
        return;
    }

    if let Some((pid, enabled)) = args.efficiency {
        match manager.set_efficiency(pid, enabled) {
            Ok(()) => println!(
                "Efficiency mode {} for {pid}{}",
                if enabled { "enabled" } else { "disabled" },
                if args.yes { "" } else { " (dry run)" }
            ),
            Err(error) => {
                eprintln!("efficiency mode failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.startup_list {
        for item in manager.startup_items() {
            println!(
                "{} {} {} {}",
                if item.enabled { "enabled " } else { "disabled" },
                match item.scope {
                    aether_taskmanager::model::StartupScope::User => "user  ",
                    aether_taskmanager::model::StartupScope::System => "system",
                },
                item.name,
                item.desktop_file
            );
        }
        return;
    }

    if let Some(file) = args.startup_disable.as_deref() {
        manager.set_startup_enabled(file, false).unwrap_or_else(|error| {
            eprintln!("startup disable failed: {error}");
            std::process::exit(1);
        });
        println!("disabled startup item: {file}");
        return;
    }

    if let Some(file) = args.startup_enable.as_deref() {
        manager.set_startup_enabled(file, true).unwrap_or_else(|error| {
            eprintln!("startup enable failed: {error}");
            std::process::exit(1);
        });
        println!("enabled startup item: {file}");
        return;
    }

    if args.services {
        for service in manager.services() {
            println!(
                "{} {} {} {}",
                service.unit, service.load, service.active, service.description
            );
        }
        return;
    }

    if args.users {
        for user in manager.users() {
            println!("{} {} {}", user.uid, user.user, user.seat.unwrap_or_default());
        }
        return;
    }

    let lifecycle = args.lifecycle_status.as_ref().and_then(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<MetricsSnapshot>(&text).ok())
    });
    let analytics = args.power_history.as_ref().and_then(|path| {
        BatteryAnalytics::load(path).ok()
    });

    let apply_context = |manager: &mut ActivityManager, rows: &mut Vec<_>| {
        if let Some(metrics) = &lifecycle {
            manager.merge_lifecycle(rows, metrics);
        }
        if let Some(analytics) = &analytics {
            manager.apply_energy_impact(rows, analytics);
        }
    };

    if let Some(name) = args.tab.as_deref() {
        let tab = aether_taskmanager::model::Tab::from_key(name).unwrap_or_else(|| {
            eprintln!("unknown tab: {name} (processes, performance, battery, sensors)");
            std::process::exit(2);
        });
        // On hosts without /proc (the macOS dev host) the tab still prints its
        // explicit "Not reported" fallbacks instead of failing.
        let (mut rows, system, history) = match manager.refresh(epoch_ms()) {
            Ok(snapshot) => (snapshot.rows, snapshot.system, snapshot.history),
            Err(error) => {
                eprintln!("sampling unavailable ({error}); printing tab fallbacks");
                (
                    Vec::new(),
                    aether_taskmanager::SystemSample::default(),
                    aether_taskmanager::PerformanceHistory::default(),
                )
            }
        };
        apply_context(&mut manager, &mut rows);
        match tab {
            aether_taskmanager::model::Tab::Processes => {
                print!("{}", ActivityManager::performance_text(&system));
                println!();
                print!("{}", ActivityManager::export(&rows, ExportFormat::Text));
            }
            aether_taskmanager::model::Tab::Performance => {
                print!(
                    "{}",
                    aether_taskmanager::performance_report(
                        &aether_taskmanager::performance_view(&system, &history)
                    )
                );
            }
            aether_taskmanager::model::Tab::Battery => {
                let status = args
                    .battery_status
                    .as_ref()
                    .and_then(|path| manager.battery_view(path));
                let view = aether_taskmanager::battery_view(
                    status.as_ref().or(system.battery.as_ref()),
                    analytics.as_ref(),
                    epoch_ms() / 1000,
                );
                print!("{}", aether_taskmanager::battery_report(&view));
            }
            aether_taskmanager::model::Tab::Sensors => {
                let sensors =
                    aether_taskmanager::collect::read_sensors(&roots.sys_root);
                let view = aether_taskmanager::sensors_view(&sensors, &system);
                print!("{}", aether_taskmanager::sensors_report(&view));
            }
            other => println!("{} tab has no rendered view yet (core data is sampled).", other.label()),
        }
        return;
    }

    if args.tui {
        loop {
            let snapshot = match manager.refresh(epoch_ms()) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    eprintln!("sampling failed: {error}");
                    std::process::exit(1);
                }
            };
            let mut rows = snapshot.rows.clone();
            apply_context(&mut manager, &mut rows);
            rows.sort_by(|a, b| {
                b.cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            print!("\x1b[2J\x1b[H");
            println!("Activity Manager — Processes (Ctrl+C to exit)\n");
            print!("{}", ActivityManager::performance_text(&snapshot.system));
            println!();
            println!(
                "{:<28} {:>7} {:>5} {:>8} {:>10} {:>8}",
                "Name", "PID", "State", "CPU %", "Memory", "Energy"
            );
            for row in rows.iter().take(args.limit.unwrap_or(40)) {
                println!(
                    "{:<28} {:>7} {:>5} {:>8.1} {:>10} {:>8}",
                    truncate(&row.name, 28),
                    row.pid,
                    row.state,
                    row.cpu_percent,
                    aether_taskmanager::manager::human_bytes(row.memory_bytes),
                    row.energy.label()
                );
            }
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_secs(args.interval.max(1)));
        }
    }

    // Default / --once.
    if let Some(path) = &args.export {
        let mut rows = match manager.refresh(epoch_ms()) {
            Ok(snapshot) => snapshot.rows,
            Err(error) => {
                eprintln!("sampling failed: {error}");
                std::process::exit(1);
            }
        };
        apply_context(&mut manager, &mut rows);
        let format = match args.format.as_deref().unwrap_or("csv") {
            "json" => ExportFormat::Json,
            "text" => ExportFormat::Text,
            _ => ExportFormat::Csv,
        };
        let content = ActivityManager::export(&rows, format);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, &content).unwrap_or_else(|error| {
            eprintln!("export failed: {error}");
            std::process::exit(1);
        });
        println!("exported {} rows to {}", rows.len(), path.display());
        return;
    }

    let snapshot = match manager.refresh(epoch_ms()) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("sampling failed: {error}");
            std::process::exit(1);
        }
    };
    let mut rows = snapshot.rows.clone();
    apply_context(&mut manager, &mut rows);
    rows.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if args.limit.is_some() {
        rows.truncate(args.limit.unwrap_or(rows.len()));
    }

    if args.json {
        let payload = serde_json::json!({
            "system": snapshot.system,
            "rows": rows,
            "battery": args.battery_status.as_ref().and_then(|path| manager.battery_view(path)),
            "tabs": manager.tabs().iter().map(|tab| tab.label()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!("Activity Manager — Processes\n");
        print!("{}", ActivityManager::performance_text(&snapshot.system));
        println!();
        print!("{}", ActivityManager::export(&rows, ExportFormat::Text));
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

/// Draw the Activity Manager window to `<dir>/activity-manager-<tab>.png` for
/// every rendered tab through the shared headless renderer (P5 evidence).
///
/// The rows come from the live sampler where `/proc` exists and from
/// [`aether_taskmanager::demo_rows`] on the dev host; the Energy column is
/// always resolved through the contract, so the screenshot proves the binding.
fn render_window(dir: &Path, args: &Args, manager: &mut ActivityManager) {
    if let Err(error) = aether_ui::render::init() {
        eprintln!("cannot initialise the renderer: {error}");
        std::process::exit(1);
    }
    if let Err(error) = std::fs::create_dir_all(dir) {
        eprintln!("cannot create {}: {error}", dir.display());
        std::process::exit(1);
    }
    let snapshot = manager.refresh(epoch_ms()).ok();
    let (mut rows, system, mut history) = match snapshot {
        Some(snapshot) => (snapshot.rows, Some(snapshot.system), snapshot.history),
        None => (
            aether_taskmanager::demo_rows(),
            Some(aether_taskmanager::demo_system()),
            aether_taskmanager::PerformanceHistory::default(),
        ),
    };
    let analytics = args
        .power_history
        .as_ref()
        .and_then(|path| BatteryAnalytics::load(path).ok())
        .or_else(|| {
            // Evidence render without a history file: three demo apps so the
            // Battery tab shows the contract-driven energy table.
            let now = epoch_ms() / 1000;
            let mut analytics = BatteryAnalytics::new(64);
            analytics.record_app_energy(now - 600, "org.aether.Browser", 18.4);
            analytics.record_app_energy(now - 900, "org.aether.Player", 4.1);
            analytics.record_app_energy(now - 1_200, "org.aether.Files", 0.6);
            Some(analytics)
        });
    if let Some(analytics) = &analytics {
        manager.apply_energy_impact(&mut rows, analytics);
    }

    let tabs = manager.tabs();
    let sample_system = system.clone().unwrap_or_default();
    if history.cpu.is_empty() {
        // Evidence render on a host without /proc: draw the demo series.
        history = aether_taskmanager::demo_history(&sample_system);
    }
    let status = args
        .battery_status
        .as_ref()
        .and_then(|path| manager.battery_view(path))
        .or_else(|| sample_system.battery.clone())
        .or_else(|| {
            Some(aether_taskmanager::BatteryView {
                percent: 79.0,
                charging: false,
                power_w: Some(9.8),
                time_remaining_secs: Some(3_780),
                cycle_count: Some(120),
                health_percent: Some(88.5),
                temperature_c: Some(30.5),
            })
        });
    let sensors = {
        let live = aether_taskmanager::collect::read_sensors(&ProcFs::system().sys_root);
        if live.is_empty() {
            aether_taskmanager::demo_sensors()
        } else {
            live
        }
    };

    let mut rendered = Vec::new();
    for tab in [
        aether_taskmanager::model::Tab::Processes,
        aether_taskmanager::model::Tab::Performance,
        aether_taskmanager::model::Tab::Battery,
        aether_taskmanager::model::Tab::Sensors,
    ] {
        let window = match ActivityManagerWindow::new() {
            Ok(window) => window,
            Err(error) => {
                eprintln!("cannot build the window: {error}");
                std::process::exit(1);
            }
        };
        window.bind_tabs(&tabs, tab);
        window.bind_shortcut(&aether_taskmanager::ShortcutBinding::default(), args.windows_profile);
        window.bind(&rows, Some(&sample_system), "");
        match tab {
            aether_taskmanager::model::Tab::Performance => {
                window.bind_performance(&aether_taskmanager::performance_view(
                    &sample_system,
                    &history,
                ));
            }
            aether_taskmanager::model::Tab::Battery => {
                window.bind_battery(&aether_taskmanager::battery_view(
                    status.as_ref().or(sample_system.battery.as_ref()),
                    analytics.as_ref(),
                    epoch_ms() / 1000,
                ));
            }
            aether_taskmanager::model::Tab::Sensors => {
                window.bind_sensors(&aether_taskmanager::sensors_view(
                    &sensors,
                    &sample_system,
                ));
            }
            _ => {}
        }

        let path = dir.join(format!("activity-manager-{}.png", tab.key()));
        match aether_ui::render::render_png(window.ui(), RENDER_WIDTH, RENDER_HEIGHT, &path) {
            Ok(frame) => {
                println!(
                    "AETHER_ACTIVITY_MANAGER_RENDERED {} ({}x{}) [{}]",
                    path.display(),
                    frame.width,
                    frame.height,
                    tab.label()
                );
                rendered.push(path);
            }
            Err(error) => {
                eprintln!("render failed: {error}");
                std::process::exit(1);
            }
        }
    }
    if rendered.is_empty() {
        eprintln!("nothing rendered");
        std::process::exit(1);
    }
}
