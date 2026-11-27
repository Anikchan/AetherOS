//! `aether-calculator` entry point (P4-T11).
//!
//! ```text
//! aether-calculator --render docs/2026/11/23/results/screenshots
//! ```

use std::path::PathBuf;

use aether_calculator::{CalculatorApp, CalculatorController};

fn usage() {
    println!(
        "aether-calculator — Calculator (P4-T11)\n\
         \n\
         Usage:\n\
         \x20 aether-calculator --render <dir>   render to <dir>/app-calculator.png\n\
         \x20 aether-calculator --help           this message"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let startup =
        aether_launcher_registry::handle_launch_arg("aether-calculator", std::env::args().skip(1));
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
            aether_ui::render::init()?;
            let ui = CalculatorApp::new()?;
            let controller = CalculatorController::with_demo(ui);
            let path = dir.join("app-calculator.png");
            let frame = aether_ui::render::render_png(controller.ui(), 420, 720, &path)?;
            println!(
                "AETHER_APP_CALCULATOR_RENDERED {}x{} -> {}",
                frame.width,
                frame.height,
                path.display()
            );
        }
        Some("--help") => usage(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            usage();
            std::process::exit(2);
        }
        // No flags: the shell's launch path. Open the real window on the
        // session's Wayland display and run until it is closed.
        None => open_window()?,
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

    let ui = CalculatorApp::new()?;
    let controller = CalculatorController::with_demo(ui);
    controller.ui().show()?;
    slint::run_event_loop()?;
    Ok(())
}
