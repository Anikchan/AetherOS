//! Filesystem operations for the Files app (P4-T08, F1 SHL-12).
//!
//! Every path the UI hands to this module is resolved against a [`Sandbox`]
//! root and canonicalised before use, so `..` traversal and symlinks that
//! point outside the root are rejected. Nothing here knows about Slint: the
//! controller in `lib.rs` maps these types to Slint models, and the unit tests
//! below drive the real filesystem through temporary directories.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Errors surfaced to the status bar.
#[derive(Debug)]
pub enum FsError {
    /// The path resolves outside the sandbox root.
    OutsideSandbox(PathBuf),
    /// A name or destination that would corrupt the operation.
    InvalidName(String),
    /// The target already exists and would be overwritten.
    AlreadyExists(String),
    /// A filesystem call failed.
    Io { path: PathBuf, source: io::Error },
}

impl FsError {
    fn io(path: &Path, source: io::Error) -> Self {
        FsError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    /// True when the underlying failure was a permission problem.
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, FsError::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied)
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::OutsideSandbox(path) => {
                write!(f, "{} is outside the sandbox", path.display())
            }
            FsError::InvalidName(name) => write!(f, "invalid name: {name}"),
            FsError::AlreadyExists(name) => write!(f, "already exists: {name}"),
            FsError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for FsError {}

/// What a directory entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl EntryKind {
    pub fn is_dir(self) -> bool {
        self == EntryKind::Directory
    }

    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Directory => "Folder",
            EntryKind::File => "File",
            EntryKind::Symlink => "Link",
            EntryKind::Other => "Other",
        }
    }
}

/// One directory entry as the UI needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// `rwxr-xr-x`-style summary of the permission bits.
    pub permissions: String,
}

/// Sort keys exposed by the file list header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Date,
}

impl SortKey {
    pub fn from_index(index: i32) -> Self {
        match index {
            1 => SortKey::Size,
            2 => SortKey::Date,
            _ => SortKey::Name,
        }
    }
}

/// A root directory every operation is confined to.
#[derive(Debug, Clone)]
pub struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// Create (if needed) and canonicalise a sandbox root.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, FsError> {
        let root = root.as_ref();
        fs::create_dir_all(root).map_err(|e| FsError::io(root, e))?;
        let root = root.canonicalize().map_err(|e| FsError::io(root, e))?;
        Ok(Sandbox { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Canonicalise `path` (absolute or root-relative) and refuse anything that
    /// ends up outside the root.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, FsError> {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let canonical = candidate
            .canonicalize()
            .map_err(|e| FsError::io(&candidate, e))?;
        if canonical.starts_with(&self.root) {
            Ok(canonical)
        } else {
            Err(FsError::OutsideSandbox(candidate))
        }
    }

    /// `Home / Documents / Notes` for the breadcrumb bar.
    pub fn display_path(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let mut parts = vec!["Home".to_string()];
        for part in relative.components() {
            parts.push(part.as_os_str().to_string_lossy().into_owned());
        }
        parts.join(" / ")
    }

    /// Breadcrumb segments (root first).
    pub fn crumbs(&self, path: &Path) -> Vec<String> {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let mut parts = vec!["Home".to_string()];
        for part in relative.components() {
            parts.push(part.as_os_str().to_string_lossy().into_owned());
        }
        parts
    }

    /// Sandbox-relative path, used by the navigation history.
    pub fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// List a directory inside the sandbox.
pub fn list(sandbox: &Sandbox, dir: &Path) -> Result<Vec<Entry>, FsError> {
    let dir = sandbox.resolve(dir)?;
    let read = fs::read_dir(&dir).map_err(|e| FsError::io(&dir, e))?;
    let mut entries = Vec::new();
    for item in read {
        let item = item.map_err(|e| FsError::io(&dir, e))?;
        let path = item.path();
        let file_type = match item.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let meta = item.metadata().ok();
        let size = if kind.is_dir() {
            0
        } else {
            meta.as_ref().map(|m| m.len()).unwrap_or(0)
        };
        entries.push(Entry {
            path,
            name: item.file_name().to_string_lossy().into_owned(),
            kind,
            size,
            modified: meta.as_ref().and_then(|m| m.modified().ok()),
            permissions: meta
                .as_ref()
                .map(permission_summary)
                .unwrap_or_else(|| "---------".to_string()),
        });
    }
    Ok(entries)
}

/// Folders first, then the selected key; ties break on the name.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, ascending: bool) {
    entries.sort_by(|a, b| {
        let folders = b.kind.is_dir().cmp(&a.kind.is_dir());
        if folders != std::cmp::Ordering::Equal {
            return folders;
        }
        let order = match key {
            SortKey::Name => name_cmp(&a.name, &b.name),
            SortKey::Size => a
                .size
                .cmp(&b.size)
                .then_with(|| name_cmp(&a.name, &b.name)),
            SortKey::Date => a
                .modified
                .cmp(&b.modified)
                .then_with(|| name_cmp(&a.name, &b.name)),
        };
        if ascending {
            order
        } else {
            order.reverse()
        }
    });
}

fn name_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Case-insensitive substring filter over names.
pub fn filter(entries: &[Entry], query: &str) -> Vec<Entry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|entry| entry.name.to_lowercase().contains(&needle))
        .cloned()
        .collect()
}

/// Ordered multi-selection. The order is the click order, so the UI can show
/// "1 of 3" without re-sorting.
#[derive(Debug, Default, Clone)]
pub struct Selection {
    items: Vec<PathBuf>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.items.iter().any(|item| item == path)
    }

    /// Single click without modifiers.
    pub fn select_only(&mut self, path: &Path) {
        self.items.clear();
        self.items.push(path.to_path_buf());
    }

    /// Toggle for click-with-modifier; returns the new state.
    pub fn toggle(&mut self, path: &Path) -> bool {
        if self.contains(path) {
            self.items.retain(|item| item != path);
            false
        } else {
            self.items.push(path.to_path_buf());
            true
        }
    }

    pub fn set(&mut self, path: &Path, selected: bool) {
        if selected {
            if !self.contains(path) {
                self.items.push(path.to_path_buf());
            }
        } else {
            self.items.retain(|item| item != path);
        }
    }

    /// Drop selections that no longer exist in the current listing.
    pub fn retain(&mut self, mut keep: impl FnMut(&Path) -> bool) {
        self.items.retain(|item| keep(item));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOp {
    Copy,
    Cut,
}

impl ClipboardOp {
    pub fn verb(self) -> &'static str {
        match self {
            ClipboardOp::Copy => "copied",
            ClipboardOp::Cut => "cut",
        }
    }
}

/// Copy/cut buffer.
#[derive(Debug, Clone)]
pub struct Clipboard {
    pub op: ClipboardOp,
    pub items: Vec<PathBuf>,
}

impl Clipboard {
    pub fn new(op: ClipboardOp, items: Vec<PathBuf>) -> Self {
        Clipboard { op, items }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn summary(&self) -> String {
        let noun = if self.items.len() == 1 { "item" } else { "items" };
        format!("{} {noun} {}", self.items.len(), self.op.verb())
    }
}

/// Create a folder; `name` may not contain separators or traversal segments.
pub fn create_folder(sandbox: &Sandbox, parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let parent = sandbox.resolve(parent)?;
    validate_name(name)?;
    let target = parent.join(name);
    fs::create_dir(&target).map_err(|e| FsError::io(&target, e))?;
    Ok(target)
}

/// Rename in place (same parent).
pub fn rename(sandbox: &Sandbox, path: &Path, new_name: &str) -> Result<PathBuf, FsError> {
    let path = sandbox.resolve(path)?;
    validate_name(new_name)?;
    let parent = path
        .parent()
        .ok_or_else(|| FsError::InvalidName("no parent folder".into()))?;
    let target = parent.join(new_name);
    if target == path {
        return Ok(path);
    }
    // fs::rename would silently replace an existing entry on Unix, which loses
    // data without warning. Refuse instead and let the user pick another name.
    if fs::symlink_metadata(&target).is_ok() {
        return Err(FsError::AlreadyExists(target.display().to_string()));
    }
    fs::rename(&path, &target).map_err(|e| FsError::io(&path, e))?;
    Ok(target)
}

/// Copy one entry into `dest_dir`, returning the new path. Name collisions get
/// a ` (2)` suffix. `progress` receives `(bytes_done, bytes_total)`.
pub fn copy_entry(
    sandbox: &Sandbox,
    src: &Path,
    dest_dir: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<PathBuf, FsError> {
    let src = sandbox.resolve(src)?;
    let dest_dir = sandbox.resolve(dest_dir)?;
    if src.is_dir() && (dest_dir == src || dest_dir.starts_with(&src)) {
        return Err(FsError::InvalidName(
            "cannot copy a folder into itself".into(),
        ));
    }
    let name = src
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| FsError::InvalidName("source has no name".into()))?;
    let target = unique_path(&dest_dir, &name);
    let total = tree_size(&src).unwrap_or(0);
    let mut done = 0;
    copy_recursive(&src, &target, &mut done, total, progress)?;
    Ok(target)
}

/// Move one entry into `dest_dir`. A move into the current parent is a no-op.
pub fn move_entry(sandbox: &Sandbox, src: &Path, dest_dir: &Path) -> Result<PathBuf, FsError> {
    let src = sandbox.resolve(src)?;
    let dest_dir = sandbox.resolve(dest_dir)?;
    if src.parent() == Some(dest_dir.as_path()) {
        return Ok(src);
    }
    if src.is_dir() && (dest_dir == src || dest_dir.starts_with(&src)) {
        return Err(FsError::InvalidName(
            "cannot move a folder into itself".into(),
        ));
    }
    let name = src
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| FsError::InvalidName("source has no name".into()))?;
    let target = unique_path(&dest_dir, &name);
    if fs::rename(&src, &target).is_err() {
        // Cross-device or restricted move: copy, then remove the source.
        let total = tree_size(&src).unwrap_or(0);
        let mut done = 0;
        copy_recursive(&src, &target, &mut done, total, &mut |_, _| {})?;
        remove_any(&src)?;
    }
    Ok(target)
}

/// Paste a clipboard into `dest_dir`; returns the written paths.
pub fn paste(
    sandbox: &Sandbox,
    clipboard: &Clipboard,
    dest_dir: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<PathBuf>, FsError> {
    let mut written = Vec::new();
    for item in &clipboard.items {
        match clipboard.op {
            ClipboardOp::Copy => written.push(copy_entry(sandbox, item, dest_dir, progress)?),
            ClipboardOp::Cut => written.push(move_entry(sandbox, item, dest_dir)?),
        }
    }
    Ok(written)
}

/// Move an entry into `<root>/.trash`, creating the trash folder on demand.
pub fn trash(sandbox: &Sandbox, path: &Path) -> Result<PathBuf, FsError> {
    let path = sandbox.resolve(path)?;
    if path == sandbox.root() {
        return Err(FsError::InvalidName("cannot trash the sandbox root".into()));
    }
    let trash_dir = sandbox.root().join(".trash");
    fs::create_dir_all(&trash_dir).map_err(|e| FsError::io(&trash_dir, e))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| FsError::InvalidName("source has no name".into()))?;
    let target = unique_path(&trash_dir, &name);
    if fs::rename(&path, &target).is_err() {
        let total = tree_size(&path).unwrap_or(0);
        let mut done = 0;
        copy_recursive(&path, &target, &mut done, total, &mut |_, _| {})?;
        remove_any(&path)?;
    }
    Ok(target)
}

/// Human-readable size, base 1024.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// `YYYY-MM-DD HH:MM` (UTC) or an em dash when the time is unknown.
pub fn format_date(time: Option<SystemTime>) -> String {
    let Some(time) = time else {
        return "—".to_string();
    };
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60
    )
}

/// Free space of the volume holding `path`, via POSIX `df -k -P`.
pub fn free_space(path: &Path) -> Option<u64> {
    let output = std::process::Command::new("df")
        .arg("-k")
        .arg("-P")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1)?;
    let available: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(available * 1024)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

fn permission_summary(meta: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        let bits = [
            (0o400, 'r'),
            (0o200, 'w'),
            (0o100, 'x'),
            (0o040, 'r'),
            (0o020, 'w'),
            (0o010, 'x'),
            (0o004, 'r'),
            (0o002, 'w'),
            (0o001, 'x'),
        ];
        bits.iter()
            .map(|(bit, ch)| if mode & bit != 0 { *ch } else { '-' })
            .collect()
    }
    #[cfg(not(unix))]
    {
        if meta.permissions().readonly() {
            "r--r--r--".to_string()
        } else {
            "rw-rw-rw-".to_string()
        }
    }
}

fn validate_name(name: &str) -> Result<(), FsError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(FsError::InvalidName(name.to_string()));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
        return Err(FsError::InvalidName(name.to_string()));
    }
    Ok(())
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let extension = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    for index in 2..1000 {
        let candidate = dir.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem} ({}){extension}", std::process::id()))
}

fn tree_size(path: &Path) -> Result<u64, FsError> {
    let meta = fs::symlink_metadata(path).map_err(|e| FsError::io(path, e))?;
    if meta.is_dir() {
        let mut total = 0;
        for entry in fs::read_dir(path).map_err(|e| FsError::io(path, e))? {
            let entry = entry.map_err(|e| FsError::io(path, e))?;
            total += tree_size(&entry.path()).unwrap_or(0);
        }
        Ok(total)
    } else {
        Ok(meta.len())
    }
}

fn copy_recursive(
    src: &Path,
    dst: &Path,
    done: &mut u64,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), FsError> {
    let meta = fs::symlink_metadata(src).map_err(|e| FsError::io(src, e))?;
    if meta.is_dir() {
        fs::create_dir_all(dst).map_err(|e| FsError::io(dst, e))?;
        for entry in fs::read_dir(src).map_err(|e| FsError::io(src, e))? {
            let entry = entry.map_err(|e| FsError::io(src, e))?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()), done, total, progress)?;
        }
    } else if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = fs::read_link(src).map_err(|e| FsError::io(src, e))?;
            let _ = fs::remove_file(dst);
            std::os::unix::fs::symlink(target, dst).map_err(|e| FsError::io(dst, e))?;
        }
    } else {
        let bytes = fs::copy(src, dst).map_err(|e| FsError::io(src, e))?;
        *done += bytes;
        progress(*done, total);
    }
    Ok(())
}

fn remove_any(path: &Path) -> Result<(), FsError> {
    let meta = fs::symlink_metadata(path).map_err(|e| FsError::io(path, e))?;
    if meta.is_dir() {
        fs::remove_dir_all(path).map_err(|e| FsError::io(path, e))
    } else {
        fs::remove_file(path).map_err(|e| FsError::io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(tag: &str) -> PathBuf {
        static TEST_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aether-files-{tag}-{}-{stamp}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture() -> (PathBuf, Sandbox) {
        let root = temp_dir("fixture");
        fs::create_dir_all(root.join("Documents/Reports")).unwrap();
        fs::create_dir_all(root.join("Pictures")).unwrap();
        fs::write(root.join("Documents/notes.md"), "# notes\n").unwrap();
        fs::write(root.join("Documents/Reports/annual.txt"), "annual".repeat(100)).unwrap();
        fs::write(root.join("Pictures/wallpaper.png"), vec![0u8; 4096]).unwrap();
        fs::write(root.join("readme.txt"), "hello").unwrap();
        let sandbox = Sandbox::new(&root).unwrap();
        // macOS resolves /var to /private/var, so compare canonical paths.
        let root = sandbox.root().to_path_buf();
        (root, sandbox)
    }

    #[test]
    fn lists_a_fixture_tree_with_metadata() {
        let (_root, sandbox) = fixture();
        let entries = list(&sandbox, sandbox.root()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names.len(), 3, "Documents, Pictures, readme.txt: {names:?}");
        assert!(entries.iter().any(|e| e.name == "Documents" && e.kind.is_dir()));
        let readme = entries.iter().find(|e| e.name == "readme.txt").unwrap();
        assert_eq!(readme.kind, EntryKind::File);
        assert_eq!(readme.size, 5);
        assert_eq!(readme.permissions.len(), 9);
        assert!(readme.modified.is_some());
    }

    #[test]
    fn sorts_folders_first_then_by_key() {
        let (root, sandbox) = fixture();
        fs::write(root.join("a-small.txt"), "x").unwrap();
        fs::write(root.join("z-large.txt"), vec![b'y'; 8192]).unwrap();

        let mut entries = list(&sandbox, sandbox.root()).unwrap();
        sort_entries(&mut entries, SortKey::Name, true);
        assert!(entries[0].kind.is_dir() && entries[1].kind.is_dir());
        let files: Vec<_> = entries.iter().filter(|e| !e.kind.is_dir()).map(|e| e.name.clone()).collect();
        assert_eq!(files, vec!["a-small.txt", "readme.txt", "z-large.txt"]);

        sort_entries(&mut entries, SortKey::Size, false);
        let files: Vec<_> = entries.iter().filter(|e| !e.kind.is_dir()).map(|e| e.name.clone()).collect();
        assert_eq!(files, vec!["z-large.txt", "readme.txt", "a-small.txt"]);
        assert!(entries[0].kind.is_dir(), "folders still come first");

        let mut entries = list(&sandbox, sandbox.root()).unwrap();
        sort_entries(&mut entries, SortKey::Date, true);
        assert_eq!(entries.iter().filter(|e| e.kind.is_dir()).count(), 2);
    }

    #[test]
    fn search_filters_by_name_case_insensitively() {
        let (root, sandbox) = fixture();
        let entries = list(&sandbox, &root.join("Documents")).unwrap();
        let hits = filter(&entries, "NOTES");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "notes.md");
        assert_eq!(filter(&entries, "  ").len(), entries.len());
        assert!(filter(&entries, "nope").is_empty());

        let root_entries = list(&sandbox, sandbox.root()).unwrap();
        assert_eq!(filter(&root_entries, "pic").len(), 1);
    }

    #[test]
    fn selection_supports_multi_select() {
        let root = temp_dir("selection");
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();

        let mut selection = Selection::new();
        selection.select_only(&a);
        assert_eq!(selection.len(), 1);
        assert!(selection.toggle(&b));
        assert_eq!(selection.len(), 2);
        assert!(!selection.toggle(&a));
        assert_eq!(selection.items(), &[b.clone()]);
        selection.set(&a, true);
        selection.retain(|path| path != b);
        assert_eq!(selection.items(), &[a]);
    }

    #[test]
    fn copies_recursively_with_progress_and_unique_names() {
        let (root, sandbox) = fixture();
        let calls = std::cell::Cell::new(0);
        let dest = root.join("Pictures");
        let copied = copy_entry(&sandbox, &root.join("Documents"), &dest, &mut |done, total| {
            calls.set(calls.get() + 1);
            assert!(done <= total);
        })
        .unwrap();
        assert_eq!(copied, dest.join("Documents"));
        assert!(copied.join("Reports/annual.txt").exists());
        assert!(calls.get() >= 2, "progress is reported per file");

        let again = copy_entry(&sandbox, &root.join("Documents"), &dest, &mut |_, _| {}).unwrap();
        assert_eq!(again, dest.join("Documents (2)"));
        assert!(again.join("notes.md").exists());
    }

    #[test]
    fn creates_renames_and_moves_entries() {
        let (root, sandbox) = fixture();
        let folder = create_folder(&sandbox, sandbox.root(), "Inbox").unwrap();
        assert!(folder.ends_with("Inbox"));
        assert!(create_folder(&sandbox, sandbox.root(), "../escape").is_err());
        assert!(create_folder(&sandbox, sandbox.root(), "a/b").is_err());

        let renamed = rename(&sandbox, &root.join("readme.txt"), "summary.txt").unwrap();
        assert!(renamed.ends_with("summary.txt"));
        assert!(renamed.exists());
        assert!(!root.join("readme.txt").exists());

        let moved = move_entry(&sandbox, &renamed, &folder).unwrap();
        assert_eq!(moved, folder.join("summary.txt"));
        assert!(moved.exists());
        assert!(!root.join("summary.txt").exists());

        // Moving into the current parent is a no-op, not a duplicate.
        let same = move_entry(&sandbox, &moved, &folder).unwrap();
        assert_eq!(same, moved);
    }

    #[test]
    fn rename_refuses_to_overwrite_an_existing_entry() {
        let (root, sandbox) = fixture();
        fs::write(root.join("b.txt"), "BBB").unwrap();
        let error = rename(&sandbox, &root.join("readme.txt"), "b.txt").unwrap_err();
        assert!(matches!(error, FsError::AlreadyExists(_)));
        assert_eq!(fs::read_to_string(root.join("readme.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "BBB",
            "the existing file is untouched"
        );
        // A free name still works.
        rename(&sandbox, &root.join("readme.txt"), "renamed.txt").unwrap();
        assert!(root.join("renamed.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clipboard_copies_and_moves() {
        let (root, sandbox) = fixture();
        let notes = root.join("Documents/notes.md");
        let inbox = create_folder(&sandbox, sandbox.root(), "Inbox").unwrap();

        let copy = Clipboard::new(ClipboardOp::Copy, vec![notes.clone()]);
        let written = paste(&sandbox, &copy, &inbox, &mut |_, _| {}).unwrap();
        assert_eq!(written[0], inbox.join("notes.md"));
        assert!(notes.exists(), "copy leaves the source in place");

        let cut = Clipboard::new(ClipboardOp::Cut, vec![notes.clone()]);
        let written = paste(&sandbox, &cut, &inbox, &mut |_, _| {}).unwrap();
        assert_eq!(written[0], inbox.join("notes (2).md"));
        assert!(!notes.exists(), "cut removes the source");
        assert_eq!(copy.summary(), "1 item copied");
    }

    #[test]
    fn trash_moves_into_dot_trash() {
        let (root, sandbox) = fixture();
        let target = root.join("readme.txt");
        let trashed = trash(&sandbox, &target).unwrap();
        assert_eq!(trashed, sandbox.root().join(".trash/readme.txt"));
        assert!(trashed.exists());
        assert!(!target.exists());

        fs::write(root.join("readme.txt"), "again").unwrap();
        let second = trash(&sandbox, &root.join("readme.txt")).unwrap();
        assert_eq!(second, sandbox.root().join(".trash/readme (2).txt"));
    }

    #[test]
    fn rejects_traversal_and_symlink_escapes() {
        let (root, sandbox) = fixture();
        let outsider = temp_dir("outsider");
        fs::write(outsider.join("secret.txt"), "secret").unwrap();

        let traversal = list(&sandbox, &sandbox.root().join("../../"));
        assert!(matches!(traversal, Err(FsError::OutsideSandbox(_))));

        let copy_out = copy_entry(&sandbox, &root.join("readme.txt"), &outsider, &mut |_, _| {});
        assert!(matches!(copy_out, Err(FsError::OutsideSandbox(_))));

        let rename_out = rename(&sandbox, &root.join("readme.txt"), "../escaped.txt");
        assert!(rename_out.is_err());

        #[cfg(unix)]
        {
            let link = sandbox.root().join("escape");
            std::os::unix::fs::symlink(&outsider, &link).unwrap();
            let escaped = list(&sandbox, &link);
            assert!(
                matches!(escaped, Err(FsError::OutsideSandbox(_))),
                "a symlink out of the sandbox is rejected: {escaped:?}"
            );
        }
    }

    #[test]
    fn permission_errors_are_reported() {
        let (root, sandbox) = fixture();
        let locked = root.join("locked");
        fs::create_dir(&locked).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        }

        let result = list(&sandbox, &locked);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let error = result.expect_err("a 000 folder cannot be listed");
            assert!(error.is_permission_denied(), "{error}");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = result;
        }
        // Restoring the mode keeps the temporary tree removable.
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn formats_sizes_dates_and_paths() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_date(None), "—");
        let epoch_plus = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(format_date(Some(epoch_plus)), "2023-11-14 22:13");

        let (root, sandbox) = fixture();
        let documents = root.join("Documents");
        assert_eq!(sandbox.display_path(&documents), "Home / Documents");
        assert_eq!(sandbox.crumbs(&documents), vec!["Home", "Documents"]);
        assert_eq!(sandbox.relative(&documents), PathBuf::from("Documents"));
        assert_eq!(sandbox.display_path(sandbox.root()), "Home");
    }
}
