//! Painting the Slint scene for the layer host (P3-T02 → P4).
//!
//! The shell draws one composed scene and crops it into the layer-surface
//! buffers. This test drives that renderer directly (no compositor): it proves
//! a frame is painted, that an unchanged shell is *not* repainted, that a
//! settings change repaints and changes pixels, and it measures the cost of a
//! frame at the two reference sizes the D6 reports.
//!
//! One test function: the software-renderer platform is installed process-wide,
//! and a component's window must not be re-targeted while it is being painted
//! (`render::render_argb_into` draws the most recently created window, so the
//! two sizes are measured one component at a time).

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use aether_shell::config::SettingsStore;
use aether_shell::host::plan;
use aether_shell::paint::LayerPainter;
use aether_shell::services::{Battery, Services};
use aether_shell::{PowerMode, ShellController, ShellRoot};

#[derive(Default)]
struct QuietServices;

impl Services for QuietServices {
    fn set_wifi(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_bluetooth(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_brightness(&mut self, _brightness: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_power_mode(&mut self, _mode: PowerMode) -> Result<(), String> {
        Ok(())
    }
    fn battery(&mut self) -> Option<Battery> {
        Some(Battery {
            percent: 84,
            charging: false,
        })
    }
    fn lock_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn temp_config_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-layer-pixels-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn shell(tag: &str) -> (Rc<ShellController>, PathBuf) {
    let root = temp_config_root(tag);
    let ui = ShellRoot::new().expect("ShellRoot");
    let controller = ShellController::new(
        ui,
        Box::new(QuietServices),
        SettingsStore::new(root.clone()),
    );
    (controller, root)
}

/// Force the scene to change so the next paint is a real repaint.
fn dirty(controller: &ShellController, tick: u32) {
    controller
        .ui()
        .set_clock(format!("{:02}:{:02}", 9 + tick % 12, tick % 60).into());
}

#[test]
fn frames_repaint_on_change_and_respect_the_motion_settings() {
    aether_ui::render::init().expect("software renderer platform");

    // --- 1920x1080: the first frame, then a settings change ---------------
    let (controller, root) = shell("main");
    let output = (1920, 1080);
    let mut painter = LayerPainter::new(controller.ui(), output, 1).expect("painter");
    assert_eq!(painter.frames(), 1, "the host paints a first frame");
    assert_eq!(painter.size(), (1920, 1080));

    // Nothing changed: no repaint, no commits (the "no busy loop" contract).
    assert!(
        !painter.paint(controller.ui()).expect("paint"),
        "an idle shell must not repaint"
    );

    let before: Vec<u8> = painter
        .pixels()
        .iter()
        .flat_map(|pixel| [pixel.red, pixel.green, pixel.blue, pixel.alpha])
        .collect();
    // A settings change (Appearance → Text scale, 100 % → 125 %) must repaint
    // and must change the pixels.
    controller.cycle_setting("theme.text-scale");
    assert_eq!(
        controller.a11y().text_scale_percent(),
        125,
        "the settings change reached the accessibility tokens"
    );
    assert!(
        painter.paint(controller.ui()).expect("paint"),
        "a settings change repaints"
    );
    let after: Vec<u8> = painter
        .pixels()
        .iter()
        .flat_map(|pixel| [pixel.red, pixel.green, pixel.blue, pixel.alpha])
        .collect();
    assert_ne!(before, after, "two renders around a settings change differ");
    assert_eq!(painter.frames(), 2);

    // A glass-tier change (Appearance → Effects tier, full → reduced) reaches
    // the components. The tier selects the compositor's material (P4-T04) and
    // the panels' own tint does not change with it, so the assertion is on the
    // property the renderer reads, not on pixels.
    controller.cycle_setting("theme.effects-tier");
    assert_ne!(
        controller.effect_tier().as_str(),
        "full",
        "the tier changed"
    );
    assert_eq!(
        controller.ui().get_tier(),
        aether_shell::GlassTier::Reduced,
        "the tier reaches the rendered components"
    );
    assert!(
        painter.paint(controller.ui()).expect("paint"),
        "the sync that follows the change repaints"
    );

    // The dirty region is reported, and it is what the host uses to decide
    // which surfaces to commit.
    let menubar = plan()
        .into_iter()
        .find(|request| request.name == "menubar")
        .unwrap();
    assert!(
        painter.touched(&menubar),
        "the text-scale change repaints the menu bar"
    );

    // Per-surface copy cost (crop + ARGB packing) for every planned surface.
    let mut scratch = Vec::new();
    let mut surface_ms = Vec::new();
    for request in plan() {
        let started = Instant::now();
        painter
            .write_surface(&request, 1, &mut scratch)
            .expect("surface pixels");
        surface_ms.push((request.name, started.elapsed().as_secs_f64() * 1000.0));
    }

    // Frame cost: paint 20 changed frames and report the median of the last 17.
    let mut frames = Vec::new();
    for tick in 0..20 {
        dirty(&controller, tick);
        let started = Instant::now();
        assert!(painter.paint(controller.ui()).expect("paint"));
        frames.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    frames.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = frames[frames.len() / 2];
    println!(
        "AETHER_SHELL_PAINT_TIMING: size=1920x1080 canvas_ms={median:.3} max_ms={:.3} frames={}",
        frames[frames.len() - 1],
        painter.frames()
    );
    for (name, ms) in &surface_ms {
        println!("AETHER_SHELL_PAINT_SURFACE: size=1920x1080 surface={name} ms={ms:.3}");
    }

    // Reduce motion: the host ticks slower (animations are collapsed), which is
    // the only cadence change the settings ask for.
    controller.toggle_setting("theme.reduce-motion", true);
    assert!(controller.a11y().reduce_motion);
    let motion = aether_shell::wayland::frame_interval(&controller);
    assert_eq!(
        motion.as_millis(),
        100,
        "reduce-motion slows the frame clock"
    );
    controller.toggle_setting("theme.reduce-motion", false);
    assert_eq!(
        aether_shell::wayland::frame_interval(&controller).as_millis(),
        16
    );
    let _ = std::fs::remove_dir_all(root);

    // --- 1280x800: a second component, measured before anything else paints it
    let (small, root) = shell("small");
    let mut painter = LayerPainter::new(small.ui(), (1280, 800), 1).expect("painter");
    let mut frames = Vec::new();
    for tick in 0..20 {
        dirty(&small, tick);
        let started = Instant::now();
        assert!(painter.paint(small.ui()).expect("paint"));
        frames.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    frames.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = frames[frames.len() / 2];
    println!(
        "AETHER_SHELL_PAINT_TIMING: size=1280x800 canvas_ms={median:.3} max_ms={:.3} frames={}",
        frames[frames.len() - 1],
        painter.frames()
    );
    let mut scratch = Vec::new();
    for request in plan() {
        let started = Instant::now();
        painter
            .write_surface(&request, 1, &mut scratch)
            .expect("surface pixels");
        println!(
            "AETHER_SHELL_PAINT_SURFACE: size=1280x800 surface={} ms={:.3}",
            request.name,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    // A 2x output doubles the buffer but not the canvas: the copy upscales.
    let request = plan()
        .into_iter()
        .find(|request| request.name == "menubar")
        .unwrap();
    let (width, height) = painter
        .write_surface(&request, 2, &mut scratch)
        .expect("scaled surface pixels");
    assert_eq!((width, height), (2560, 56), "scale 2 doubles the buffer");

    let _ = std::fs::remove_dir_all(root);
}
