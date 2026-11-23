//! AetherOS Archive (P4-T17, DA-AC-08).
//!
//! [`archive`] owns format detection, listing, extraction with traversal
//! protection, tar.gz creation and rebuild-based deletion. This module maps
//! that model to the Slint window and keeps every filesystem call out of the
//! view layer.

slint::include_modules!();

pub mod archive;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ModelRc, VecModel};

use archive::{Entry, Format};

/// Everything the window shows.
pub struct ArchiveState {
    pub archive: Option<PathBuf>,
    pub format: Option<Format>,
    pub entries: Vec<Entry>,
    pub selection: Vec<String>,
    pub progress: f32,
    pub progress_text: String,
    pub status: String,
    pub error: String,
    pub folder: String,
}

impl ArchiveState {
    fn summary(&self) -> String {
        let Some(format) = self.format else {
            return "No archive open".to_string();
        };
        let bytes: u64 = self.entries.iter().filter(|entry| !entry.is_dir).map(|entry| entry.size).sum();
        format!(
            "{} · {} entries · {}",
            format.label(),
            self.entries.len(),
            archive::format_bytes(bytes)
        )
    }
}

fn extraction_dir(archive_path: &Path) -> PathBuf {
    let parent = archive_path.parent().unwrap_or_else(|| Path::new("."));
    let name = archive_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name
        .trim_end_matches(".tar.gz")
        .trim_end_matches(".tgz")
        .trim_end_matches(".tar")
        .trim_end_matches(".zip");
    parent.join(format!("{stem}-extracted"))
}

fn build_rows(state: &ArchiveState) -> Vec<ArchiveRow> {
    state
        .entries
        .iter()
        .map(|entry| ArchiveRow {
            name: entry.path.clone().into(),
            size: entry.size_label().into(),
            kind: entry.kind_label().into(),
            modified: archive::format_time(entry.modified).into(),
            is_dir: entry.is_dir,
            selected: state.selection.iter().any(|name| name == &entry.path),
        })
        .collect()
}

fn refresh(ui: &ArchiveApp, state: &ArchiveState) {
    ui.set_entries(ModelRc::new(VecModel::from(build_rows(state))));
    ui.set_archive_path(
        state
            .archive
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_format_label(
        state
            .format
            .map(Format::label)
            .unwrap_or("—")
            .into(),
    );
    ui.set_entry_count(state.entries.len() as i32);
    ui.set_selected_count(state.selection.len() as i32);
    ui.set_status(state.status.clone().into());
    ui.set_error(state.error.clone().into());
    ui.set_progress(state.progress);
    ui.set_progress_text(state.progress_text.clone().into());
}

// --- actions -------------------------------------------------------------

fn action_open(ui: &ArchiveApp, state: &mut ArchiveState, path: &str) {
    let trimmed = path.trim().trim_matches('"');
    if trimmed.is_empty() {
        state.error = "Enter an archive path first".to_string();
        refresh(ui, state);
        return;
    }
    let path = PathBuf::from(trimmed);
    match archive::list(&path) {
        Ok((format, entries)) => {
            state.archive = Some(path.clone());
            state.format = Some(format);
            state.entries = entries;
            state.selection.clear();
            state.error.clear();
            state.progress = 0.0;
            state.progress_text = format!("{} ready to extract", format.label());
            state.status = state.summary();
            ui.set_open_path(path.display().to_string().into());
        }
        Err(error) => {
            state.error = error.to_string();
            state.status = "Could not open the archive".to_string();
        }
    }
    refresh(ui, state);
}

fn action_select(ui: &ArchiveApp, state: &mut ArchiveState, index: i32) {
    if let Some(entry) = state.entries.get(index.max(0) as usize) {
        let name = entry.path.clone();
        if let Some(position) = state.selection.iter().position(|item| item == &name) {
            state.selection.remove(position);
        } else {
            state.selection.push(name);
        }
        state.status = format!("{} selected", state.selection.len());
    }
    refresh(ui, state);
}

fn action_extract(ui: &ArchiveApp, state: &mut ArchiveState, only_selected: bool) {
    let Some(archive_path) = state.archive.clone() else {
        state.error = "Open an archive first".to_string();
        refresh(ui, state);
        return;
    };
    if only_selected && state.selection.is_empty() {
        state.error = "Select at least one entry first".to_string();
        refresh(ui, state);
        return;
    }
    let dest = extraction_dir(&archive_path);
    let selected = state.selection.clone();
    let label = if only_selected { "selected" } else { "all" };
    state.progress = 0.0;
    state.progress_text = format!("Extracting {label}…");

    let result = {
        let mut progress = |done: u64, total: u64| {
            let fraction = if total == 0 {
                1.0
            } else {
                (done as f32 / total as f32).clamp(0.0, 1.0)
            };
            ui.set_progress(fraction);
            ui.set_progress_text(
                format!(
                    "{} of {} · {label}",
                    archive::format_bytes(done),
                    archive::format_bytes(total)
                )
                .into(),
            );
        };
        if only_selected {
            archive::extract_selected(&archive_path, &dest, &selected, &mut progress)
        } else {
            archive::extract_all(&archive_path, &dest, &mut progress)
        }
    };

    match result {
        Ok(count) => {
            state.progress = 1.0;
            state.progress_text = format!("{count} entries extracted · 100%");
            state.status = format!("Extracted {count} entries into {}", dest.display());
            state.error.clear();
        }
        Err(error) => {
            state.progress = 0.0;
            state.progress_text = "Extraction stopped".to_string();
            state.error = error.to_string();
            state.status = "Extraction failed".to_string();
        }
    }
    refresh(ui, state);
}

fn action_delete(ui: &ArchiveApp, state: &mut ArchiveState) {
    let Some(archive_path) = state.archive.clone() else {
        state.error = "Open an archive first".to_string();
        refresh(ui, state);
        return;
    };
    if state.selection.is_empty() {
        state.error = "Select at least one entry first".to_string();
        refresh(ui, state);
        return;
    }
    let doomed = state.selection.clone();
    let result = archive::delete_entries(&archive_path, &doomed, &mut |_, _| {});
    match result {
        Ok(remaining) => match archive::list(&archive_path) {
            Ok((format, entries)) => {
                state.format = Some(format);
                state.entries = entries;
                state.selection.clear();
                state.error.clear();
                state.status = format!(
                    "Deleted {} entries · {remaining} left",
                    doomed.len()
                );
            }
            Err(error) => state.error = error.to_string(),
        },
        Err(error) => {
            state.error = error.to_string();
            state.status = "Delete failed".to_string();
        }
    }
    refresh(ui, state);
}

fn action_create(ui: &ArchiveApp, state: &mut ArchiveState, folder: &str) {
    let trimmed = folder.trim().trim_matches('"');
    if trimmed.is_empty() {
        state.error = "Enter a folder to archive".to_string();
        refresh(ui, state);
        return;
    }
    let source = PathBuf::from(trimmed);
    if !source.is_dir() {
        state.error = format!("{} is not a folder", source.display());
        refresh(ui, state);
        return;
    }
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());
    let parent = source.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let target = parent.join(format!("{name}.tar.gz"));
    match archive::create_tar_gz(&target, &[source.clone()], &mut |_, _| {}) {
        Ok(bytes) => {
            state.folder = source.display().to_string();
            action_open(ui, state, &target.display().to_string());
            state.status = format!(
                "Created {} ({})",
                target.display(),
                archive::format_bytes(bytes)
            );
            state.progress_text = format!("{} written", archive::format_bytes(bytes));
        }
        Err(error) => {
            state.error = error.to_string();
            state.status = "Could not create the archive".to_string();
        }
    }
    refresh(ui, state);
}

/// Owns the Archive window and its state.
pub struct ArchiveController {
    ui: ArchiveApp,
    state: Rc<RefCell<ArchiveState>>,
}

impl ArchiveController {
    pub fn new() -> Result<Self, String> {
        let ui = ArchiveApp::new().map_err(|error| error.to_string())?;
        let state = ArchiveState {
            archive: None,
            format: None,
            entries: Vec::new(),
            selection: Vec::new(),
            progress: 0.0,
            progress_text: "Idle".to_string(),
            status: "Open an archive to begin".to_string(),
            error: String::new(),
            folder: String::new(),
        };
        let state = Rc::new(RefCell::new(state));
        let controller = Self { ui, state };
        controller.wire();
        refresh(&controller.ui, &controller.state.borrow());
        Ok(controller)
    }

    /// Build a real archive in a temporary folder, open it and extract it so
    /// the rendered frame shows genuine entries and a finished progress bar.
    pub fn demo() -> Result<Self, String> {
        let root = std::env::temp_dir().join("aether-archive-demo");
        let _ = std::fs::remove_dir_all(&root);
        build_demo_tree(&root).map_err(|error| error.to_string())?;
        let controller = Self::new()?;
        let source = root.join("aether-kit");
        let archive_path = root.join("aether-kit.tar.gz");
        archive::create_tar_gz(&archive_path, &[source.clone(), root.join("release-notes.md")], &mut |_, _| {})
            .map_err(|error| error.to_string())?;
        controller.open(&archive_path.display().to_string());
        {
            let mut state = controller.state.borrow_mut();
            state.folder = source.display().to_string();
        }
        controller.ui.set_folder_path(source.display().to_string().into());
        controller.extract_all();
        {
            let mut state = controller.state.borrow_mut();
            let first = state
                .entries
                .iter()
                .find(|entry| entry.path == "aether-kit/aether.slint")
                .map(|entry| entry.path.clone());
            if let Some(path) = first {
                state.selection.push(path);
            }
            state.status = format!(
                "{} · extracted into aether-kit-extracted",
                state.summary()
            );
        }
        refresh(&controller.ui, &controller.state.borrow());
        Ok(controller)
    }

    pub fn ui(&self) -> &ArchiveApp {
        &self.ui
    }

    pub fn state(&self) -> std::cell::Ref<'_, ArchiveState> {
        self.state.borrow()
    }

    // --- public actions (also used by the headless interaction test) ------

    pub fn open(&self, path: &str) {
        let mut state = self.state.borrow_mut();
        action_open(&self.ui, &mut state, path);
    }

    pub fn select_entry(&self, index: i32) {
        let mut state = self.state.borrow_mut();
        action_select(&self.ui, &mut state, index);
    }

    pub fn extract_all(&self) {
        let mut state = self.state.borrow_mut();
        action_extract(&self.ui, &mut state, false);
    }

    pub fn extract_selected(&self) {
        let mut state = self.state.borrow_mut();
        action_extract(&self.ui, &mut state, true);
    }

    pub fn delete_selected(&self) {
        let mut state = self.state.borrow_mut();
        action_delete(&self.ui, &mut state);
    }

    pub fn create(&self, folder: &str) {
        let mut state = self.state.borrow_mut();
        action_create(&self.ui, &mut state, folder);
    }

    fn wire(&self) {
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

        bind!(on_open_archive, |path| (ui, state) => {
            action_open(&ui, &mut state, &path);
        });
        bind!(on_select_entry, |index| (ui, state) => {
            action_select(&ui, &mut state, index);
        });
        bind!(on_extract_all_here, (ui, state) => {
            action_extract(&ui, &mut state, false);
        });
        bind!(on_extract_selected_here, (ui, state) => {
            action_extract(&ui, &mut state, true);
        });
        bind!(on_delete_selected, (ui, state) => {
            action_delete(&ui, &mut state);
        });
        bind!(on_create_archive, |folder| (ui, state) => {
            action_create(&ui, &mut state, &folder);
        });
    }
}

/// A small source tree plus one file outside the folder, archived at runtime.
fn build_demo_tree(root: &Path) -> Result<(), std::io::Error> {
    use std::fs;

    let kit = root.join("aether-kit");
    fs::create_dir_all(kit.join("icons"))?;
    fs::create_dir_all(kit.join("scripts"))?;
    fs::write(kit.join("aether.slint"), "// AetherOS widget kit\n".repeat(40))?;
    fs::write(kit.join("tokens.md"), "# Tokens\n\n4 px grid, dark glass.\n".repeat(30))?;
    fs::write(kit.join("icons/app.svg"), "<svg><!-- icon --></svg>".repeat(20))?;
    fs::write(kit.join("scripts/build.sh"), "#!/bin/sh\ncargo build\n")?;
    fs::write(root.join("release-notes.md"), "# AetherOS 0.4\n\nArchive app.\n".repeat(20))?;
    Ok(())
}
