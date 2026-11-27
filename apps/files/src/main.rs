//! AetherOS Files binary entry point (P4-T08).
//!
//! `--render <dir>` draws the app surface to `<dir>/app-files.png` through the
//! headless software renderer used by the P4 evidence pipeline.

use std::path::PathBuf;

use aether_files::FilesController;

fn usage() {
    println!("aether-files — AetherOS Files (P4-T08, F1 SHL-12)");
    println!("usage: aether-files [--render <dir>] [--help]");
    println!("  --render <dir>  render the window to <dir>/app-files.png");
    println!("  --help          print this message");
}

fn main() {
    let startup =
        aether_launcher_registry::handle_launch_arg("aether-files", std::env::args().skip(1));
    if let Some(code) = startup.exit {
        std::process::exit(code);
    }
    let mut args = startup.rest.into_iter();
    let mut render_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--render" => match args.next() {
                Some(dir) => render_dir = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("aether-files: --render needs a directory");
                    std::process::exit(2);
                }
            },
            "--help" | "-h" => {
                usage();
                return;
            }
            other => {
                eprintln!("aether-files: unknown argument {other}");
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
        eprintln!("aether-files: cannot initialise the renderer: {error}");
        std::process::exit(1);
    }
    let controller = match FilesController::demo() {
        Ok(controller) => controller,
        Err(error) => {
            eprintln!("aether-files: cannot build the demo folder: {error}");
            std::process::exit(1);
        }
    };
    let path = dir.join("app-files.png");
    match aether_ui::render::render_png(controller.ui(), 1200, 800, &path) {
        Ok(frame) => println!(
            "AETHER_APP_FILES_RENDERED {} ({}x{})",
            path.display(),
            frame.width,
            frame.height
        ),
        Err(error) => {
            eprintln!("aether-files: render failed: {error}");
            std::process::exit(1);
        }
    }
}
