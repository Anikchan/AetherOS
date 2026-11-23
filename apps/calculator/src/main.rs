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
    let args: Vec<String> = std::env::args().skip(1).collect();
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
            let frame = aether_ui::render::render_png(controller.ui(), 420, 640, &path)?;
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
        None => {
            println!("aether-calculator: use --render <dir> to render the window, --help for usage")
        }
    }
    Ok(())
}
