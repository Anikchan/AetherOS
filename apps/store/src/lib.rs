//! Software app core: local repository index and install/update/remove
//! (P6-T09, F9 AP-AC-08).
//!
//! The index points at bundles on disk (a local repository or a mounted
//! download cache); no network code is implied. Installing delegates to
//! `aether-pkg`, so permissions and checksums behave exactly as on the CLI.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aether_pkg::manifest::{read_manifest, AppId, PackageManifest};
use aether_pkg::{
    InstallError, InstallOutcome, Installer, PackageInstaller, Paths,
};

pub mod contract;

pub use contract::{
    handle, handle_json, handle_with_compat, rows, AppRow, ChangeRow, CompatIndex, StoreRequest,
    StoreResponse, DUAL_BOOT_GUIDANCE,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub manifest: PackageManifest,
    /// Bundle directory or file the software app installs from.
    pub source: PathBuf,
    #[serde(default)]
    pub channel: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreIndex {
    pub apps: BTreeMap<String, Vec<IndexEntry>>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("app {0} is not in the store")]
    NotFound(String),
    #[error("install failed: {0}")]
    Install(#[from] InstallError),
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    #[error("index error: {0}")]
    Index(String),
}

impl StoreIndex {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }

    /// Build an index from a repository directory of bundles.
    pub fn scan(repo: &Path) -> Result<Self, StoreError> {
        let mut index = Self::default();
        for entry in walkdir::WalkDir::new(repo).max_depth(3) {
            let entry = entry.map_err(|e| StoreError::Index(e.to_string()))?;
            if !entry.file_type().is_dir() {
                continue;
            }
            if !entry.path().join(aether_pkg::MANIFEST_NAME).exists() {
                continue;
            }
            let manifest = read_manifest(entry.path())
                .map_err(|e| StoreError::Index(e.to_string()))?;
            index.add(IndexEntry {
                manifest,
                source: entry.path().to_path_buf(),
                channel: "stable".into(),
            });
        }
        Ok(index)
    }

    pub fn add(&mut self, entry: IndexEntry) {
        self.apps
            .entry(entry.manifest.id.as_str().to_string())
            .or_default()
            .push(entry);
        for versions in self.apps.values_mut() {
            versions.sort_by(|a, b| version_cmp(&b.manifest.version, &a.manifest.version));
        }
    }

    pub fn latest(&self, id: &str) -> Option<&IndexEntry> {
        self.apps.get(id).and_then(|versions| versions.first())
    }

    pub fn search(&self, query: &str) -> Vec<&IndexEntry> {
        let query = query.to_ascii_lowercase();
        let mut rows: Vec<&IndexEntry> = self
            .apps
            .values()
            .filter_map(|versions| versions.first())
            .filter(|entry| {
                query.is_empty()
                    || entry.manifest.name.to_ascii_lowercase().contains(&query)
                    || entry
                        .manifest
                        .id
                        .as_str()
                        .to_ascii_lowercase()
                        .contains(&query)
                    || entry
                        .manifest
                        .description
                        .as_deref()
                        .map(|text| text.to_ascii_lowercase().contains(&query))
                        .unwrap_or(false)
            })
            .collect();
        rows.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        rows
    }
}

/// Software app operations.
pub struct Store {
    pub paths: Paths,
    pub index: StoreIndex,
}

impl Store {
    pub fn new(paths: Paths, index: StoreIndex) -> Self {
        Self { paths, index }
    }

    pub fn installed(&self) -> Vec<aether_pkg::InstalledApp> {
        aether_pkg::Registry::load(&self.paths.registry)
            .list()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Installed apps that have a newer version in the index.
    pub fn updates(&self) -> Vec<(AppId, String, String)> {
        let mut updates = Vec::new();
        for app in self.installed() {
            if let Some(entry) = self.index.latest(app.manifest.id.as_str()) {
                if version_cmp(&entry.manifest.version, &app.manifest.version)
                    == std::cmp::Ordering::Greater
                {
                    updates.push((
                        app.manifest.id.clone(),
                        app.manifest.version.clone(),
                        entry.manifest.version.clone(),
                    ));
                }
            }
        }
        updates
    }

    /// Install the newest version, using already-granted permissions.
    pub fn install(
        &self,
        id: &str,
        granted: impl IntoIterator<Item = String>,
    ) -> Result<InstallOutcome, StoreError> {
        let entry = self
            .index
            .latest(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        let installer = Installer::new(self.paths.clone(), granted.into_iter().collect());
        Ok(installer.install(&entry.manifest, &entry.source)?)
    }

    /// Install the newest version of every app with an update available.
    pub fn update_all(
        &self,
        granted: impl IntoIterator<Item = String> + Clone,
    ) -> Vec<(AppId, Result<InstallOutcome, StoreError>)> {
        self.updates()
            .into_iter()
            .map(|(id, _, _)| {
                let result = self.install(id.as_str(), granted.clone().into_iter());
                (id, result)
            })
            .collect()
    }

    pub fn remove(&self, id: &AppId, granted: impl IntoIterator<Item = String>) -> Result<(), StoreError> {
        let installer = Installer::new(self.paths.clone(), granted.into_iter().collect());
        installer.uninstall(id)?;
        Ok(())
    }
}

/// Numeric-aware version comparison (`1.10` > `1.9`).
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |value: &str| -> Vec<u64> {
        value
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let left = parse(a);
    let right = parse(b);
    for index in 0..left.len().max(right.len()) {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_pkg::manifest::{compute_files, write_manifest, PackageKind};
    use std::io::Write;

    pub(crate) fn temp_root(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-store-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(crate) fn bundle(repo: &Path, id: &str, name: &str, version: &str) {
        let dir = repo.join(format!("{name}-{version}"));
        fs::create_dir_all(dir.join("bin")).unwrap();
        let mut file = fs::File::create(dir.join("bin/app")).unwrap();
        writeln!(file, "#!/bin/sh\necho {name} {version}").unwrap();
        let files = compute_files(&dir).unwrap();
        let manifest = PackageManifest {
            id: AppId::new(id).unwrap(),
            name: name.into(),
            version: version.into(),
            kind: PackageKind::NativeBundle,
            permissions: vec![],
            entry: "bin/app".into(),
            installed_bytes: files.iter().map(|f| f.size).sum(),
            signature: None,
            files,
            description: Some(format!("{name} test bundle")),
            homepage: None,
        };
        write_manifest(&dir, &manifest).unwrap();
    }

    #[test]
    fn scans_repository_and_searches() {
        let root = temp_root("scan");
        let repo = root.join("repo");
        bundle(&repo, "org.example.Text", "Text", "1.0.0");
        bundle(&repo, "org.example.Paint", "Paint", "2.0.0");
        let index = StoreIndex::scan(&repo).unwrap();
        assert_eq!(index.apps.len(), 2);
        assert_eq!(index.search("paint").len(), 1);
        assert_eq!(index.search("").len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_update_and_remove_flow() {
        let root = temp_root("flow");
        let repo = root.join("repo");
        bundle(&repo, "org.example.Text", "Text", "1.0.0");
        let index = StoreIndex::scan(&repo).unwrap();
        let paths = Paths::at(root.join("store"));
        let store = Store::new(paths.clone(), index.clone());
        let outcome = store.install("org.example.Text", Vec::new()).unwrap();
        assert!(matches!(outcome, InstallOutcome::Installed(_)));
        assert_eq!(store.installed().len(), 1);
        assert!(store.updates().is_empty());

        // New version appears in the repository.
        bundle(&repo, "org.example.Text", "Text", "1.1.0");
        let index = StoreIndex::scan(&repo).unwrap();
        let store = Store::new(paths.clone(), index);
        let updates = store.updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].2, "1.1.0");

        let installed = store.install("org.example.Text", Vec::new()).unwrap();
        assert!(matches!(installed, InstallOutcome::Installed(_)));
        let app = aether_pkg::Registry::load(&paths.registry)
            .list()
            .into_iter()
            .next()
            .unwrap()
            .clone();
        assert_eq!(app.manifest.version, "1.1.0");

        let id = AppId::new("org.example.Text").unwrap();
        store.remove(&id, Vec::new()).unwrap();
        assert!(store.installed().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_app_is_reported() {
        let root = temp_root("missing");
        let store = Store::new(Paths::at(root.join("store")), StoreIndex::default());
        assert!(matches!(
            store.install("org.example.Nope", Vec::new()),
            Err(StoreError::NotFound(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_compare_is_numeric() {
        assert_eq!(
            version_cmp("1.10.0", "1.9.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(version_cmp("2.0", "2.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("1.0.1", "1.1"), std::cmp::Ordering::Less);
    }

    #[test]
    fn index_round_trips() {
        let root = temp_root("index");
        let repo = root.join("repo");
        bundle(&repo, "org.example.Text", "Text", "1.0.0");
        let index = StoreIndex::scan(&repo).unwrap();
        let path = root.join("index.json");
        index.save(&path).unwrap();
        let loaded = StoreIndex::load(&path).unwrap();
        assert_eq!(loaded, index);
        let _ = fs::remove_dir_all(root);
    }
}
