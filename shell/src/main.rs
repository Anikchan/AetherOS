//! AetherOS desktop shell entry point (P4, S3).
//!
//! Today the shell runs headlessly: it composes every surface, applies the
//! presentation logic, can render each surface to a PNG, and can verify the
//! layer-shell host contract against a compositor log. The interactive host (a
//! layer-shell `Platform` over the VRAM/software renderer, proven by spike S1)
//! arrives with the on-device image. Until then this binary is the reproducible
//! way to inspect and verify the shell:
//!
//! ```text
//! aether-shell --render docs/2026/11/23/results/screenshots
//! aether-shell --status
//! aether-shell --host-check /var/log/aether-compositor.log
//! ```

use std::path::PathBuf;

use aether_shell::config::SettingsStore;
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
         \x20 aether-shell --help                  this message"
    );
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
            let text = if source == "-" {
                use std::io::Read;
                let mut buffer = String::new();
                std::io::stdin().read_to_string(&mut buffer)?;
                buffer
            } else {
                std::fs::read_to_string(source).map_err(|error| {
                    format!("cannot read compositor log {source}: {error}")
                })?
            };
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
