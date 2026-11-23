//! Headless interaction test for the Files window (P4-T08).
//!
//! One `#[test]` per file: the Slint testing backend installs a process-wide
//! platform. Clicks go through the real widget tree and the assertions read
//! both the Slint properties and the Rust state / real filesystem.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::ComponentHandle;

use aether_files::{FilesApp, FilesController};

fn temp_root() -> PathBuf {
    static TEST_COUNTER: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "aether-files-ui-{}-{stamp}-{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

/// Click the largest visible element with this accessible label.
async fn click(ui: &FilesApp, label: &str) {
    click_pick(ui, label, true).await;
}

/// Sidebar items share their label with same-named folder rows; the sidebar
/// instance is the smaller one.
async fn click_sidebar(ui: &FilesApp, label: &str) {
    click_pick(ui, label, false).await;
}

async fn click_pick(ui: &FilesApp, label: &str, largest: bool) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |handle: &ElementHandle| handle.size().width * handle.size().height;
        if largest {
            area(b).partial_cmp(&area(a)).unwrap()
        } else {
            area(a).partial_cmp(&area(b)).unwrap()
        }
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn files_window_navigates_selects_and_pastes() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_root();
        fs::create_dir_all(root.join("Pictures")).unwrap();
        fs::write(root.join("Notes.md"), "# notes\n").unwrap();
        fs::write(root.join("Report.pdf"), "%PDF-1.4 demo").unwrap();

        let controller = FilesController::open(&root).unwrap();
        let ui = controller.ui();
        ui.show().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1200, 800));
        slint::platform::update_timers_and_animations();

        assert_eq!(ui.get_item_count(), 3, "two files and one folder");
        assert_eq!(ui.get_view_mode(), 0);
        assert_eq!(ui.get_path().to_string(), "Home");

        // Opening a folder with the toolbar Open button, then Back/Forward (P4-T08).
        let folder = ElementQuery::from_root(ui)
            .match_accessible_label("Pictures")
            .find_all()
            .into_iter()
            .max_by(|a, b| {
                (a.size().width * a.size().height)
                    .partial_cmp(&(b.size().width * b.size().height))
                    .unwrap()
            })
            .expect("folder row is in the tree");
        // Double-click opens the row; selection runs first, which rebuilds the
        // row model, so the test drives the same action through the toolbar.
        folder.single_click(PointerEventButton::Left).await;
        assert_eq!(ui.get_selected_count(), 1);
        click(&ui, "Open").await;
        assert_eq!(ui.get_path().to_string(), "Home / Pictures");
        assert_eq!(ui.get_item_count(), 0);
        click(&ui, "Back").await;
        assert_eq!(ui.get_path().to_string(), "Home", "Back returns to Home");
        click(&ui, "Forward").await;
        assert_eq!(
            ui.get_path().to_string(),
            "Home / Pictures",
            "Forward works after Back"
        );
        click(&ui, "Back").await;

        // Clicking a row toggles the Rust selection model and the Slint count.
        click(&ui, "Notes.md").await;
        assert_eq!(ui.get_selected_count(), 1);
        let canonical = root.join("Notes.md").canonicalize().unwrap();
        assert!(controller.state().selection.contains(&canonical));

        // A second row extends the selection; clicking it again removes it.
        click(&ui, "Report.pdf").await;
        assert_eq!(ui.get_selected_count(), 2);
        click(&ui, "Report.pdf").await;
        assert_eq!(ui.get_selected_count(), 1);
        click(&ui, "Report.pdf").await;
        assert_eq!(ui.get_selected_count(), 2);

        // Copy through the real toolbar button.
        click(&ui, "Copy").await;
        assert_eq!(ui.get_clipboard().to_string(), "2 items copied");

        // Sidebar navigation clears the selection and lists the target folder.
        click_sidebar(&ui, "Pictures").await;
        assert_eq!(ui.get_path().to_string(), "Home / Pictures");
        assert_eq!(ui.get_item_count(), 0);
        assert_eq!(ui.get_selected_count(), 0);
        assert_eq!(ui.get_location().to_string(), "pictures");

        // Paste writes both selected files into the folder the window shows.
        click(&ui, "Paste").await;
        assert_eq!(ui.get_item_count(), 2);
        assert_eq!(ui.get_selected_count(), 0);
        assert!(root.join("Pictures/Notes.md").exists());
        assert!(root.join("Pictures/Report.pdf").exists());
        assert_eq!(
            fs::read_to_string(root.join("Pictures/Notes.md")).unwrap(),
            "# notes\n"
        );

        // The view switch is a real segmented control.
        click(&ui, "Icons").await;
        assert_eq!(ui.get_view_mode(), 2);
        click(&ui, "Columns").await;
        assert_eq!(ui.get_view_mode(), 1);

        // Back returns to the previous folder.
        click(&ui, "Back").await;
        assert_eq!(ui.get_path().to_string(), "Home");
        assert_eq!(ui.get_item_count(), 3);

        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
