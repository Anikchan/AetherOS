//! Headless interaction test for the Documents window (P4-T13).
//!
//! One test per file: the testing backend installs a process-wide platform.

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model};

use aether_documents::document::RecentFiles;
use aether_documents::{DocumentsApp, DocumentsController};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aether-documents-ui-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn sample_markdown() -> String {
    let mut text = String::from("# AetherOS reading list\n\n");
    for index in 1..=10 {
        text.push_str(&format!("## Chapter {index}\n\n"));
        for _ in 0..3 {
            text.push_str("The glass panel keeps the desktop visible while the reading surface stays calm and legible.\n\n");
        }
        text.push_str("- a bullet about glass\n- another bullet\n\n");
    }
    text
}

/// Click the visible instance with this accessible label.
async fn click(ui: &DocumentsApp, label: &str) {
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
fn documents_window_clicking_updates_rust_state() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let dir = temp_dir("interaction");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("reading-list.md");
        std::fs::write(&path, sample_markdown()).unwrap();

        let recent = RecentFiles::new(dir.join("recent.txt"));
        let ui = DocumentsApp::new().unwrap();
        let controller = DocumentsController::new(ui, recent);
        controller.open_path(&path);
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1200, 800));

        // Opening the file populated the viewer.
        assert!(controller.page_count() >= 2, "the sample spans several pages");
        assert_eq!(controller.page_index(), 0);
        assert_eq!(
            controller.ui().get_page_label().to_string(),
            format!("Page 1 of {}", controller.page_count())
        );
        assert!(controller.ui().get_blocks().row_count() > 0);
        assert_eq!(controller.ui().get_pages().row_count(), controller.page_count());
        assert_eq!(controller.recent().entries()[0], path, "opening records the file");

        // --- page navigation through the toolbar ---------------------------
        click(controller.ui(), "Next page").await;
        assert_eq!(controller.page_index(), 1);
        assert!(controller
            .ui()
            .get_page_label()
            .to_string()
            .starts_with("Page 2 of"));

        click(controller.ui(), "Previous page").await;
        assert_eq!(controller.page_index(), 0);

        // --- zoom ----------------------------------------------------------
        assert_eq!(controller.ui().get_zoom_label().to_string(), "100%");
        click(controller.ui(), "Zoom in").await;
        assert_eq!(controller.ui().get_zoom_label().to_string(), "115%");
        assert!(controller.ui().get_font_scale() > 1.0);
        click(controller.ui(), "Zoom out").await;
        assert_eq!(controller.ui().get_zoom_label().to_string(), "100%");

        // --- find ----------------------------------------------------------
        let search = ElementHandle::find_by_accessible_label(controller.ui(), "Find in document")
            .next()
            .expect("find field is in the tree");
        search.set_accessible_value("AetherOS");
        click(controller.ui(), "Find").await;

        let hits = controller.hits();
        assert_eq!(hits.len(), 1, "only the title mentions AetherOS");
        drop(hits);
        assert_eq!(controller.ui().get_hits().row_count(), 1);
        assert!(controller
            .ui()
            .get_find_label()
            .to_string()
            .contains("match"));

        // Searching for a common word fills the results bar with page chips.
        search.set_accessible_value("glass");
        click(controller.ui(), "Find").await;
        assert!(controller.hits().len() > 5);
        assert!(controller.ui().get_hits().row_count() >= 2, "chips per page");

        // --- jump to a page from the sidebar -------------------------------
        click(controller.ui(), "Page 3").await;
        assert_eq!(controller.page_index(), 2, "the page thumbnail jumps to page 3");

        // --- a missing file reports a status, never panics -----------------
        click(controller.ui(), "Match on page p. 1").await;
        assert_eq!(controller.page_index(), 0);
        assert!(controller.ui().get_status().is_empty());

        let _ = std::fs::remove_dir_all(dir);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
