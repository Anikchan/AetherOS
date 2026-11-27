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
    let startup =
        aether_launcher_registry::handle_launch_arg("aether-images", std::env::args().skip(1));
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
        // Shell launch path: open the real window on the session's Wayland
        // display (winit + software renderer) and run until it is closed.
        run_window();
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

/// Open the real window and run the Slint event loop until it is closed.
///
/// The app binaries opt into Slint's winit backend on Wayland (software
/// renderer → softbuffer → `wl_shm`, no GPU). `--render` never reaches this
/// path: it installs the headless `aether-ui` platform first.
fn run_window() {
    if let Err(error) = open_window() {
        eprintln!("aether-images: cannot open the window: {error}");
        std::process::exit(1);
    }
}

fn open_window() -> Result<(), Box<dyn std::error::Error>> {
    use slint::ComponentHandle;

    let controller = ImagesController::demo()?;
    controller.ui().show()?;
    slint::run_event_loop()?;
    Ok(())
}
