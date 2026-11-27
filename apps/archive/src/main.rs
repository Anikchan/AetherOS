//! AetherOS Archive binary entry point (P4-T17).
//!
//! `--render <dir>` draws the app surface to `<dir>/app-archive.png` with the
//! headless software renderer.

use std::path::PathBuf;

use aether_archive::ArchiveController;

fn usage() {
    println!("aether-archive — AetherOS Archive (P4-T17, DA-AC-08)");
    println!("usage: aether-archive [--render <dir>] [--help]");
    println!("  --render <dir>  render the window to <dir>/app-archive.png");
    println!("  --help          print this message");
}

fn main() {
    if let Some(code) =
        aether_launcher_registry::handle_launch_arg("aether-archive", std::env::args().skip(1))
    {
        std::process::exit(code);
    }
    let mut args = std::env::args().skip(1);
    let mut render_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--render" => match args.next() {
                Some(dir) => render_dir = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("aether-archive: --render needs a directory");
                    std::process::exit(2);
                }
            },
            "--help" | "-h" => {
                usage();
                return;
            }
            other => {
                eprintln!("aether-archive: unknown argument {other}");
                usage();
                std::process::exit(2);
            }
        }
    }

    let Some(dir) = render_dir else {
        usage();
        return;
    };

    if let Err(error) = aether_ui::render::init() {
        eprintln!("aether-archive: cannot initialise the renderer: {error}");
        std::process::exit(1);
    }
    let controller = match ArchiveController::demo() {
        Ok(controller) => controller,
        Err(error) => {
            eprintln!("aether-archive: cannot build the demo archive: {error}");
            std::process::exit(1);
        }
    };
    let path = dir.join("app-archive.png");
    match aether_ui::render::render_png(controller.ui(), 1200, 800, &path) {
        Ok(frame) => println!(
            "AETHER_APP_ARCHIVE_RENDERED {} ({}x{})",
            path.display(),
            frame.width,
            frame.height
        ),
        Err(error) => {
            eprintln!("aether-archive: render failed: {error}");
            std::process::exit(1);
        }
    }
}
