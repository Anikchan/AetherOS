//! Headless interaction test for AetherPad (P4-T10).
//!
//! Drives the real Slint tree: typing into the model, then clicking the
//! toolbar and the find toggle. Asserts both the Rust-side document state and
//! the properties the window shows. One test per file because the testing
//! backend installs a process-wide platform.

use std::path::PathBuf;
use std::rc::Rc;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::ComponentHandle;

use aether_text::{TextApp, TextController};

fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-text-ui-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Click the visible element with this accessible label.
async fn click(ui: &TextApp, label: &str) {
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
fn aetherpad_toolbar_drives_the_document_model() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_root("toolbar");
        let ui = TextApp::new().unwrap();
        let controller = TextController::new(ui, root.clone());
        controller
            .ui()
            .show()
            .expect("the window shows in the testing backend");
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1200, 800));

        // Start from an edited buffer so the counts and the dirty dot engage.
        // `set_text` is the same core path the editor's `edited` callback uses.
        controller.set_text("alpha beta gamma");
        assert_eq!(controller.word_count(), 3);
        assert_eq!(
            controller.word_count() as i32,
            controller.ui().get_word_count()
        );
        assert_eq!(
            controller.char_count() as i32,
            controller.ui().get_char_count()
        );
        assert!(controller.is_dirty());
        assert!(controller.ui().get_dirty());

        // Undo / redo are real clicks that walk the document history.
        click(controller.ui(), "Undo").await;
        assert_eq!(controller.text(), "", "undo restores the empty buffer");
        assert_eq!(controller.ui().get_editor_text().as_str(), "");
        click(controller.ui(), "Redo").await;
        assert_eq!(controller.text(), "alpha beta gamma");

        // The find toggle opens the find/replace bar in the window.
        assert!(!controller.ui().get_find_open());
        click(controller.ui(), "Find").await;
        assert!(controller.ui().get_find_open(), "Find opens the bar");

        // Saving an untitled document writes the autosave file under the
        // controller's config root and clears the dirty flag.
        click(controller.ui(), "Save").await;
        assert!(!controller.is_dirty());
        assert!(!controller.ui().get_dirty());
        let autosave = root.join("text").join("autosave.txt");
        assert!(
            autosave.exists(),
            "autosave written to {}",
            autosave.display()
        );
        assert_eq!(
            std::fs::read_to_string(&autosave).unwrap(),
            "alpha beta gamma"
        );

        // New clears the editor through the same core path.
        click(controller.ui(), "New").await;
        assert_eq!(controller.text(), "");
        assert_eq!(controller.ui().get_editor_text().as_str(), "");
        assert_eq!(controller.ui().get_word_count(), 0);
        assert_eq!(controller.ui().get_line_count(), 0);
        assert_eq!(controller.note(), "New document");

        let _ = Rc::strong_count(&controller);
        let _ = std::fs::remove_dir_all(&root);
    })
    .unwrap();
}
