//! Headless interaction test for the Terminal (P4-T15).
//!
//! Clicks the real tab strip and toolbar, then asserts the Rust-side terminal
//! state and the model the window renders. One test per file because the
//! testing backend installs a process-wide platform.

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model};

use aether_terminal::{TerminalApp, TerminalController};

/// Click the visible element with this accessible label.
async fn click(ui: &TerminalApp, label: &str) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |h: &ElementHandle| h.size().width * h.size().height;
        area(b).partial_cmp(&area(a)).unwrap()
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn terminal_tabs_and_screen_are_interactive() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let ui = TerminalApp::new().unwrap();
        let controller = TerminalController::with_demo(ui);
        controller.ui().show().expect("window shows");
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1200, 800));

        assert_eq!(controller.tab_titles(), vec!["zsh", "build"]);
        assert_eq!(controller.ui().get_active_tab(), 0);
        assert!(controller.ui().get_glyphs().row_count() > 100);
        assert_eq!(controller.viewport(), (124, 32));

        // Switching tabs is a real click on the tab strip.
        click(controller.ui(), "build").await;
        assert_eq!(controller.active_tab(), 1);
        assert_eq!(controller.ui().get_active_tab(), 1);
        assert!(controller.screen_text().contains("Build log"));

        click(controller.ui(), "zsh").await;
        assert_eq!(controller.active_tab(), 0);
        assert!(controller
            .screen_text()
            .contains("cargo test -p aether-terminal"));

        // Running a command goes through the same path as pressing ⏎ in the
        // prompt: the parser receives the echo plus its output. The testing
        // backend cannot synthesize Enter inside a TextInput, so the controller
        // method that the `accepted` callback invokes is called directly.
        controller.run_line("echo aether-rocks");
        assert!(controller.screen_text().contains("aether-rocks"));
        assert!(controller.screen_text().contains("$ echo aether-rocks"));

        // The trash button clears the visible screen through the toolbar.
        click(controller.ui(), "Clear screen").await;
        assert!(controller.screen_text().trim().is_empty());
        assert_eq!(controller.ui().get_glyphs().row_count(), 0);

        // A new tab appears in the model and becomes active.
        click(controller.ui(), "New tab").await;
        assert_eq!(controller.tab_titles().len(), 3);
        assert_eq!(controller.active_tab(), 2);
        assert_eq!(controller.ui().get_tabs().row_count(), 3);

        // Resizing the window (the host calls this) resizes the grid.
        controller.resize(100, 24);
        assert_eq!(controller.viewport(), (100, 24));
        assert_eq!(controller.ui().get_grid_cols(), 100);
        assert_eq!(controller.ui().get_grid_rows(), 24);

        // A real PTY shell reports its exit code instead of a raw wait word.
        click(controller.ui(), "build").await;
        assert_eq!(controller.active_tab(), 1);
        match controller.spawn_pty(80, 24) {
            Ok(()) => {
                controller
                    .write_pty(b"exit 9\n")
                    .expect("write exit to the pty");
                for _ in 0..200 {
                    controller.pump(50);
                    if controller.note().starts_with("Process ") {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                assert_eq!(controller.note(), "Process exit 9");
                assert!(controller.screen_text().contains("[process exit 9]"));
            }
            // A sandbox without /dev/ptmx still exercises the grid resize above.
            Err(error) => eprintln!("skipping PTY exit-code check: {error}"),
        }
    })
    .unwrap();
}
