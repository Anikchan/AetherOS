//! `aether-player` entry point (P4-T14, DA-AC-06).
//!
//! ```text
//! aether-player --render <dir>  render app-player.png into <dir>
//! aether-player --status        print the queue and the audio backend
//! aether-player --help
//! ```

use std::path::PathBuf;

use aether_player::engine::{Engine, GStreamerEngine, MockEngine};
use aether_player::{demo_playlist, PlayerApp, PlayerController};

fn usage() {
    println!(
        "aether-player — AetherOS Player (P4-T14)\n\
         \n\
         Usage:\n\
         \x20 aether-player --render <dir>  render app-player.png into <dir>\n\
         \x20 aether-player --status        print queue and audio backend state\n\
         \x20 aether-player --help          this message"
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
            let ui = PlayerApp::new()?;
            let controller = PlayerController::with_demo(ui);
            let path = dir.join("app-player.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_PLAYER_RENDERED {} -> {} ({}x{})",
                dir.display(),
                path.display(),
                frame.width,
                frame.height
            );
        }
        Some("--status") => {
            // Rendering must not start audio, and neither must --status, so the
            // mock engine stands in when GStreamer is present. The backend line
            // reports what the real adapter detected.
            let backend = match GStreamerEngine::detect() {
                Ok(engine) => format!("gstreamer ({})", engine.program().display()),
                Err(error) => error.message(),
            };
            let playlist = demo_playlist(7);
            println!("backend: {backend}");
            println!("engine-name: {}", MockEngine::new().name());
            println!("tracks: {}", playlist.len());
            println!("total: {}", playlist.total_label());
            println!("repeat: {}", playlist.repeat().label());
            println!("shuffle: {}", playlist.is_shuffled());
            for (index, track) in playlist.tracks().iter().enumerate() {
                println!(
                    "{}. {} — {} ({})",
                    index + 1,
                    track.title,
                    track.artist,
                    track.duration_label()
                );
            }
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
