//! Headless interaction test for the Images window (P4-T12).
//!
//! One `#[test]` per file: the testing backend installs a process-wide
//! platform. Images are generated at runtime, clicks go through the real
//! widget tree, and assertions read Slint properties plus the Rust state.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::ComponentHandle;

use aether_images::{ImagesApp, ImagesController};
use image::GenericImageView;

fn temp_root() -> PathBuf {
    static TEST_COUNTER: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "aether-images-ui-{}-{stamp}-{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn gradient(path: &PathBuf, width: u32, height: u32) {
    let mut buffer = image::RgbaImage::new(width, height);
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        *pixel = image::Rgba([
            (x % 255) as u8,
            (y % 255) as u8,
            ((x + y) % 255) as u8,
            255,
        ]);
    }
    image::DynamicImage::ImageRgba8(buffer).save(path).unwrap();
}

async fn click(ui: &ImagesApp, label: &str) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |handle: &ElementHandle| handle.size().width * handle.size().height;
        area(b).partial_cmp(&area(a)).unwrap()
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn images_window_navigates_rotates_flips_and_zooms() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_root();
        gradient(&root.join("alpha.png"), 400, 200);
        gradient(&root.join("bravo.png"), 600, 300);

        let controller = ImagesController::new().unwrap();
        controller.open_folder(&root.display().to_string());
        let ui = controller.ui();
        ui.show().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1200, 800));
        slint::platform::update_timers_and_animations();

        assert!(ui.get_has_image());
        assert_eq!(ui.get_total_images(), 2);
        assert_eq!(ui.get_current_index(), 0);
        assert_eq!(ui.get_file_name().to_string(), "alpha.png");
        assert_eq!(ui.get_dimensions().to_string(), "400 × 200");
        assert_eq!(ui.get_format_label().to_string(), "PNG");

        // Next/previous walk the sorted folder.
        click(&ui, "Next").await;
        assert_eq!(ui.get_current_index(), 1);
        assert_eq!(ui.get_file_name().to_string(), "bravo.png");
        assert_eq!(ui.get_dimensions().to_string(), "600 × 300");

        // Rotating 90° swaps the reported dimensions in Rust.
        click(&ui, "Rotate").await;
        assert_eq!(ui.get_image_width(), 300);
        assert_eq!(ui.get_image_height(), 600);
        assert_eq!(ui.get_dimensions().to_string(), "300 × 600");
        assert_eq!(controller.state().rotation, 1);
        assert_eq!(
            controller.state().image.as_ref().unwrap().dimensions(),
            (300, 600)
        );

        // Zoom in multiplies the fitted zoom and leaves Fit mode.
        let fitted = ui.get_zoom();
        click(&ui, "Zoom in").await;
        assert!(ui.get_zoom() > fitted, "{} > {}", ui.get_zoom(), fitted);
        assert!(!controller.state().fit_mode);
        click(&ui, "Actual size").await;
        assert_eq!(ui.get_zoom(), 1.0);
        click(&ui, "Fit").await;
        assert!(controller.state().fit_mode);
        assert!(ui.get_zoom() > 0.0);

        // Flips toggle the model.
        assert!(!ui.get_flipped_h());
        click(&ui, "Flip H").await;
        assert!(ui.get_flipped_h());
        click(&ui, "Flip V").await;
        assert!(ui.get_flipped_v());

        // Previous wraps back to the first file.
        click(&ui, "Previous").await;
        assert_eq!(ui.get_current_index(), 0);
        assert_eq!(ui.get_file_name().to_string(), "alpha.png");

        // A thumbnail click selects that file.
        click(&ui, "bravo.png").await;
        assert_eq!(ui.get_current_index(), 1);
        assert_eq!(ui.get_file_name().to_string(), "bravo.png");

        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
