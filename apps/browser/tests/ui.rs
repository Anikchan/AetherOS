//! Headless interaction test for the browser chrome (P4-T18).
//!
//! One test per process because the Slint testing backend installs a global
//! platform. The engine is a `NullEngine`, so no external process is spawned.

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::{ComponentHandle, Model};
use slint::platform::PointerEventButton;

use aether_browser::engine::NullEngine;
use aether_browser::store::BrowserStore;
use aether_browser::{BrowserApp, BrowserController};

async fn click(ui: &BrowserApp, label: &str) {
    let handle = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0)
        .max_by(|a, b| {
            (a.size().width * a.size().height)
                .partial_cmp(&(b.size().width * b.size().height))
                .unwrap()
        })
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn browser_chrome_is_interactive() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = std::env::temp_dir().join(format!(
            "aether-browser-ui-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ui = BrowserApp::new().unwrap();
        let controller = BrowserController::new(
            ui,
            BrowserStore::open(&root),
            Box::new(NullEngine::new()),
        );
        controller.ui().show().unwrap();
        controller.ui().window().set_size(slint::PhysicalSize::new(1200, 800));

        let ui = controller.ui();
        // Start state: one home tab.
        assert_eq!(ui.get_tabs().row_count(), 1);
        assert_eq!(ui.get_address().to_string(), "about:home");

        // New tab button in the tab strip.
        click(ui, "Open new tab").await;
        assert_eq!(ui.get_tabs().row_count(), 2, "new tab opens");
        assert_eq!(controller.core().tabs.active, 1);

        // Navigate through the address bar path.
        controller.navigate("example.com");
        assert_eq!(ui.get_address().to_string(), "example.com/");
        assert_eq!(ui.get_history().row_count(), 1, "visit recorded");

        // Bookmark star toggles and the sidebar lists it.
        click(ui, "Bookmark this page").await;
        assert!(ui.get_bookmarked());
        assert_eq!(ui.get_bookmarks().row_count(), 1);
        controller.open_sidebar("bookmarks");
        assert_eq!(controller.sidebar_len(), 1);

        // Clicking the bookmark row navigates.
        let bookmark_title = ui.get_bookmarks().row_data(0).unwrap().title.to_string();
        click(ui, &bookmark_title).await;
        assert!(ui.get_address().to_string().contains("example.com"));

        // Sidebar section switching.
        controller.open_sidebar("history");
        assert_eq!(ui.get_section(), 1);
        assert_eq!(ui.get_history().row_count(), 1);

        // Closing the last remaining home tab keeps one tab alive.
        controller.close_tab(0);
        assert_eq!(ui.get_tabs().row_count(), 1);

        // Zoom controls clamp and reset.
        controller.ui().set_zoom(100);
        click(ui, "Zoom 100%").await;
        assert_eq!(ui.get_zoom(), 100);

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
