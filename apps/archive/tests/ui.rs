//! Headless interaction test for the Archive window (P4-T17).
//!
//! One `#[test]` per file: the testing backend installs a process-wide
//! platform. The archive is built at runtime, clicks go through the widget
//! tree, and assertions read Slint properties plus the real archive on disk.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::ComponentHandle;

use aether_archive::archive;
use aether_archive::{ArchiveApp, ArchiveController};

fn temp_root() -> PathBuf {
    static TEST_COUNTER: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "aether-archive-ui-{}-{stamp}-{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

async fn click(ui: &ArchiveApp, label: &str) {
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
fn archive_window_lists_extracts_deletes_and_reports_errors() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_root();
        let source = root.join("src");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("a.txt"), "alpha").unwrap();
        fs::write(source.join("nested/b.txt"), "bravo").unwrap();

        let archive_path = root.join("bundle.tar.gz");
        archive::create_tar_gz(&archive_path, &[source.clone()], &mut |_, _| {}).unwrap();
        let expected = archive::list(&archive_path).unwrap().1.len();
        assert_eq!(expected, 4, "src, a.txt, nested, nested/b.txt");

        let controller = ArchiveController::new().unwrap();
        controller.open(&archive_path.display().to_string());
        let ui = controller.ui();
        ui.show().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1200, 800));
        slint::platform::update_timers_and_animations();

        assert_eq!(ui.get_entry_count(), 4);
        assert_eq!(ui.get_format_label().to_string(), "TAR.GZ");
        assert_eq!(ui.get_error().to_string(), "");

        // Clicking an entry toggles the Rust selection.
        click(&ui, "src/a.txt").await;
        assert_eq!(ui.get_selected_count(), 1);
        assert_eq!(controller.state().selection, vec!["src/a.txt".to_string()]);

        // Extract all writes the real tree next to the archive.
        click(&ui, "Extract all").await;
        assert_eq!(ui.get_progress(), 1.0);
        assert!(ui.get_status().to_string().contains("Extracted"));
        let extracted = root.join("bundle-extracted");
        assert_eq!(
            fs::read_to_string(extracted.join("src/a.txt")).unwrap(),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(extracted.join("src/nested/b.txt")).unwrap(),
            "bravo"
        );

        // Delete rebuilds the archive without the selected entry.
        click(&ui, "Delete selected").await;
        assert_eq!(ui.get_entry_count() as usize, expected - 1);
        assert_eq!(ui.get_selected_count(), 0);
        let names: Vec<String> = archive::list(&archive_path)
            .unwrap()
            .1
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert!(!names.iter().any(|name| name == "src/a.txt"));
        assert!(names.iter().any(|name| name == "src/nested/b.txt"));

        // Extracting with an empty selection surfaces the error banner.
        click(&ui, "Extract selected").await;
        assert!(!ui.get_error().to_string().is_empty());
        click(&ui, "Dismiss error").await;
        assert_eq!(ui.get_error().to_string(), "");

        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
