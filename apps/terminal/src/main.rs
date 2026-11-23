//! `aether-terminal` entry point (P4-T15).
//!
//! ```text
//! aether-terminal --render docs/2026/11/23/results/screenshots
//! ```

use std::path::PathBuf;

use aether_terminal::{TerminalApp, TerminalController};

fn usage() {
    println!(
        "aether-terminal — Terminal (P4-T15)\n\
         \n\
         Usage:\n\
         \x20 aether-terminal --render <dir>   render to <dir>/app-terminal.png\n\
         \x20 aether-terminal --help           this message"
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
            let ui = TerminalApp::new()?;
            let controller = TerminalController::with_demo(ui);
            let path = dir.join("app-terminal.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_TERMINAL_RENDERED {}x{} -> {}",
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
            println!("aether-terminal: use --render <dir> to render the window, --help for usage")
        }
    }
    Ok(())
}
