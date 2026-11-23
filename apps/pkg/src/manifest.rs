//! Package manifest and bundle format (P6-T01, D8 section 2).
//!
//! A native bundle is a directory containing `aether-manifest.toml` plus its
//! files. The manifest is the contract: identity, kind, permissions, entry
//! point, per-file SHA-256 checksums and the optional signature.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use aether_lifecycled::identity::{AppId, IdentityError};

pub const MANIFEST_NAME: &str = "aether-manifest.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageKind {
    NativeBundle,
    AppImage,
    WindowsInstaller,
    CrossPlatformBundle,
    MacosPackage,
}

impl PackageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeBundle => "native-bundle",
            Self::AppImage => "appimage",
            Self::WindowsInstaller => "windows-installer",
            Self::CrossPlatformBundle => "cross-platform-bundle",
            Self::MacosPackage => "macos-package",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileScope {
    PickedByUser,
    AppFolder,
    Home,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Permission {
    Network,
    Files { scope: FileScope },
    Camera,
    Microphone,
    Location,
    Notifications,
    Background,
}

impl Permission {
    pub fn label(&self) -> String {
        match self {
            Self::Network => "Network access".into(),
            Self::Files { scope } => format!("Files ({scope:?})"),
            Self::Camera => "Camera".into(),
            Self::Microphone => "Microphone".into(),
            Self::Location => "Location".into(),
            Self::Notifications => "Notifications".into(),
            Self::Background => "Run in background".into(),
        }
    }

    pub fn key(&self) -> String {
        match self {
            Self::Network => "network".into(),
            Self::Files { scope } => format!("files:{}", format!("{scope:?}").to_lowercase()),
            Self::Camera => "camera".into(),
            Self::Microphone => "microphone".into(),
            Self::Location => "location".into(),
            Self::Notifications => "notifications".into(),
            Self::Background => "background".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path relative to the bundle root.
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub id: AppId,
    pub name: String,
    pub version: String,
    pub kind: PackageKind,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    pub entry: String,
    pub installed_bytes: u64,
    #[serde(default)]
    pub signature: Option<String>,
    /// Every payload file except the manifest itself.
    #[serde(default)]
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest not found at {0}")]
    NotFound(PathBuf),
    #[error("invalid manifest: {0}")]
    Parse(String),
    #[error("invalid app id: {0}")]
    Identity(#[from] IdentityError),
    #[error("bundle file is missing: {0}")]
    MissingFile(String),
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("entry point {entry} is outside the bundle")]
    EntryOutside { entry: String },
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ManifestError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String, ManifestError> {
    let bytes = fs::read(path).map_err(|e| ManifestError::io(path, e))?;
    Ok(sha256_bytes(&bytes))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Parse a manifest from bundle text.
pub fn parse_manifest(text: &str) -> Result<PackageManifest, ManifestError> {
    let manifest: PackageManifest =
        toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
    // `AppId` is serde-transparent, so validate the identity explicitly.
    AppId::new(manifest.id.as_str())?;
    if manifest.entry.starts_with('/') || manifest.entry.split('/').any(|part| part == "..") {
        return Err(ManifestError::EntryOutside {
            entry: manifest.entry.clone(),
        });
    }
    Ok(manifest)
}

pub fn manifest_path(bundle_root: &Path) -> PathBuf {
    bundle_root.join(MANIFEST_NAME)
}

/// Read a manifest from a bundle directory.
pub fn read_manifest(bundle_root: &Path) -> Result<PackageManifest, ManifestError> {
    let path = manifest_path(bundle_root);
    if !path.exists() {
        return Err(ManifestError::NotFound(path));
    }
    let text = fs::read_to_string(&path).map_err(|e| ManifestError::io(&path, e))?;
    parse_manifest(&text)
}

/// Write a manifest as TOML.
pub fn write_manifest(bundle_root: &Path, manifest: &PackageManifest) -> Result<(), ManifestError> {
    let text = toml::to_string_pretty(manifest)
        .map_err(|e| ManifestError::Parse(e.to_string()))?;
    fs::write(manifest_path(bundle_root), text)
        .map_err(|e| ManifestError::io(manifest_path(bundle_root), e))
}

/// Compute the file list and checksums of a bundle directory.
pub fn compute_files(bundle_root: &Path) -> Result<Vec<FileEntry>, ManifestError> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(bundle_root).follow_links(false) {
        let entry = entry.map_err(|e| {
            ManifestError::io(
                bundle_root,
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(bundle_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if relative == MANIFEST_NAME {
            continue;
        }
        let size = entry
            .metadata()
            .map_err(|e| {
                ManifestError::io(
                    entry.path(),
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                )
            })?
            .len();
        files.push(FileEntry {
            path: relative,
            sha256: sha256_file(entry.path())?,
            size,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Verify every listed file exists and matches its size and checksum.
/// Returns the total installed size.
pub fn verify_files(bundle_root: &Path, manifest: &PackageManifest) -> Result<u64, ManifestError> {
    let mut total = 0u64;
    for file in &manifest.files {
        let path = bundle_root.join(&file.path);
        if !path.exists() {
            return Err(ManifestError::MissingFile(file.path.clone()));
        }
        let actual = sha256_file(&path)?;
        if actual != file.sha256 {
            return Err(ManifestError::ChecksumMismatch {
                path: file.path.clone(),
                expected: file.sha256.clone(),
                actual,
            });
        }
        let size = file
            .metadata_size(&path)
            .map_err(|e| ManifestError::io(&path, e))?;
        if size != file.size {
            return Err(ManifestError::ChecksumMismatch {
                path: file.path.clone(),
                expected: format!("{} bytes", file.size),
                actual: format!("{size} bytes"),
            });
        }
        total += size;
    }
    Ok(total)
}

impl FileEntry {
    fn metadata_size(&self, path: &Path) -> std::io::Result<u64> {
        Ok(fs::metadata(path)?.len())
    }
}

/// Permissions that are not covered by an explicit grant.
pub fn missing_permissions<'a>(
    manifest: &'a PackageManifest,
    granted: &BTreeSet<String>,
) -> Vec<&'a Permission> {
    manifest
        .permissions
        .iter()
        .filter(|permission| !granted.contains(&permission.key()))
        .collect()
}

/// Minimal manifest for an AppImage (no embedded manifest).
pub fn appimage_manifest(path: &Path) -> Result<PackageManifest, ManifestError> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app.AppImage".into());
    let stem = file_name
        .trim_end_matches(".AppImage")
        .trim_end_matches(".appimage")
        .to_string();
    let id = AppId::new(stem.replace(' ', "-"))
        .map_err(ManifestError::Identity)?;
    let size = fs::metadata(path)
        .map_err(|e| ManifestError::io(path, e))?
        .len();
    Ok(PackageManifest {
        id,
        name: stem,
        version: "0.0.0".into(),
        kind: PackageKind::AppImage,
        permissions: vec![
            Permission::Network,
            Permission::Files {
                scope: FileScope::PickedByUser,
            },
        ],
        entry: file_name,
        installed_bytes: size,
        signature: None,
        files: Vec::new(),
        description: Some("AppImage (permissions requested at install)".into()),
        homepage: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-pkg-manifest-{tag}-{}-{}-{}",
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

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    fn sample_manifest(root: &Path) -> PackageManifest {
        write(&root.join("bin/app"), "#!/bin/sh\necho hello\n");
        write(&root.join("share/icon.svg"), "<svg/>");
        let files = compute_files(root).unwrap();
        PackageManifest {
            id: AppId::new("org.example.Hello").unwrap(),
            name: "Hello".into(),
            version: "1.2.3".into(),
            kind: PackageKind::NativeBundle,
            permissions: vec![Permission::Network],
            entry: "bin/app".into(),
            installed_bytes: files.iter().map(|f| f.size).sum(),
            signature: None,
            files,
            description: None,
            homepage: None,
        }
    }

    #[test]
    fn manifest_round_trips_through_toml() {
        let dir = temp_dir("roundtrip");
        let manifest = sample_manifest(&dir);
        write_manifest(&dir, &manifest).unwrap();
        let parsed = read_manifest(&dir).unwrap();
        assert_eq!(parsed, manifest);
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.permissions[0].key(), "network");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn checksums_detect_tampering() {
        let dir = temp_dir("tamper");
        let manifest = sample_manifest(&dir);
        verify_files(&dir, &manifest).unwrap();
        write(&dir.join("bin/app"), "#!/bin/sh\necho hacked\n");
        assert!(matches!(
            verify_files(&dir, &manifest),
            Err(ManifestError::ChecksumMismatch { .. })
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn entry_cannot_escape_the_bundle() {
        let text = r#"
id = "org.example.Bad"
name = "Bad"
version = "1"
kind = "native-bundle"
entry = "../outside"
installed_bytes = 1
"#;
        assert!(matches!(
            parse_manifest(text),
            Err(ManifestError::EntryOutside { .. })
        ));
    }

    #[test]
    fn invalid_ids_are_rejected() {
        let text = r#"
id = "bad id"
name = "Bad"
version = "1"
kind = "native-bundle"
entry = "app"
installed_bytes = 1
"#;
        assert!(matches!(parse_manifest(text), Err(ManifestError::Identity(_))));
    }

    #[test]
    fn missing_permissions_are_reported() {
        let dir = temp_dir("perm");
        let manifest = sample_manifest(&dir);
        let mut granted = BTreeSet::new();
        assert_eq!(missing_permissions(&manifest, &granted).len(), 1);
        granted.insert("network".to_string());
        assert!(missing_permissions(&manifest, &granted).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn appimage_manifest_is_inferred() {
        let dir = temp_dir("appimage");
        let path = dir.join("Cool App.AppImage");
        fs::write(&path, "binary").unwrap();
        let manifest = appimage_manifest(&path).unwrap();
        assert_eq!(manifest.kind, PackageKind::AppImage);
        assert_eq!(manifest.id.as_str(), "Cool-App");
        assert_eq!(manifest.entry, "Cool App.AppImage");
        let _ = fs::remove_dir_all(dir);
    }
}
