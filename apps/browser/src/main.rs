//! `aether-browser` (P4-T18): tabs, bookmarks, history, downloads, private tabs.
//!
//! Usage:
//!   aether-browser --render <dir>   write app-browser.png (headless evidence)
//!   aether-browser --help

use aether_browser::{BrowserController, BrowserApp};

fn usage() {
    println!(
        "aether-browser — AetherOS default browser (P4-T18)\n\
         \n\
         \x20 --render <dir>  render the browser window to <dir>/app-browser.png\n\
         \x20 --help          this message"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--render") => {
            let dir = args.get(1).cloned().unwrap_or_else(|| "screenshots".into());
            aether_ui::render::init()?;
            let ui = BrowserApp::new()?;
            let controller = BrowserController::with_demo(ui);
            let path = std::path::Path::new(&dir).join("app-browser.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_BROWSER_RENDERED {}x{} -> {}",
                frame.width,
                frame.height,
                path.display()
            );
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
