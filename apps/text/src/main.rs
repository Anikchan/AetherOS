//! `aether-text` entry point (P4-T10).
//!
//! The window is rendered headlessly for evidence:
//!
//! ```text
//! aether-text --render docs/2026/11/23/results/screenshots
//! ```
//!
//! The interactive host arrives with the compositor's layer-shell work; until
//! then the binary is the reproducible way to inspect the app.

use std::path::PathBuf;

use aether_text::{config_root, TextApp, TextController};

fn usage() {
    println!(
        "aether-text — AetherPad (P4-T10)\n\
         \n\
         Usage:\n\
         \x20 aether-text --render <dir>   render the window to <dir>/app-text.png\n\
         \x20 aether-text --help           this message"
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
            let ui = TextApp::new()?;
            let controller = TextController::with_demo(ui, config_root());
            let path = dir.join("app-text.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_TEXT_RENDERED {}x{} -> {}",
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
        None => println!("aether-text: use --render <dir> to render the window, --help for usage"),
    }
    Ok(())
}
