//! App identity and one cgroup scope per app (P5-T01, D3 section 4).
//!
//! Every running app gets exactly one cgroup v2 leaf scope under
//! `<root>/aether.slice/aether-apps.slice/`. Identity comes from the desktop
//! file ID (for example `org.example.Mail`); the scope unit name is derived
//! deterministically and survives daemon restarts, so state can be rebuilt
//! from the cgroup tree (D3 section 8, "Daemon crash").

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Root slice for everything AetherOS manages.
pub const ROOT_SLICE: &str = "aether.slice";
/// Slice holding one scope per app.
pub const APP_SLICE: &str = "aether-apps.slice";
/// Prefix of every app scope unit.
pub const SCOPE_PREFIX: &str = "aether-app-";

/// Validation failure for an app identity.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("app id is empty")]
    Empty,
    #[error("app id is longer than 255 bytes ({0})")]
    TooLong(usize),
    #[error("app id contains an invalid character: {0:?}")]
    InvalidChar(char),
    #[error("app id contains an empty segment")]
    EmptySegment,
    #[error("app id segment must start with a letter or digit: {0:?}")]
    InvalidSegmentStart(String),
}

/// Desktop-file style application identity, for example `org.example.Mail`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AppId(String);

impl AppId {
    /// Validate and wrap a desktop-file ID.
    pub fn new(raw: impl Into<String>) -> Result<Self, IdentityError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(IdentityError::Empty);
        }
        if raw.len() > 255 {
            return Err(IdentityError::TooLong(raw.len()));
        }
        for segment in raw.split('.') {
            if segment.is_empty() {
                return Err(IdentityError::EmptySegment);
            }
            let mut chars = segment.chars();
            let first = chars.next().expect("segment is non-empty");
            if !first.is_ascii_alphanumeric() {
                return Err(IdentityError::InvalidSegmentStart(segment.to_string()));
            }
            for c in segment.chars() {
                if !(c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return Err(IdentityError::InvalidChar(c));
                }
            }
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Deterministic systemd-style unit name for an app.
///
/// Characters outside `[A-Za-z0-9_.-]` are replaced with `_`. An 8-hex-digit
/// FNV-1a hash of the raw ID is always appended, which keeps the mapping
/// one-to-one even when two IDs sanitize to the same string (for example
/// `org.example.a_b` and `org.example.a-b` are distinct IDs).
pub fn scope_unit_name(id: &AppId) -> String {
    let mut sanitized = String::with_capacity(id.as_str().len());
    for c in id.as_str().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
            sanitized.push(c);
        } else {
            sanitized.push('_');
        }
    }
    format!("{SCOPE_PREFIX}{sanitized}-{:08x}.scope", fnv1a(id.as_str()))
}

fn fnv1a(input: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in input.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Path of an app's scope, relative to the cgroup v2 mount point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppScope {
    pub id: AppId,
    pub unit: String,
    /// `aether.slice/aether-apps.slice/<unit>`
    pub relative_path: PathBuf,
}

impl AppScope {
    pub fn new(id: AppId) -> Self {
        let unit = scope_unit_name(&id);
        let relative_path = Path::new(ROOT_SLICE).join(APP_SLICE).join(&unit);
        Self {
            id,
            unit,
            relative_path,
        }
    }

    /// Absolute scope path under a cgroup v2 mount point (normally
    /// `/sys/fs/cgroup`).
    pub fn path(&self, cgroup_root: &Path) -> PathBuf {
        cgroup_root.join(&self.relative_path)
    }
}

/// Creates and manages the cgroup scope of each app.
pub trait ScopeManager {
    /// Create (idempotently) the app scope and return it.
    fn ensure_scope(&self, id: &AppId) -> io::Result<AppScope>;
    /// Move one process into the app scope.
    fn attach(&self, id: &AppId, pid: u32) -> io::Result<()>;
    /// Scope names that exist in the tree, sorted.
    fn list_scopes(&self) -> io::Result<Vec<String>>;
    /// Remove an app scope directory (best effort, empty scopes only).
    fn remove_scope(&self, id: &AppId) -> io::Result<()>;
}

/// Scope manager backed by a real cgroup v2 mount (`/sys/fs/cgroup`).
///
/// `dry_run` performs only the in-memory bookkeeping and the path
/// computation; it is used by benchmark harnesses and macOS development so
/// the daemon can be exercised without a Linux cgroup tree.
#[derive(Debug, Clone)]
pub struct CgroupScopeManager {
    root: PathBuf,
    dry_run: bool,
}

impl CgroupScopeManager {
    pub fn system() -> Self {
        Self {
            root: PathBuf::from("/sys/fs/cgroup"),
            dry_run: false,
        }
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            dry_run: false,
        }
    }

    pub fn dry_run(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            dry_run: true,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// True when the host exposes a cgroup v2 tree at the configured root.
    pub fn available(&self) -> bool {
        if self.dry_run {
            return true;
        }
        let controllers = self.root.join("cgroup.controllers");
        fs::read_to_string(controllers)
            .map(|s| s.split_whitespace().any(|c| c == "memory"))
            .unwrap_or(false)
    }

    fn enable_controllers(&self, slice_dir: &Path) -> io::Result<()> {
        let control = slice_dir.join("cgroup.subtree_control");
        if control.exists() && fs::read_to_string(&control)?.trim() != "+memory +cpu +pids" {
            // Best effort: controllers may already be enabled by the service
            // manager, or delegated to us in a subtree.
            let _ = fs::write(&control, "+memory +cpu +pids");
        }
        Ok(())
    }
}

impl ScopeManager for CgroupScopeManager {
    fn ensure_scope(&self, id: &AppId) -> io::Result<AppScope> {
        let scope = AppScope::new(id.clone());
        if self.dry_run {
            return Ok(scope);
        }
        let root_slice = self.root.join(ROOT_SLICE);
        let app_slice = root_slice.join(APP_SLICE);
        fs::create_dir_all(&app_slice)?;
        self.enable_controllers(&root_slice)?;
        self.enable_controllers(&app_slice)?;
        fs::create_dir_all(scope.path(&self.root))?;
        Ok(scope)
    }

    fn attach(&self, id: &AppId, pid: u32) -> io::Result<()> {
        let scope = AppScope::new(id.clone());
        let procs = scope.path(&self.root).join("cgroup.procs");
        if self.dry_run {
            return Ok(());
        }
        fs::write(procs, pid.to_string())
    }

    fn list_scopes(&self) -> io::Result<Vec<String>> {
        if self.dry_run {
            return Ok(Vec::new());
        }
        let app_slice = self.root.join(ROOT_SLICE).join(APP_SLICE);
        let mut units = Vec::new();
        if !app_slice.exists() {
            return Ok(units);
        }
        for entry in fs::read_dir(app_slice)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(SCOPE_PREFIX) && name.ends_with(".scope") {
                units.push(name);
            }
        }
        units.sort();
        Ok(units)
    }

    fn remove_scope(&self, id: &AppId) -> io::Result<()> {
        if self.dry_run {
            return Ok(());
        }
        let scope = AppScope::new(id.clone());
        match fs::remove_dir(scope.path(&self.root)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// In-memory scope manager used by unit tests and the benchmark harness.
/// It enforces the "one scope per app" rule explicitly.
#[derive(Debug, Default)]
pub struct MemoryScopeManager {
    scopes: std::sync::Mutex<BTreeMap<AppId, Vec<u32>>>,
}

impl MemoryScopeManager {
    pub fn pids_of(&self, id: &AppId) -> Vec<u32> {
        self.scopes
            .lock()
            .expect("scope map poisoned")
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn scope_count(&self) -> usize {
        self.scopes.lock().expect("scope map poisoned").len()
    }
}

impl ScopeManager for MemoryScopeManager {
    fn ensure_scope(&self, id: &AppId) -> io::Result<AppScope> {
        self.scopes
            .lock()
            .expect("scope map poisoned")
            .entry(id.clone())
            .or_default();
        Ok(AppScope::new(id.clone()))
    }

    fn attach(&self, id: &AppId, pid: u32) -> io::Result<()> {
        let mut scopes = self.scopes.lock().expect("scope map poisoned");
        let entry = scopes.entry(id.clone()).or_default();
        if !entry.contains(&pid) {
            entry.push(pid);
        }
        Ok(())
    }

    fn list_scopes(&self) -> io::Result<Vec<String>> {
        Ok(self
            .scopes
            .lock()
            .expect("scope map poisoned")
            .keys()
            .map(scope_unit_name)
            .collect())
    }

    fn remove_scope(&self, id: &AppId) -> io::Result<()> {
        self.scopes
            .lock()
            .expect("scope map poisoned")
            .remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_desktop_ids() {
        for id in [
            "org.example.Mail",
            "aether.text",
            "com.github.owner_app-2",
            "Firefox",
        ] {
            assert!(AppId::new(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_ids() {
        assert_eq!(AppId::new(""), Err(IdentityError::Empty));
        assert_eq!(AppId::new("org..Mail"), Err(IdentityError::EmptySegment));
        assert_eq!(AppId::new(".Mail"), Err(IdentityError::EmptySegment));
        assert!(matches!(
            AppId::new("org.exa mple"),
            Err(IdentityError::InvalidChar(' '))
        ));
        assert!(matches!(
            AppId::new("org.-example"),
            Err(IdentityError::InvalidSegmentStart(_))
        ));
        assert!(matches!(
            AppId::new("a".repeat(256)),
            Err(IdentityError::TooLong(256))
        ));
    }

    #[test]
    fn scope_path_is_deterministic() {
        let id = AppId::new("org.example.Mail").unwrap();
        let a = AppScope::new(id.clone());
        let b = AppScope::new(id);
        assert_eq!(a, b);
        assert_eq!(
            a.relative_path,
            Path::new("aether.slice/aether-apps.slice")
                .join(scope_unit_name(&AppId::new("org.example.Mail").unwrap()))
        );
        assert!(a.unit.starts_with("aether-app-org.example.Mail-"));
        assert_eq!(
            a.path(Path::new("/sys/fs/cgroup")),
            Path::new("/sys/fs/cgroup/aether.slice/aether-apps.slice").join(&a.unit)
        );
    }

    #[test]
    fn distinct_ids_never_share_a_scope() {
        let a = AppId::new("org.example.a_b").unwrap();
        let b = AppId::new("org.example.a-b").unwrap();
        // Both sanitize to the same text, but the hash suffix keeps them apart.
        assert_ne!(scope_unit_name(&a), scope_unit_name(&b));
        assert_ne!(AppScope::new(a).relative_path, AppScope::new(b).relative_path);
    }

    #[test]
    fn one_scope_per_app() {
        let manager = MemoryScopeManager::default();
        let id = AppId::new("org.example.Mail").unwrap();
        let first = manager.ensure_scope(&id).unwrap();
        let second = manager.ensure_scope(&id).unwrap();
        manager.attach(&id, 100).unwrap();
        manager.attach(&id, 101).unwrap();

        assert_eq!(first, second);
        assert_eq!(manager.scope_count(), 1);
        assert_eq!(manager.pids_of(&id), vec![100, 101]);
        assert_eq!(manager.list_scopes().unwrap(), vec![first.unit]);
    }

    #[test]
    fn remove_scope_is_idempotent() {
        let manager = MemoryScopeManager::default();
        let id = AppId::new("org.example.Mail").unwrap();
        manager.ensure_scope(&id).unwrap();
        manager.remove_scope(&id).unwrap();
        manager.remove_scope(&id).unwrap();
        assert_eq!(manager.scope_count(), 0);
    }

    #[test]
    fn cgroup_manager_creates_leaf_scope() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "aether-scope-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let manager = CgroupScopeManager::at(&base);
        fs::create_dir_all(base.join(ROOT_SLICE).join(APP_SLICE)).unwrap();
        fs::write(
            base.join(ROOT_SLICE).join("cgroup.subtree_control"),
            "",
        )
        .unwrap();
        let id = AppId::new("org.example.Mail").unwrap();
        let scope = manager.ensure_scope(&id).unwrap();
        assert!(scope.path(&base).is_dir());
        assert!(base.join(ROOT_SLICE).join("cgroup.subtree_control").exists());
        assert_eq!(manager.list_scopes().unwrap(), vec![scope.unit.clone()]);
        manager.remove_scope(&id).unwrap();
        assert!(!scope.path(&base).exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dry_run_needs_no_cgroup_tree() {
        let manager = CgroupScopeManager::dry_run("/nonexistent/cgroup");
        let id = AppId::new("org.example.Mail").unwrap();
        assert!(manager.ensure_scope(&id).is_ok());
        assert!(manager.attach(&id, 42).is_ok());
        assert!(manager.available());
    }
}
