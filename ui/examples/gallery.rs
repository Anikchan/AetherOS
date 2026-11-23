//! Render the design-system gallery headlessly (P4 evidence screenshot).
//!
//! Usage: cargo run -p aether-ui --example gallery [output.png]

use aether_ui::DesignGallery;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "docs/2026/11/23/results/screenshots/ui-gallery.png".to_string());
    aether_ui::render::init().expect("install headless platform");
    let ui = DesignGallery::new().expect("create gallery");
    let frame = aether_ui::render::render_png(&ui, 960, 620, &out).expect("render gallery");
    println!("AETHER_UI_GALLERY_RENDERED {}x{} -> {}", frame.width, frame.height, out);
}
