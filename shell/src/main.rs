//! AetherOS desktop shell entry point (P4, S3).
//!
//! Today the shell runs headlessly: it composes every surface, applies the
//! presentation logic, can render each surface to a PNG, and can verify the
//! layer-shell host contract and the compositor's effect/frame markers against
//! a compositor log. The interactive host (a layer-shell `Platform` over the
//! VRAM/software renderer, proven by spike S1) arrives with the on-device
//! image. Until then this binary is the reproducible way to inspect and verify
//! the shell:
//!
//! ```text
//! aether-shell --render docs/2026/11/23/results/screenshots
//! aether-shell --status
//! aether-shell --host-check /var/log/aether-compositor.log
//! aether-shell --effects-tier
//! eval "$(aether-shell --effects-env)"
//! aether-shell --effects-check /var/log/aether-compositor.log
//! aether-shell --nohang-check /var/log/aether-compositor.log
//! aether-shell --start-begin
//! ```

use std::path::PathBuf;

use aether_shell::config::SettingsStore;
use aether_shell::effects::{self, EffectSettings};
use aether_shell::nohang::{self, StartLedger};
use aether_shell::render;
use aether_shell::services::CommandServices;
use aether_shell::{Overlay, ShellController, ShellRoot};

fn usage() {
    println!(
        "aether-shell — AetherOS desktop shell (P4)\n\
         \n\
         Usage:\n\
         \x20 aether-shell --render <dir>          render every surface to PNG\n\
         \x20 aether-shell --status                print shell state as JSON-ish text\n\
         \x20 aether-shell --host-check <log|->    verify the layer-shell host contract\n\
         \x20 aether-shell --host-plan             print the layer-surface request plan\n\
         \x20 aether-shell --effects-tier          print the effective glass tier\n\
         \x20 aether-shell --effects-env           print the compositor tier export\n\
         \x20 aether-shell --effects-check <log|-> verify the compositor's effect/frame markers\n\
         \x20 aether-shell --nohang-check <log|->  verify the no-hang markers and the G-NH2 bound\n\
         \x20 aether-shell --start-begin [log]     count a start (safe mode after 3 failed)\n\
         \x20 aether-shell --start-confirm         record that the session is ready\n\
         \x20 aether-shell --safe-mode-reset       leave safe mode\n\
         \x20 aether-shell --help                  this message"
    );
}

/// Read a log path, or stdin for `-`.
fn read_source(source: &str) -> Result<String, Box<dyn std::error::Error>> {
    if source == "-" {
        use std::io::Read;
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        Ok(buffer)
    } else {
        std::fs::read_to_string(source)
            .map_err(|error| format!("cannot read compositor log {source}: {error}").into())
    }
}

/// Effective tier from the user's settings, for the session launcher:
/// `AETHER_COMPOSITOR_EFFECT_TIER=$(aether-shell --effects-tier) aether-compositor`.
fn effects_tier() -> EffectSettings {
    EffectSettings::load(&SettingsStore::at_default_location())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--render") => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("screenshots"));
            for (name, path) in render::render_all(&dir)? {
                println!("AETHER_SHELL_RENDERED {name} -> {path}");
            }
            println!("AETHER_SHELL_RENDER_DONE");
        }
        Some("--host-check") => {
            // P3-T02: the compositor log proves every shell surface bound and
            // was configured at the size the shell draws.
            let Some(source) = args.get(1) else {
                eprintln!("--host-check needs a compositor log path (or - for stdin)");
                std::process::exit(2);
            };
            let text = read_source(source)?;
            let report = aether_shell::BindingReport::scan(&text);
            print!("{}", report.summary());
            if !report.is_complete() {
                std::process::exit(1);
            }
        }
        Some("--host-plan") => {
            for request in aether_shell::layer_plan() {
                println!(
                    "LAYER_REQUEST name={} namespace={} layer={} anchors={} size={}x{} exclusive_zone={} keyboard={}",
                    request.name,
                    request.namespace,
                    request.layer_name,
                    request.anchors,
                    request.width,
                    request.height,
                    request.exclusive_zone,
                    request.keyboard_interactivity
                );
            }
            println!("AETHER_SHELL_LAYER_PLAN_OK");
        }
        Some("--effects-tier") => {
            println!("{}", effects_tier().effective().as_str());
        }
        Some("--effects-env") => {
            // Sourced by the session launcher before the compositor starts.
            let settings = effects_tier();
            println!(
                "# effects: requested={} reduce-transparency={}",
                settings.requested.as_str(),
                settings.reduce_transparency
            );
            println!("export {}", settings.env_line());
        }
        Some("--effects-check") => {
            // P4-T04 ↔ P3-T05: the compositor's own markers must agree with the
            // persisted tier and prove the frame/idle pipeline (P3-T06).
            let Some(source) = args.get(1) else {
                eprintln!("--effects-check needs a compositor log path (or - for stdin)");
                std::process::exit(2);
            };
            let expected = effects_tier().effective();
            let text = read_source(source)?;
            let report = effects::EffectsReport::scan(&text);
            print!("{}", report.summary(expected));
            if !report.is_complete(expected) {
                std::process::exit(1);
            }
        }
        Some("--nohang-check") => {
            // G-NH2: the compositor's own no-hang markers must show a hang
            // inside the 6 s bound and a shell that is still committing.
            let Some(source) = args.get(1) else {
                eprintln!("--nohang-check needs a compositor log path (or - for stdin)");
                std::process::exit(2);
            };
            let store = SettingsStore::at_default_location();
            let ledger = StartLedger::load(&StartLedger::path(&store));
            let text = read_source(source)?;
            let report = nohang::NoHangReport::scan(&text);
            print!("{}", report.summary(&ledger));
            if !report.is_complete() {
                std::process::exit(1);
            }
        }
        Some("--start-begin") => {
            // Session launcher: count this start before the surfaces bind. The
            // optional argument is the compositor journal, so a shell that was
            // watchdog-restarted three times also enters safe mode.
            let store = SettingsStore::at_default_location();
            let path = StartLedger::path(&store);
            let mut ledger = StartLedger::load(&path);
            if let Some(source) = args.get(1) {
                let text = read_source(source)?;
                ledger.observe_watchdog(&nohang::WatchdogReport::scan(&text));
            }
            let outcome = ledger.begin_start();
            print!("{}", ledger.start_line(&outcome));
            if outcome.safe_mode {
                print!("{}", ledger.safe_mode_line());
            }
            ledger.save(&path)?;
        }
        Some("--start-confirm") => {
            // The session bound every surface: the start is good.
            let store = SettingsStore::at_default_location();
            let path = StartLedger::path(&store);
            let mut ledger = StartLedger::load(&path);
            ledger.confirm_start();
            ledger.save(&path)?;
            println!("AETHER_SHELL_START_CONFIRMED: attempts={}", ledger.attempts);
        }
        Some("--safe-mode-reset") => {
            let store = SettingsStore::at_default_location();
            let path = StartLedger::path(&store);
            let mut ledger = StartLedger::load(&path);
            ledger.reset();
            ledger.save(&path)?;
            println!(
                "AETHER_SHELL_SAFE_MODE_RESET: attempts={} safe_mode=false",
                ledger.attempts
            );
        }
        Some("--status") => {
            aether_ui::render::init()?;
            let ui = ShellRoot::new()?;
            let store = SettingsStore::at_default_location();
            let controller = ShellController::with_demo_content(
                ui,
                Box::new(CommandServices::new()),
                store,
            );
            controller.open(Overlay::None);
            let state = controller.state();
            println!("active-app: {}", state.active_app);
            println!("clock: {}", state.clock);
            println!("battery: {}", state.battery_label());
            println!("power-mode: {}", state.power_mode.as_str());
            println!("wifi: {}", state.wifi);
            println!("bluetooth: {}", state.bluetooth);
            println!("focus: {}", state.focus_active());
            println!("notifications: {}", state.notifications.len());
            println!("dock: {}", state.dock_apps().len());
            println!("windows: {}", state.windows.len());
            println!("effects-tier: {}", controller.effect_settings().requested.as_str());
            println!("effects-effective: {}", controller.effect_tier().as_str());
            println!("effects-env: {}", controller.effect_env_line());
            println!("safe-mode: {}", controller.safe_mode());
            println!("start-attempts: {}", controller.startup().attempts);
            let nohang = controller
                .nohang()
                .current()
                .map(|app| format!("not-responding {}", app.id))
                .unwrap_or_else(|| "idle".to_string());
            println!("nohang: {nohang}");
            println!("settings-root: {}", controller.store().root().display());
            println!("AETHER_SHELL_STATUS_OK");
        }
        Some("--help") | None => usage(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}
