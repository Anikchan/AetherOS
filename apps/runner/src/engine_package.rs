//! Windows engine packaging and versioning (P6-T05, D8 section 5).
//!
//! An engine is installed as `engines/<name>-<version>/` with an
//! `aether-engine.toml` manifest that pins every file's size and SHA-256.
//! Discovery walks the engine root; verification refuses a tampered engine
//! before it is used to run a Windows app.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aether_pkg::manifest::sha256_file;

/// Manifest file inside an engine directory.
pub const ENGINE_MANIFEST: &str = "aether-engine.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnginePackage {
    /// Runtime name, for example `wine`.
    pub name: String,
    /// Runtime version, for example `10.0`.
    pub version: String,
    pub arch: String,
    pub installed_bytes: u64,
    pub files: Vec<EngineFile>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub min_os: Option<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid engine manifest at {0}")]
    Manifest(PathBuf),
    #[error("engine {0} has no {ENGINE_MANIFEST}")]
    NotPackaged(PathBuf),
    #[error("engine verification failed: {0} missing, {1} changed")]
    Verify(usize, usize),
}

impl EngineError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl EnginePackage {
    /// Build a package description from a staged engine tree.
    pub fn compute(
        root: &Path,
        name: &str,
        version: &str,
        arch: &str,
    ) -> Result<Self, EngineError> {
        let files = collect_files(root)?;
        let installed_bytes = files.iter().map(|file| file.size).sum();
        Ok(Self {
            name: name.into(),
            version: version.into(),
            arch: arch.into(),
            installed_bytes,
            files,
            license: String::new(),
            min_os: None,
            notes: String::new(),
        })
    }

    /// Write `aether-engine.toml` inside the engine directory.
    pub fn write(&self, root: &Path) -> Result<PathBuf, EngineError> {
        let path = root.join(ENGINE_MANIFEST);
        let text = toml::to_string_pretty(self)
            .map_err(|_| EngineError::Manifest(path.clone()))?;
        fs::write(&path, text).map_err(|error| EngineError::io(&path, error))?;
        Ok(path)
    }

    pub fn read(root: &Path) -> Result<Self, EngineError> {
        let path = root.join(ENGINE_MANIFEST);
        if !path.exists() {
            return Err(EngineError::NotPackaged(root.to_path_buf()));
        }
        let text =
            fs::read_to_string(&path).map_err(|error| EngineError::io(&path, error))?;
        toml::from_str(&text).map_err(|_| EngineError::Manifest(path))
    }

    /// Files that are missing or changed relative to the manifest.
    pub fn verify(&self, root: &Path) -> Result<Vec<String>, EngineError> {
        let mut problems = Vec::new();
        for file in &self.files {
            let path = root.join(&file.path);
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    problems.push(format!("missing {}", file.path));
                    continue;
                }
            };
            if metadata.len() != file.size {
                problems.push(format!("size changed {}", file.path));
                continue;
            }
            let digest = sha256_file(&path)
                .map_err(|_| EngineError::Manifest(path.clone()))?;
            if digest != file.sha256 {
                problems.push(format!("checksum changed {}", file.path));
            }
        }
        Ok(problems)
    }

    /// Engine directory name used by the registry (`wine-10.0`).
    pub fn dir_name(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }
}

fn collect_files(root: &Path) -> Result<Vec<EngineFile>, EngineError> {
    let mut files = Vec::new();
    collect_into(root, root, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn collect_into(root: &Path, at: &Path, files: &mut Vec<EngineFile>) -> Result<(), EngineError> {
    let entries = fs::read_dir(at).map_err(|error| EngineError::io(at, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| EngineError::io(at, error))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| EngineError::io(&path, error))?;
        if file_type.is_dir() {
            collect_into(root, &path, files)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative == ENGINE_MANIFEST {
            continue;
        }
        let size = entry
            .metadata()
            .map_err(|error| EngineError::io(&path, error))?
            .len();
        let sha256 = sha256_file(&path).map_err(|_| EngineError::Manifest(path.clone()))?;
        files.push(EngineFile {
            path: relative,
            size,
            sha256,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-engine-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        dir
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn package_round_trips_and_verifies() {
        let root = temp_root("roundtrip");
        fs::write(root.join("bin/wine"), "#!/bin/sh\n").unwrap();
        let package = EnginePackage::compute(&root, "wine", "10.0", "x86_64").unwrap();
        assert_eq!(package.installed_bytes, 10);
        package.write(&root).unwrap();
        let loaded = EnginePackage::read(&root).unwrap();
        assert_eq!(loaded, package);
        assert!(loaded.verify(&root).unwrap().is_empty());
        assert_eq!(loaded.dir_name(), "wine-10.0");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tampering_is_detected() {
        let root = temp_root("tamper");
        fs::write(root.join("bin/wine"), "#!/bin/sh\n").unwrap();
        let package = EnginePackage::compute(&root, "wine", "10.0", "x86_64").unwrap();
        package.write(&root).unwrap();
        fs::write(root.join("bin/wine"), "#!/bin/shX").unwrap();
        let problems = package.verify(&root).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("checksum changed"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_lists_packaged_engines() {
        let root = temp_root("registry");
        let engine = root.join("engines/wine-10.0");
        fs::create_dir_all(engine.join("bin")).unwrap();
        fs::write(engine.join("bin/wine"), "#!/bin/sh\n").unwrap();
        EnginePackage::compute(&engine, "wine", "10.0", "x86_64")
            .unwrap()
            .write(&engine)
            .unwrap();
        // A legacy directory without a manifest is not a packaged engine.
        fs::create_dir_all(root.join("engines/wine-9.0")).unwrap();
        let registry = crate::EngineRegistry::at(root.join("engines"));
        assert_eq!(registry.packages().len(), 1);
        assert_eq!(registry.verify_all(), vec![("10.0".to_string(), Vec::new())]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn packaging_a_staged_tree_writes_the_manifest() {
        let root = temp_root("pack");
        fs::write(root.join("bin/wine"), "#!/bin/sh\n").unwrap();
        let registry = crate::EngineRegistry::at(root.join("engines"));
        let package = registry.package(&root, "wine", "10.0", "x86_64").unwrap();
        assert!(root.join(ENGINE_MANIFEST).is_file());
        assert!(package.verify(&root).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
