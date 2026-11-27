//! `aether-calendar` entry point (P4-T16, DA-07).
//!
//! The app runs headlessly like the rest of P4: `--render` draws the real
//! component tree through `aether-ui`'s software renderer, so the screenshot is
//! produced from the same state the window would show.
//!
//! ```text
//! aether-calendar --render docs/2026/11/23/results/screenshots
//! aether-calendar --events          print the stored events as JSON
//! aether-calendar --alarms          print the stored alarms as JSON
//! aether-calendar --help
//! ```

use std::path::PathBuf;

use aether_calendar::{AlarmStore, CalendarApp, CalendarController, EventStore};
use slint::{ModelRc, VecModel};

fn usage() {
    println!(
        "aether-calendar — AetherOS Calendar & Clock (P4-T16)\n\
         \n\
         Usage:\n\
         \x20 aether-calendar --render <dir>  render app-calendar.png into <dir>\n\
         \x20 aether-calendar --events         print stored events and exit\n\
         \x20 aether-calendar --alarms         print stored alarms and exit\n\
         \x20 aether-calendar --help           this message"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let startup =
        aether_launcher_registry::handle_launch_arg("aether-calendar", std::env::args().skip(1));
    if let Some(code) = startup.exit {
        std::process::exit(code);
    }
    let args: Vec<String> = startup.rest;
    match args.first().map(String::as_str) {
        Some("--render") => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("screenshots"));
            // The headless platform must be installed before any component is
            // created, otherwise Slint picks its default (absent) backend.
            aether_ui::render::init()?;
            let ui = CalendarApp::new()?;
            let store = EventStore::new(
                std::env::temp_dir()
                    .join("aether-calendar-render")
                    .join("events.json"),
            );
            let controller = CalendarController::with_demo_events(ui, store);
            let path = dir.join("app-calendar.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_CALENDAR_RENDERED {} -> {} ({}x{})",
                dir.display(),
                path.display(),
                frame.width,
                frame.height
            );

            // Week view (tab 1).
            controller.ui().set_tab(1);
            let week_path = dir.join("app-calendar-week.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &week_path)?;
            println!(
                "AETHER_APP_CALENDAR_WEEK_RENDERED {} -> {} ({}x{})",
                dir.display(),
                week_path.display(),
                frame.width,
                frame.height
            );

            // The clock tab (Month = 0, Week = 1, Clock = 2) is a second
            // surface of the same window.
            controller.ui().set_tab(2);
            controller.ui().set_stopwatch_text("00:12.3".into());
            controller
                .ui()
                .set_laps(ModelRc::new(VecModel::from(vec![
                    slint::SharedString::from("00:08.4"),
                    slint::SharedString::from("00:03.9"),
                ])));
            controller.ui().set_timer_text("04:12".into());
            controller.ui().set_timer_progress(0.16);
            let clock_path = dir.join("app-calendar-clock.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &clock_path)?;
            println!(
                "AETHER_APP_CALENDAR_CLOCK_RENDERED {} -> {} ({}x{})",
                dir.display(),
                clock_path.display(),
                frame.width,
                frame.height
            );
        }
        Some("--events") => {
            let mut store = EventStore::at_config_dir();
            let error = store.load().err();
            println!("{}", serde_json::to_string_pretty(store.events())?);
            if let Some(error) = error {
                eprintln!("warning: {error}");
            }
        }
        Some("--alarms") => {
            let mut store = AlarmStore::at_config_dir();
            let error = store.load().err();
            println!("{}", serde_json::to_string_pretty(store.alarms())?);
            if let Some(error) = error {
                eprintln!("warning: {error}");
            }
        }
        Some("--help") => usage(),
        // No flags: the shell's launch path. Open the real window on the
        // session's Wayland display and run until it is closed.
        None => open_window()?,
        Some(other) => {
            eprintln!("unknown argument: {other}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}

/// Open the real window and run the Slint event loop until it is closed.
///
/// The app binaries opt into Slint's winit backend on Wayland (software
/// renderer → softbuffer → `wl_shm`, no GPU). `--render` never reaches this
/// path: it installs the headless `aether-ui` platform first.
fn open_window() -> Result<(), Box<dyn std::error::Error>> {
    use slint::ComponentHandle;

    let ui = CalendarApp::new()?;
    let controller = CalendarController::with_demo_events(ui, EventStore::at_config_dir());
    controller.ui().show()?;
    slint::run_event_loop()?;
    Ok(())
}
