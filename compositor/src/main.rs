//! AetherOS compositor entry point (P3-T01).

use aether_compositor::run;

fn main() {
    if let Err(err) = run() {
        eprintln!("aether-compositor: FATAL: {err}");
        std::process::exit(1);
    }
}
