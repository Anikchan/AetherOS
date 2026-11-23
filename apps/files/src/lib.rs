//! AetherOS Files (P4-T08, F1 SHL-12).
//!
//! [`fsops`] holds every filesystem rule (sandboxed listing, sorting, search,
//! selection, clipboard, trash) as plain Rust with unit tests. This module is
//! the controller: it owns the Slint component, maps the model to Slint
//! properties and never lets the UI touch the filesystem directly.

slint::include_modules!();

pub mod fsops;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ModelRc, VecModel};

use fsops::{Clipboard, ClipboardOp, Entry, EntryKind, FsError, Sandbox, Selection, SortKey};

/// The three views offered by the segmented control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Columns,
    Icons,
}

impl ViewMode {
    fn from_index(index: i32) -> Self {
        match index {
            1 => ViewMode::Columns,
            2 => ViewMode::Icons,
            _ => ViewMode::List,
        }
    }

    fn index(self) -> i32 {
        match self {
            ViewMode::List => 0,
            ViewMode::Columns => 1,
            ViewMode::Icons => 2,
        }
    }
}

/// Everything the window shows. Mutated through the controller only.
pub struct FilesState {
    pub sandbox: Sandbox,
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub selection: Selection,
    pub clipboard: Option<Clipboard>,
    pub sort: SortKey,
    pub ascending: bool,
    pub query: String,
    pub view: ViewMode,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
    pub status: String,
}

impl FilesState {
    /// State rooted at the sandbox root.
    pub fn root(sandbox: Sandbox) -> FilesState {
        let cwd = sandbox.root().to_path_buf();
        FilesState {
            cwd,
            sandbox,
            entries: Vec::new(),
            selection: Selection::default(),
            clipboard: None,
            sort: SortKey::Name,
            ascending: true,
            query: String::new(),
            view: ViewMode::List,
            back: Vec::new(),
            forward: Vec::new(),
            status: String::new(),
        }
    }

    fn open(&mut self, dir: &Path) -> Result<(), FsError> {
        self.navigate(dir, true)
    }

    /// Move to `dir`. `record` is false for Back/Forward, which manage the
    /// history stacks themselves (recording there used to clear `forward`
    /// immediately, so Forward never worked).
    fn navigate(&mut self, dir: &Path, record: bool) -> Result<(), FsError> {
        let dir = self.sandbox.resolve(dir)?;
        if record && dir != self.cwd {
            self.back.push(self.cwd.clone());
            self.forward.clear();
        }
        self.cwd = dir;
        self.selection.clear();
        self.reload()
    }

    fn reload(&mut self) -> Result<(), FsError> {
        let mut entries = fsops::list(&self.sandbox, &self.cwd)?;
        fsops::sort_entries(&mut entries, self.sort, self.ascending);
        let entries = fsops::filter(&entries, &self.query);
        let paths: Vec<PathBuf> = entries.iter().map(|entry| entry.path.clone()).collect();
        self.selection
            .retain(|path| paths.iter().any(|known| known == path));
        self.entries = entries;
        Ok(())
    }

    /// Go back one folder in history.
    pub fn go_back(&mut self) -> Result<(), FsError> {
        let Some(previous) = self.back.pop() else {
            return Ok(());
        };
        let current = self.cwd.clone();
        self.navigate(&previous, false)?;
        self.forward.push(current);
        Ok(())
    }

    /// Go forward one folder in history.
    pub fn go_forward(&mut self) -> Result<(), FsError> {
        let Some(next) = self.forward.pop() else {
            return Ok(());
        };
        let current = self.cwd.clone();
        self.navigate(&next, false)?;
        self.back.push(current);
        Ok(())
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selection.items().to_vec()
    }

    /// Status line for the current folder.
    fn folder_status(&self) -> String {
        format!(
            "{} · {} items",
            self.sandbox.display_path(&self.cwd),
            self.entries.len()
        )
    }
}

/// Sidebar targets and their folder names inside the sandbox.
fn sidebar_dir(id: &str) -> Option<&'static str> {
    match id {
        "home" => Some(""),
        "documents" => Some("Documents"),
        "pictures" => Some("Pictures"),
        "downloads" => Some("Downloads"),
        "trash" => Some(".trash"),
        _ => None,
    }
}

fn location_of(sandbox: &Sandbox, cwd: &Path) -> String {
    match sandbox.relative(cwd).to_string_lossy().as_ref() {
        "" => "home".to_string(),
        "Documents" => "documents".to_string(),
        "Pictures" => "pictures".to_string(),
        "Downloads" => "downloads".to_string(),
        ".trash" => "trash".to_string(),
        _ => "custom".to_string(),
    }
}

fn icon_for(entry: &Entry) -> IconKind {
    match entry.kind {
        EntryKind::Directory => IconKind::Folder,
        EntryKind::Symlink => IconKind::Globe,
        _ => {
            let extension = entry
                .path
                .extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            match extension.as_str() {
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => IconKind::Image,
                "zip" | "tar" | "gz" | "tgz" => IconKind::Archive,
                "md" | "txt" | "pdf" | "csv" => IconKind::Document,
                "sh" | "rs" | "py" | "js" | "json" | "toml" => IconKind::Terminal,
                _ => IconKind::File,
            }
        }
    }
}

fn kind_label(entry: &Entry) -> String {
    match entry.kind {
        EntryKind::Directory => "Folder".to_string(),
        EntryKind::Symlink => "Alias".to_string(),
        EntryKind::Other => "Other".to_string(),
        EntryKind::File => {
            let extension = entry
                .path
                .extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            match extension.as_str() {
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => "Image",
                "zip" | "tar" | "gz" | "tgz" => "Archive",
                "md" => "Markdown",
                "csv" | "json" | "toml" => "Data",
                "pdf" => "PDF",
                _ => "File",
            }
            .to_string()
        }
    }
}

fn build_rows(state: &FilesState) -> Vec<FileRow> {
    state
        .entries
        .iter()
        .map(|entry| FileRow {
            name: entry.name.clone().into(),
            size: if entry.kind.is_dir() {
                "—".into()
            } else {
                fsops::format_bytes(entry.size).into()
            },
            modified: fsops::format_date(entry.modified).into(),
            permissions: entry.permissions.clone().into(),
            kind_label: kind_label(entry).into(),
            icon: icon_for(entry),
            kind: if entry.kind.is_dir() { 1 } else { 0 },
            selected: state.selection.contains(&entry.path),
        })
        .collect()
}

fn free_space_label(root: &Path) -> String {
    match fsops::free_space(root) {
        Some(bytes) => format!("{} free", fsops::format_bytes(bytes)),
        None => "Free space unavailable".to_string(),
    }
}

/// Push the whole model into the Slint properties.
fn refresh(ui: &FilesApp, state: &FilesState) {
    let crumbs: Vec<slint::SharedString> = state
        .sandbox
        .crumbs(&state.cwd)
        .into_iter()
        .map(slint::SharedString::from)
        .collect();
    ui.set_rows(ModelRc::new(VecModel::from(build_rows(state))));
    ui.set_crumbs(ModelRc::new(VecModel::from(crumbs)));
    ui.set_path(state.sandbox.display_path(&state.cwd).into());
    ui.set_location(location_of(&state.sandbox, &state.cwd).into());
    ui.set_item_count(state.entries.len() as i32);
    ui.set_selected_count(state.selection.len() as i32);
    ui.set_view_mode(state.view.index());
    ui.set_can_go_back(!state.back.is_empty());
    ui.set_can_go_forward(!state.forward.is_empty());
    ui.set_status(state.status.clone().into());
    ui.set_clipboard(
        state
            .clipboard
            .as_ref()
            .filter(|clipboard| !clipboard.is_empty())
            .map(Clipboard::summary)
            .unwrap_or_default()
            .into(),
    );
    ui.set_free_space(free_space_label(state.sandbox.root()).into());
}

// --- actions -------------------------------------------------------------
//
// Shared by the public controller methods and the Slint callbacks so both go
// through exactly the same rules.

fn action_navigate(ui: &FilesApp, state: &mut FilesState, target: &str) {
    match sidebar_dir(target) {
        Some("") => {
            let root = state.sandbox.root().to_path_buf();
            match state.open(&root) {
                Ok(()) => state.status = format!("Home · {} items", state.entries.len()),
                Err(error) => state.status = error.to_string(),
            }
        }
        Some(name) => {
            let dir = state.sandbox.root().join(name);
            match state.open(&dir) {
                Ok(()) => state.status = format!("{name} · {} items", state.entries.len()),
                Err(error) => state.status = error.to_string(),
            }
        }
        None => state.status = format!("unknown location: {target}"),
    }
    refresh(ui, state);
}

fn action_open_row(ui: &FilesApp, state: &mut FilesState, index: i32) {
    let Some(entry) = usize::try_from(index)
        .ok()
        .and_then(|index| state.entries.get(index))
        .cloned()
    else {
        return;
    };
    if entry.kind == fsops::EntryKind::Directory {
        match state.open(&entry.path) {
            Ok(()) => state.status = state.folder_status(),
            Err(error) => state.status = error.to_string(),
        }
        refresh(ui, state);
    } else {
        state.status = format!("{} opens with the default app (P6)", entry.name);
        refresh(ui, state);
    }
}

/// Open the selected entry (toolbar "Open" and Enter).
fn action_open_selected(ui: &FilesApp, state: &mut FilesState) {
    let Some(path) = state.selection.items().first().cloned() else {
        state.status = "Select a folder to open".to_string();
        refresh(ui, state);
        return;
    };
    if path.is_dir() {
        match state.open(&path) {
            Ok(()) => state.status = state.folder_status(),
            Err(error) => state.status = error.to_string(),
        }
    } else {
        state.status = format!(
            "{} opens with the default app (P6)",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
    }
    refresh(ui, state);
}

fn action_select(ui: &FilesApp, state: &mut FilesState, index: i32) {
    if let Some(entry) = state.entries.get(index.max(0) as usize).cloned() {
        state.selection.toggle(&entry.path);
        state.status = if state.selection.is_empty() {
            state.folder_status()
        } else {
            format!("{} selected", state.selection.len())
        };
    }
    refresh(ui, state);
}

fn action_view(ui: &FilesApp, state: &mut FilesState, index: i32) {
    state.view = ViewMode::from_index(index);
    refresh(ui, state);
}

fn action_search(ui: &FilesApp, state: &mut FilesState, text: &str) {
    state.query = text.to_string();
    match state.reload() {
        Ok(()) => {
            state.status = if state.query.trim().is_empty() {
                state.folder_status()
            } else {
                format!("{} matches for \"{}\"", state.entries.len(), state.query.trim())
            }
        }
        Err(error) => state.status = error.to_string(),
    }
    refresh(ui, state);
}

fn action_back(ui: &FilesApp, state: &mut FilesState) {
    match state.go_back() {
        Ok(()) => state.status = state.folder_status(),
        Err(error) => state.status = error.to_string(),
    }
    refresh(ui, state);
}

fn action_forward(ui: &FilesApp, state: &mut FilesState) {
    match state.go_forward() {
        Ok(()) => state.status = state.folder_status(),
        Err(error) => state.status = error.to_string(),
    }
    refresh(ui, state);
}

fn action_up(ui: &FilesApp, state: &mut FilesState) {
    let parent = state.cwd.parent().map(Path::to_path_buf);
    match parent {
        Some(parent) if parent.starts_with(state.sandbox.root()) => match state.open(&parent) {
            Ok(()) => state.status = state.folder_status(),
            Err(error) => state.status = error.to_string(),
        },
        _ => state.status = "Already at the sandbox root".to_string(),
    }
    refresh(ui, state);
}

fn action_new_folder(ui: &FilesApp, state: &mut FilesState) {
    let cwd = state.cwd.clone();
    let mut name = "New Folder".to_string();
    let mut index = 2;
    while cwd.join(&name).exists() {
        name = format!("New Folder {index}");
        index += 1;
    }
    match fsops::create_folder(&state.sandbox, &cwd, &name) {
        Ok(path) => {
            let _ = state.reload();
            state.selection.select_only(&path);
            state.status = format!("Created {name}");
        }
        Err(error) => state.status = error.to_string(),
    }
    refresh(ui, state);
}

fn action_set_clipboard(ui: &FilesApp, state: &mut FilesState, op: ClipboardOp) {
    let items = state.selected_paths();
    if items.is_empty() {
        state.status = "Select a file first".to_string();
    } else {
        let clipboard = Clipboard::new(op, items);
        state.status = clipboard.summary();
        state.clipboard = Some(clipboard);
    }
    refresh(ui, state);
}

fn action_paste(ui: &FilesApp, state: &mut FilesState) {
    let Some(clipboard) = state.clipboard.clone() else {
        state.status = "Nothing on the clipboard".to_string();
        refresh(ui, state);
        return;
    };
    let dest = state.cwd.clone();
    let mut last = 0_u64;
    let result = fsops::paste(&state.sandbox, &clipboard, &dest, &mut |done, _| last = done);
    match result {
        Ok(written) => {
            let _ = state.reload();
            state.selection.clear();
            if clipboard.op == ClipboardOp::Cut {
                state.clipboard = None;
            }
            state.status = format!(
                "Pasted {} items ({} written)",
                written.len(),
                fsops::format_bytes(last)
            );
        }
        Err(error) => state.status = error.to_string(),
    }
    refresh(ui, state);
}

fn action_rename(ui: &FilesApp, state: &mut FilesState, new_name: &str) {
    let Some(path) = state.selection.items().first().cloned() else {
        state.status = "Select one item to rename".to_string();
        refresh(ui, state);
        return;
    };
    match fsops::rename(&state.sandbox, &path, new_name) {
        Ok(target) => {
            let _ = state.reload();
            state.selection.select_only(&target);
            state.status = format!(
                "Renamed to {}",
                target.file_name().unwrap_or_default().to_string_lossy()
            );
            ui.set_rename_text("".into());
        }
        Err(error) => state.status = error.to_string(),
    }
    refresh(ui, state);
}

fn action_trash(ui: &FilesApp, state: &mut FilesState) {
    let items = state.selected_paths();
    if items.is_empty() {
        state.status = "Select a file first".to_string();
        refresh(ui, state);
        return;
    }
    let mut trashed = 0;
    let mut failure: Option<String> = None;
    for item in &items {
        match fsops::trash(&state.sandbox, item) {
            Ok(_) => trashed += 1,
            Err(error) => failure = Some(error.to_string()),
        }
    }
    let _ = state.reload();
    state.selection.clear();
    state.status = match failure {
        Some(error) => error,
        None => format!("Moved {trashed} items to Trash"),
    };
    refresh(ui, state);
}

/// Owns the Files window and its state.
pub struct FilesController {
    ui: FilesApp,
    state: Rc<RefCell<FilesState>>,
}

impl FilesController {
    /// Open a sandbox. The root folder becomes "Home".
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let sandbox = Sandbox::new(root).map_err(|error| error.to_string())?;
        Self::with_sandbox(sandbox)
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Result<Self, String> {
        let ui = FilesApp::new().map_err(|error| error.to_string())?;
        let mut state = FilesState::root(sandbox);
        state.selection = Selection::new();
        state.status = "Ready".to_string();
        let state = Rc::new(RefCell::new(state));
        let controller = Self { ui, state };
        controller.wire();
        {
            let mut state = controller.state.borrow_mut();
            match state.reload() {
                Ok(()) => state.status = state.folder_status(),
                Err(error) => state.status = error.to_string(),
            }
        }
        refresh(&controller.ui, &controller.state.borrow());
        Ok(controller)
    }

    /// Synthetic tree used by `--render` and first-run demos. Never reads the
    /// user's own files.
    pub fn demo() -> Result<Self, String> {
        let root = std::env::temp_dir().join("aether-files-demo");
        let _ = std::fs::remove_dir_all(&root);
        build_demo_tree(&root).map_err(|error| error.to_string())?;
        let controller = Self::open(&root)?;
        controller.open_dir(&root.join("Documents"));
        {
            let mut state = controller.state.borrow_mut();
            // Show the multi-select highlight in the rendered evidence frame.
            if let Some(entry) = state.entries.iter().find(|entry| entry.name == "Notes.md") {
                let path = entry.path.clone();
                state.selection.select_only(&path);
                state.status = "1 selected · Copy, Move or Trash from the toolbar".to_string();
            }
        }
        refresh(&controller.ui, &controller.state.borrow());
        Ok(controller)
    }

    pub fn ui(&self) -> &FilesApp {
        &self.ui
    }

    pub fn state(&self) -> std::cell::Ref<'_, FilesState> {
        self.state.borrow()
    }

    // --- public actions (also used by the headless interaction test) ------

    pub fn navigate(&self, target: &str) {
        let mut state = self.state.borrow_mut();
        action_navigate(&self.ui, &mut state, target);
    }

    pub fn open_dir(&self, dir: &Path) {
        let mut state = self.state.borrow_mut();
        match state.open(dir) {
            Ok(()) => state.status = state.folder_status(),
            Err(error) => state.status = error.to_string(),
        }
        refresh(&self.ui, &state);
    }

    /// Open a row: folders navigate, files are handed to their app (P6).
    pub fn open_row(&self, index: i32) {
        let mut state = self.state.borrow_mut();
        action_open_row(&self.ui, &mut state, index);
    }

    /// Open the current selection (toolbar "Open").
    pub fn open_selected(&self) {
        let mut state = self.state.borrow_mut();
        action_open_selected(&self.ui, &mut state);
    }

    pub fn select_row(&self, index: i32) {
        let mut state = self.state.borrow_mut();
        action_select(&self.ui, &mut state, index);
    }

    pub fn go_up(&self) {
        let mut state = self.state.borrow_mut();
        action_up(&self.ui, &mut state);
    }

    pub fn go_back(&self) {
        let mut state = self.state.borrow_mut();
        action_back(&self.ui, &mut state);
    }

    pub fn go_forward(&self) {
        let mut state = self.state.borrow_mut();
        action_forward(&self.ui, &mut state);
    }

    pub fn set_view(&self, index: i32) {
        let mut state = self.state.borrow_mut();
        action_view(&self.ui, &mut state, index);
    }

    pub fn search(&self, query: &str) {
        let mut state = self.state.borrow_mut();
        action_search(&self.ui, &mut state, query);
    }

    pub fn new_folder(&self) {
        let mut state = self.state.borrow_mut();
        action_new_folder(&self.ui, &mut state);
    }

    pub fn copy_selected(&self) {
        let mut state = self.state.borrow_mut();
        action_set_clipboard(&self.ui, &mut state, ClipboardOp::Copy);
    }

    pub fn cut_selected(&self) {
        let mut state = self.state.borrow_mut();
        action_set_clipboard(&self.ui, &mut state, ClipboardOp::Cut);
    }

    pub fn paste(&self) {
        let mut state = self.state.borrow_mut();
        action_paste(&self.ui, &mut state);
    }

    pub fn rename_selected(&self, new_name: &str) {
        let mut state = self.state.borrow_mut();
        action_rename(&self.ui, &mut state, new_name);
    }

    pub fn trash_selected(&self) {
        let mut state = self.state.borrow_mut();
        action_trash(&self.ui, &mut state);
    }

    fn wire(&self) {
        // Every callback upgrades the weak handle first: a click after the
        // window is gone must not touch state.
        macro_rules! bind {
            ($setter:ident, |$($arg:ident),*| ($ui:ident, $st:ident) => $body:block) => {{
                let weak = self.ui.as_weak();
                let state = self.state.clone();
                self.ui.$setter(move |$($arg),*| {
                    if let Some($ui) = weak.upgrade() {
                        let mut $st = state.borrow_mut();
                        $body
                    }
                });
            }};
            ($setter:ident, ($ui:ident, $st:ident) => $body:block) => {{
                let weak = self.ui.as_weak();
                let state = self.state.clone();
                self.ui.$setter(move || {
                    if let Some($ui) = weak.upgrade() {
                        let mut $st = state.borrow_mut();
                        $body
                    }
                });
            }};
        }

        bind!(on_navigate, |target| (ui, state) => {
            action_navigate(&ui, &mut state, &target);
        });
        bind!(on_select_row, |index| (ui, state) => {
            action_select(&ui, &mut state, index);
        });
        bind!(on_open_row, |index| (ui, state) => {
            action_open_row(&ui, &mut state, index);
        });
        bind!(on_open_selected, (ui, state) => {
            action_open_selected(&ui, &mut state);
        });
        bind!(on_view_changed, |index| (ui, state) => {
            action_view(&ui, &mut state, index);
        });
        bind!(on_search_edited, |text| (ui, state) => {
            action_search(&ui, &mut state, &text);
        });
        bind!(on_go_back, (ui, state) => {
            action_back(&ui, &mut state);
        });
        bind!(on_go_forward, (ui, state) => {
            action_forward(&ui, &mut state);
        });
        bind!(on_go_up, (ui, state) => {
            action_up(&ui, &mut state);
        });
        bind!(on_new_folder, (ui, state) => {
            action_new_folder(&ui, &mut state);
        });
        bind!(on_copy_selected, (ui, state) => {
            action_set_clipboard(&ui, &mut state, ClipboardOp::Copy);
        });
        bind!(on_cut_selected, (ui, state) => {
            action_set_clipboard(&ui, &mut state, ClipboardOp::Cut);
        });
        bind!(on_paste_here, (ui, state) => {
            action_paste(&ui, &mut state);
        });
        bind!(on_rename_selected, |name| (ui, state) => {
            action_rename(&ui, &mut state, &name);
        });
        bind!(on_trash_selected, (ui, state) => {
            action_trash(&ui, &mut state);
        });
    }
}

/// Write a realistic demo tree for the screenshot render.
fn build_demo_tree(root: &Path) -> Result<(), std::io::Error> {
    use std::fs;

    let documents = root.join("Documents");
    let pictures = root.join("Pictures");
    let downloads = root.join("Downloads");
    let projects = root.join("Projects");
    for dir in [&documents, &pictures, &downloads, &projects] {
        fs::create_dir_all(dir)?;
    }
    let payload = |bytes: usize| vec![b'a'; bytes];
    fs::write(documents.join("Design brief.pdf"), payload(48 * 1024))?;
    fs::write(
        documents.join("Notes.md"),
        "# Notes\n\n- ship Files app\n- verify render\n",
    )?;
    fs::write(documents.join("Quarterly report.txt"), payload(12 * 1024))?;
    fs::write(documents.join("Budget.csv"), payload(3 * 1024))?;
    fs::write(documents.join("Meeting notes.md"), payload(2 * 1024))?;
    fs::write(documents.join("Roadmap.txt"), payload(9 * 1024))?;
    fs::write(documents.join("Archive 2025.zip"), payload(64 * 1024))?;
    fs::write(documents.join("Photo scan.jpg"), payload(220 * 1024))?;
    fs::write(pictures.join("Wallpaper.png"), payload(180 * 1024))?;
    fs::write(pictures.join("Screenshot 2026-11-23.png"), payload(96 * 1024))?;
    fs::write(downloads.join("aether-sdk.tar.gz"), payload(30 * 1024))?;
    fs::write(projects.join("README.md"), "# Projects\n")?;
    Ok(())
}


#[cfg(test)]
mod navigation_tests {
    use super::*;

    fn sandbox() -> (PathBuf, FilesState) {
        let root = std::env::temp_dir().join(format!(
            "aether-files-nav-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("A")).unwrap();
        std::fs::create_dir_all(root.join("B")).unwrap();
        let sandbox = Sandbox::new(&root).unwrap();
        let root = sandbox.root().to_path_buf();
        (root.clone(), FilesState::root(sandbox))
    }

    #[test]
    fn back_and_forward_track_history_in_both_directions() {
        let (root, mut state) = sandbox();
        state.open(&root.join("A")).unwrap();
        state.open(&root.join("B")).unwrap();
        assert_eq!(state.cwd, root.join("B"));
        assert_eq!(state.back.len(), 2);

        state.go_back().unwrap();
        assert_eq!(state.cwd, root.join("A"));
        assert_eq!(state.forward.len(), 1, "forward is available");

        state.go_back().unwrap();
        assert_eq!(state.cwd, state.sandbox.root());

        state.go_forward().unwrap();
        assert_eq!(state.cwd, root.join("A"), "forward returns to A");
        state.go_forward().unwrap();
        assert_eq!(state.cwd, root.join("B"), "forward returns to B");
        assert!(state.back.len() == 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn opening_a_new_folder_clears_the_forward_stack() {
        let (root, mut state) = sandbox();
        state.open(&root.join("A")).unwrap();
        state.open(&root.join("B")).unwrap();
        state.go_back().unwrap();
        state.open(&root.join("A")).ok();
        state.open(&root.join("B")).unwrap();
        assert!(state.forward.is_empty(), "forward clears on a new navigation");
        let _ = std::fs::remove_dir_all(root);
    }
}
