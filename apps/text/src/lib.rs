//! AetherOS AetherPad (P4-T10, DA-AC-01/02).
//!
//! * [`document`] — text buffer, bounded undo/redo, counts, find and replace,
//!   UTF-8 persistence, autosave and the recent file list. Every rule is unit
//!   tested there.
//! * [`TextController`] — the only place that mutates Slint properties. The
//!   window is a view over a [`Document`], so the counts, the dirty dot, the
//!   find match count and the save state all come from the core model.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

pub mod document;

pub use document::{
    autosave_path, config_root, short_path, Document, DocumentError, RecentFiles,
    DEFAULT_RECENT_LIMIT, MAX_DOCUMENT_BYTES,
};

slint::include_modules!();

struct State {
    doc: Document,
    recent: RecentFiles,
    root: PathBuf,
    find: String,
    replace: String,
    case_sensitive: bool,
    find_cursor: usize,
    selected_recent: i32,
    note: String,
}

/// Owns the document and mirrors it into the window.
pub struct TextController {
    ui: TextApp,
    state: RefCell<State>,
}

impl TextController {
    /// Empty document with the recent list loaded from `config_root`.
    pub fn new(ui: TextApp, config_root: impl Into<PathBuf>) -> Rc<TextController> {
        let root = config_root.into();
        let mut recent =
            RecentFiles::new(root.join("text").join("recent.txt"), DEFAULT_RECENT_LIMIT);
        let _ = recent.load();
        let controller = Rc::new(TextController {
            ui,
            state: RefCell::new(State {
                doc: Document::new(),
                recent,
                root: root.clone(),
                find: String::new(),
                replace: String::new(),
                case_sensitive: false,
                find_cursor: 0,
                selected_recent: -1,
                note: "Ready".to_string(),
            }),
        });
        controller.wire();
        controller.sync();
        controller
    }

    /// Demo content for the screenshot renderer: an unsaved document plus a
    /// recent file list, with the find bar closed.
    pub fn with_demo(ui: TextApp, config_root: impl Into<PathBuf>) -> Rc<TextController> {
        let controller = TextController::new(ui, config_root);
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/anik".to_string());
        {
            let mut state = controller.state.borrow_mut();
            let doc_path = PathBuf::from(&home).join("Documents/roadmap-notes.md");
            state.doc = Document::with_path(DEMO_TEXT, doc_path);
            state.note = "Autosaved 2 minutes ago".to_string();
            for path in [
                PathBuf::from(&home).join("Downloads/todo.txt"),
                PathBuf::from(&home).join("Projects/aetheros/README.md"),
                PathBuf::from(&home).join("Documents/meeting-2026-11-20.md"),
                PathBuf::from(&home).join("Documents/roadmap-notes.md"),
            ] {
                state.recent.add(path);
            }
            state.selected_recent = 0;
        }
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &TextApp {
        &self.ui
    }

    /// Current buffer text (what the editor shows).
    pub fn text(&self) -> String {
        self.state.borrow().doc.text().to_string()
    }

    pub fn word_count(&self) -> usize {
        self.state.borrow().doc.word_count()
    }

    pub fn char_count(&self) -> usize {
        self.state.borrow().doc.char_count()
    }

    pub fn line_count(&self) -> usize {
        self.state.borrow().doc.line_count()
    }

    pub fn is_dirty(&self) -> bool {
        self.state.borrow().doc.is_dirty()
    }

    pub fn note(&self) -> String {
        self.state.borrow().note.clone()
    }

    pub fn match_count(&self) -> usize {
        let state = self.state.borrow();
        state.doc.count_matches(&state.find, state.case_sensitive)
    }

    pub fn document(&self) -> Document {
        self.state.borrow().doc.clone()
    }

    /// The recent list as paths (front first).
    pub fn recent(&self) -> Vec<PathBuf> {
        self.state.borrow().recent.entries()
    }

    /// Replace the buffer through the same path as an editor edit.
    pub fn set_text(&self, text: &str) {
        {
            let mut state = self.state.borrow_mut();
            if let Err(error) = state.doc.set_text(text) {
                state.note = error.to_string();
            }
            state.find_cursor = 0;
        }
        self.sync();
    }

    /// Save to the document path, or to the autosave path when untitled.
    pub fn save(&self) {
        let mut note = String::new();
        {
            let mut state = self.state.borrow_mut();
            if state.doc.text().len() > MAX_DOCUMENT_BYTES {
                state.note = "Document is too large to save".to_string();
                drop(state);
                self.sync();
                return;
            }
            let target = state
                .doc
                .path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| state.root.join("text").join("autosave.txt"));
            match state.doc.save_as(&target) {
                Ok(path) => {
                    state.recent.add(&path);
                    if let Err(error) = state.recent.persist() {
                        note = error.to_string();
                    }
                    state.note = if note.is_empty() {
                        format!("Saved to {}", short_path(&path))
                    } else {
                        note
                    };
                    let selected = state
                        .recent
                        .entries()
                        .iter()
                        .position(|entry| entry == &path);
                    drop(state);
                    self.sync();
                    if let Some(index) = selected {
                        self.ui.set_selected_recent(index as i32);
                    }
                    return;
                }
                Err(error) => state.note = error.to_string(),
            }
        }
        self.sync();
    }

    /// Load `path` into the editor and remember it.
    pub fn open(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        match Document::load(path) {
            Ok(doc) => {
                let mut state = self.state.borrow_mut();
                state.doc = doc;
                state.recent.add(path);
                let _ = state.recent.persist();
                state.selected_recent = 0;
                state.note = format!("Opened {}", short_path(path));
                state.find_cursor = 0;
            }
            Err(error) => {
                self.state.borrow_mut().note = error.to_string();
            }
        }
        self.sync();
    }

    pub fn new_document(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.doc = Document::new();
            state.selected_recent = -1;
            state.find_cursor = 0;
            state.note = "New document".to_string();
        }
        self.sync();
    }

    pub fn undo(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.note = if state.doc.undo() {
                "Undo".to_string()
            } else {
                "Nothing to undo".to_string()
            };
        }
        self.sync();
    }

    pub fn redo(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.note = if state.doc.redo() {
                "Redo".to_string()
            } else {
                "Nothing to redo".to_string()
            };
        }
        self.sync();
    }

    pub fn toggle_find(&self) {
        let open = !self.ui.get_find_open();
        self.ui.set_find_open(open);
        if open {
            self.recount();
        }
    }

    /// Find the next match at or after the stored cursor and report it.
    pub fn find_next(&self) {
        let mut state = self.state.borrow_mut();
        if state.find.is_empty() {
            state.note = "Type something to find".to_string();
            drop(state);
            self.sync();
            return;
        }
        let total = state.doc.count_matches(&state.find, state.case_sensitive);
        let found = state
            .doc
            .find(&state.find, state.find_cursor, state.case_sensitive)
            .or_else(|| {
                // Wrap around to the start of the document.
                state.find_cursor = 0;
                state.doc.find(&state.find, 0, state.case_sensitive)
            });
        match found {
            Some((start, end)) => {
                state.find_cursor = end;
                state.note = format!(
                    "Match at line {} · {total} match(es) in the document",
                    line_of(state.doc.text(), start)
                );
            }
            None => state.note = "No matches".to_string(),
        }
        drop(state);
        self.sync();
    }

    pub fn replace_all(&self) {
        {
            let mut state = self.state.borrow_mut();
            let needle = state.find.clone();
            let replacement = state.replace.clone();
            let case_sensitive = state.case_sensitive;
            let replaced = state.doc.replace_all(&needle, &replacement, case_sensitive);
            state.find_cursor = 0;
            state.note = if replaced == 0 {
                "Nothing replaced".to_string()
            } else {
                format!("Replaced {replaced} match(es)")
            };
        }
        self.sync();
    }

    // --- wiring -----------------------------------------------------------

    fn wire(self: &Rc<Self>) {
        {
            let weak = Rc::downgrade(self);
            self.ui.on_editor_edited(move |text| {
                if let Some(controller) = weak.upgrade() {
                    controller.set_text(text.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_open_selected(move || {
                if let Some(controller) = weak.upgrade() {
                    let path = {
                        let state = controller.state.borrow();
                        let index = if state.selected_recent >= 0 {
                            state.selected_recent as usize
                        } else {
                            0
                        };
                        state.recent.entries().get(index).cloned()
                    };
                    match path {
                        Some(path) => controller.open(path),
                        None => {
                            let mut state = controller.state.borrow_mut();
                            state.note = "No recent document yet".to_string();
                            drop(state);
                            controller.sync();
                        }
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_new_document(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.new_document();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_save_document(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.save();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_undo(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.undo();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_redo(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.redo();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_open_recent(move |index| {
                if let Some(controller) = weak.upgrade() {
                    let path = controller
                        .state
                        .borrow()
                        .recent
                        .entries()
                        .get(index.max(0) as usize)
                        .cloned();
                    match path {
                        Some(path) => controller.open(path),
                        None => controller.state.borrow_mut().note = "No such document".into(),
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_toggle_find(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.toggle_find();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_close_find(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.ui.set_find_open(false);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_find_query_edited(move |text| {
                if let Some(controller) = weak.upgrade() {
                    {
                        let mut state = controller.state.borrow_mut();
                        state.find = text.to_string();
                        state.find_cursor = 0;
                    }
                    controller.sync();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_replace_edited(move |text| {
                if let Some(controller) = weak.upgrade() {
                    controller.state.borrow_mut().replace = text.to_string();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_case_toggled(move |checked| {
                if let Some(controller) = weak.upgrade() {
                    {
                        let mut state = controller.state.borrow_mut();
                        state.case_sensitive = checked;
                        state.find_cursor = 0;
                    }
                    controller.recount();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_find_next(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.find_next();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_replace_all(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.replace_all();
                }
            });
        }
    }

    fn recount(&self) {
        let matches = self.match_count();
        self.ui.set_match_count(matches as i32);
    }

    /// Push the whole model into the window.
    fn sync(&self) {
        let state = self.state.borrow();
        let ui = &self.ui;

        if ui.get_editor_text().as_str() != state.doc.text() {
            ui.set_editor_text(SharedString::from(state.doc.text()));
        }
        ui.set_word_count(state.doc.word_count() as i32);
        ui.set_char_count(state.doc.char_count() as i32);
        ui.set_line_count(state.doc.line_count() as i32);
        ui.set_dirty(state.doc.is_dirty());
        ui.set_doc_name(SharedString::from(state.doc.name()));
        ui.set_doc_path(SharedString::from(state.doc.display_path()));
        ui.set_status_note(SharedString::from(state.note.clone()));
        ui.set_encoding_label(SharedString::from(state.doc.encoding().label()));
        ui.set_can_undo(state.doc.can_undo());
        ui.set_can_redo(state.doc.can_redo());
        ui.set_match_count(state.doc.count_matches(&state.find, state.case_sensitive) as i32);
        ui.set_case_sensitive(state.case_sensitive);
        ui.set_find_query(SharedString::from(state.find.clone()));
        ui.set_replace_text(SharedString::from(state.replace.clone()));
        ui.set_selected_recent(state.selected_recent);

        let rows: Vec<RecentEntry> = state
            .recent
            .labels()
            .into_iter()
            .map(|(name, path)| RecentEntry {
                name: name.into(),
                path: path.into(),
            })
            .collect();
        ui.set_recent(ModelRc::new(VecModel::from(rows)));
    }
}

/// 1-based line number of a char index, for the find status note.
fn line_of(text: &str, char_index: usize) -> usize {
    text.chars()
        .take(char_index)
        .filter(|ch| *ch == '\n')
        .count()
        + 1
}

/// Demo document used by the screenshot renderer.
const DEMO_TEXT: &str = "# AetherPad — release notes

## P4 · default apps

- AetherPad opens and saves UTF-8 documents without losing characters.
- Undo and redo keep a bounded history, so long sessions stay light.
- Find and replace run on unicode text; match case is one click away.
- Recent files are remembered under ~/.config/aether/text.

## Editor behaviour

The window is a view over apps/text/src/document.rs. Counts, the unsaved dot,
the find match count and the status note are all computed in Rust, never in
the .slint layer.

> Tip: press Find, type a query and use Next to walk the matches.

Next up: syntax colours for common languages (P4-T10c).
";
