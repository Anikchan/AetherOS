//! AetherOS Images binary entry point (P4-T12).
//!
//! `--render <dir>` draws the app surface to `<dir>/app-images.png` with the
//! headless software renderer.

use std::path::PathBuf;

use aether_images::ImagesController;

fn usage() {
    println!("aether-images — AetherOS Images (P4-T12, DA-AC-04)");
    println!("usage: aether-images [--render <dir>] [--help]");
    println!("  --render <dir>  render the window to <dir>/app-images.png");
    println!("  --help          print this message");
}

fn main() {
    if let Some(code) =
        aether_launcher_registry::handle_launch_arg("aether-images", std::env::args().skip(1))
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
                    eprintln!("aether-images: --render needs a directory");
                    std::process::exit(2);
                }
            },
            "--help" | "-h" => {
                usage();
                return;
            }
            other => {
                eprintln!("aether-images: unknown argument {other}");
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
        eprintln!("aether-images: cannot initialise the renderer: {error}");
        std::process::exit(1);
    }
    let controller = match ImagesController::demo() {
        Ok(controller) => controller,
        Err(error) => {
            eprintln!("aether-images: cannot build the demo album: {error}");
            std::process::exit(1);
        }
    };
    let path = dir.join("app-images.png");
    match aether_ui::render::render_png(controller.ui(), 1200, 800, &path) {
        Ok(frame) => println!(
            "AETHER_APP_IMAGES_RENDERED {} ({}x{})",
            path.display(),
            frame.width,
            frame.height
        ),
        Err(error) => {
            eprintln!("aether-images: render failed: {error}");
            std::process::exit(1);
        }
    }
}
