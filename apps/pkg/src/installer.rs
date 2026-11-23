//! Installer and file registry (P6-T01/T02, D8 sections 3 and 4).
//!
//! Install is atomic: stage → verify checksums → move into place → registry.
//! A failure at any step removes staging and leaves the registry untouched.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::manifest::{
    appimage_manifest, missing_permissions, read_manifest, verify_files, AppId, FileEntry,
    ManifestError, PackageKind, PackageManifest, Permission,
};

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("manifest error: {0}")]
    Manifest(#[from] ManifestError),
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("signature is not valid")]
    BadSignature,
    #[error("the app is already installed by another user or location: {0}")]
    Conflict(String),
    #[error("package kind {0} is not supported on this system")]
    Unsupported(String),
    #[error("staging failed: {0}")]
    Staging(String),
}

impl InstallError {
    fn io(path: impl Into<PathBuf>, error: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source: error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed(AppId),
    NeedsPermission(Vec<Permission>),
    Unsupported {
        reason: String,
        alternatives: Vec<String>,
    },
}

/// D8 section 2 contract.
pub trait PackageInstaller {
    fn inspect(&self, file: &Path) -> Result<PackageManifest, InstallError>;
    fn install(
        &self,
        manifest: &PackageManifest,
        file: &Path,
    ) -> Result<InstallOutcome, InstallError>;
    fn uninstall(&self, id: &AppId) -> Result<(), InstallError>;
}

/// Install locations (D8 section 3, `aether-` naming per ADR-012).
#[derive(Debug, Clone)]
pub struct Paths {
    pub applications: PathBuf,
    pub staging: PathBuf,
    pub registry: PathBuf,
    pub data: PathBuf,
    pub prefixes: PathBuf,
}

impl Paths {
    pub fn for_user() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/home/user"));
        let share = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("aether");
        let applications = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("aether/applications");
        Self {
            applications,
            staging: share.join("staging"),
            registry: share.join("install-registry.json"),
            data: share.join("apps"),
            prefixes: share.join("prefixes"),
        }
    }

    /// Test/local root: everything lives under one directory.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            applications: root.join("applications"),
            staging: root.join("staging"),
            registry: root.join("install-registry.json"),
            data: root.join("apps"),
            prefixes: root.join("prefixes"),
        }
    }

    pub fn app_dir(&self, id: &AppId) -> PathBuf {
        self.applications.join(id.as_str())
    }

    pub fn app_file(&self, id: &AppId) -> PathBuf {
        self.applications.join(id.as_str())
    }

    pub fn data_dir(&self, id: &AppId) -> PathBuf {
        self.data.join(id.as_str()).join("data")
    }

    pub fn prefix_dir(&self, id: &AppId) -> PathBuf {
        self.prefixes.join(id.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub manifest: PackageManifest,
    pub target: PathBuf,
    pub installed_at: u64,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    pub apps: BTreeMap<String, InstalledApp>,
}

impl Registry {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), InstallError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| InstallError::Staging(e.to_string()))?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, text).map_err(|e| InstallError::io(&temp, e))?;
        fs::rename(&temp, path).map_err(|e| InstallError::io(path, e))
    }

    pub fn get(&self, id: &AppId) -> Option<&InstalledApp> {
        self.apps.get(id.as_str())
    }

    pub fn list(&self) -> Vec<&InstalledApp> {
        self.apps.values().collect()
    }
}

/// File-based installer.
#[derive(Debug)]
pub struct Installer {
    pub paths: Paths,
    granted: BTreeSet<String>,
    pub dry_run: bool,
}

impl Installer {
    pub fn new(paths: Paths, granted: BTreeSet<String>) -> Self {
        Self {
            paths,
            granted,
            dry_run: false,
        }
    }

    pub fn dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    pub fn grant(&mut self, permission: &Permission) {
        self.granted.insert(permission.key());
    }

    pub fn registry(&self) -> Registry {
        Registry::load(&self.paths.registry)
    }

    fn unsupported(&self, kind: PackageKind) -> InstallOutcome {
        InstallOutcome::Unsupported {
            reason: format!("{} packages cannot run on AetherOS", kind.as_str()),
            alternatives: match kind {
                PackageKind::MacosPackage => vec![
                    "Ask the vendor for a Linux build".into(),
                    "Run the program in a macOS virtual machine".into(),
                    "Look for a Windows version, which can run through the Windows engine".into(),
                ],
                _ => vec!["Look for a native AetherOS bundle or an AppImage".into()],
            },
        }
    }

    fn copy_tree(&self, source: &Path, target: &Path) -> Result<Vec<String>, InstallError> {
        fs::create_dir_all(target).map_err(|e| InstallError::io(target, e))?;
        let mut copied = Vec::new();
        for entry in walkdir::WalkDir::new(source).follow_links(false) {
            let entry = entry.map_err(|e| InstallError::Staging(e.to_string()))?;
            let relative = entry
                .path()
                .strip_prefix(source)
                .unwrap_or(entry.path())
                .to_path_buf();
            if relative.as_os_str().is_empty() {
                continue;
            }
            let destination = target.join(&relative);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&destination)
                    .map_err(|e| InstallError::io(&destination, e))?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
                }
                fs::copy(entry.path(), &destination)
                    .map_err(|e| InstallError::io(&destination, e))?;
                copied.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(copied)
    }

    fn verify_signature(&self, manifest: &PackageManifest) -> Result<(), InstallError> {
        // A signature is currently a SHA-256 of the manifest's file list.
        // The optional field exists so bundles can carry a real Ed25519
        // signature later without a format change; an unverifiable signature
        // is rejected rather than ignored.
        if let Some(signature) = &manifest.signature {
            let mut canonical = String::new();
            for file in &manifest.files {
                canonical.push_str(&format!("{}:{}:{}\n", file.path, file.sha256, file.size));
            }
            let expected = crate::manifest::sha256_bytes(canonical.as_bytes());
            if signature.trim_start_matches("sha256:") != expected {
                return Err(InstallError::BadSignature);
            }
        }
        Ok(())
    }

    fn commit(
        &self,
        manifest: &PackageManifest,
        stage: &Path,
        files: Vec<String>,
        registry: &mut Registry,
    ) -> Result<InstallOutcome, InstallError> {
        let target = if manifest.kind == PackageKind::AppImage {
            self.paths.app_file(&manifest.id)
        } else {
            self.paths.app_dir(&manifest.id)
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
        }
        let backup = target.with_extension("old");
        if target.exists() {
            let _ = fs::remove_dir_all(&backup);
            fs::rename(&target, &backup).map_err(|e| InstallError::io(&target, e))?;
        }
        let moved = if manifest.kind == PackageKind::AppImage {
            fs::rename(stage, &target)
        } else {
            fs::rename(stage, &target)
        };
        if let Err(error) = moved {
            // Roll back the previous version.
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(InstallError::io(&target, error));
        }
        let _ = fs::remove_dir_all(&backup);
        let _ = fs::set_permissions(
            &target,
            fs::metadata(&target)
                .map(|m| m.permissions())
                .unwrap_or_else(|_| fs::metadata(".").unwrap().permissions()),
        );

        registry.apps.insert(
            manifest.id.as_str().to_string(),
            InstalledApp {
                manifest: manifest.clone(),
                target: target.clone(),
                installed_at: now_secs(),
                files,
            },
        );
        if !self.dry_run {
            registry.save(&self.paths.registry)?;
        }
        // App data folder is the only writable location by default.
        let _ = fs::create_dir_all(self.paths.data_dir(&manifest.id));
        Ok(InstallOutcome::Installed(manifest.id.clone()))
    }
}

impl PackageInstaller for Installer {
    fn inspect(&self, file: &Path) -> Result<PackageManifest, InstallError> {
        if file.is_dir() {
            if crate::manifest::manifest_path(file).exists() {
                return Ok(read_manifest(file)?);
            }
            // A `.app` directory is a macOS bundle.
            if file.extension().and_then(|e| e.to_str()) == Some("app") {
                return Ok(macos_manifest(file)?);
            }
            return Err(InstallError::Unsupported(
                "directory is not an AetherOS bundle (no aether-manifest.toml)".into(),
            ));
        }
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.ends_with(".appimage") {
            return Ok(appimage_manifest(file)?);
        }
        if name.ends_with(".exe") || name.ends_with(".msi") {
            return Ok(windows_manifest(file)?);
        }
        if name.ends_with(".dmg") || name.ends_with(".pkg") {
            return Ok(macos_manifest(file)?);
        }
        Err(InstallError::Unsupported(format!(
            "unknown package type: {}",
            file.display()
        )))
    }

    fn install(
        &self,
        manifest: &PackageManifest,
        file: &Path,
    ) -> Result<InstallOutcome, InstallError> {
        if manifest.kind == PackageKind::MacosPackage {
            return Ok(self.unsupported(manifest.kind));
        }
        let missing: Vec<Permission> = missing_permissions(manifest, &self.granted)
            .into_iter()
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Ok(InstallOutcome::NeedsPermission(missing));
        }
        self.verify_signature(manifest)?;

        let mut registry = self.registry();
        let stage_root = self.paths.staging.join(manifest.id.as_str());
        let _ = fs::remove_dir_all(&stage_root);
        fs::create_dir_all(&self.paths.staging)
            .map_err(|e| InstallError::io(&self.paths.staging, e))?;

        let installed = (|| -> Result<InstallOutcome, InstallError> {
            match manifest.kind {
            PackageKind::NativeBundle | PackageKind::CrossPlatformBundle => {
                let files = self.copy_tree(file, &stage_root)?;
                let total = verify_files(&stage_root, manifest)?;
                if total != manifest.installed_bytes {
                    let _ = fs::remove_dir_all(&stage_root);
                    return Err(InstallError::ChecksumMismatch);
                }
                if !stage_root.join(&manifest.entry).exists() {
                    let _ = fs::remove_dir_all(&stage_root);
                    return Err(InstallError::Manifest(
                        crate::manifest::ManifestError::MissingFile(manifest.entry.clone()),
                    ));
                }
                let result = self.commit(manifest, &stage_root, files, &mut registry);
                result
            }
            PackageKind::AppImage => {
                fs::create_dir_all(&stage_root)
                    .map_err(|e| InstallError::io(&stage_root, e))?;
                let staged = stage_root.join(&manifest.entry);
                fs::copy(file, &staged).map_err(|e| InstallError::io(&staged, e))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755));
                }
                let result = self.commit(
                    manifest,
                    &stage_root,
                    vec![manifest.entry.clone()],
                    &mut registry,
                );
                result
            }
            PackageKind::WindowsInstaller => {
                // Keep the installer payload in the app folder; the prefix is
                // created by the Windows engine manager (P6-T05).
                let files = if file.is_dir() {
                    self.copy_tree(file, &stage_root)?
                } else {
                    fs::create_dir_all(&stage_root)
                        .map_err(|e| InstallError::io(&stage_root, e))?;
                    let name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| manifest.entry.clone());
                    let staged = stage_root.join(&name);
                    fs::copy(file, &staged)
                        .map_err(|e| InstallError::io(&staged, e))?;
                    vec![name]
                };
                let _ = fs::create_dir_all(self.paths.prefix_dir(&manifest.id));
                self.commit(manifest, &stage_root, files, &mut registry)
            }
            PackageKind::MacosPackage => Ok(self.unsupported(manifest.kind)),
            }
        })();
        if installed.is_err() {
            let _ = fs::remove_dir_all(&stage_root);
        }
        installed
    }

    fn uninstall(&self, id: &AppId) -> Result<(), InstallError> {
        let mut registry = self.registry();
        let Some(entry) = registry.get(id).cloned() else {
            return Err(InstallError::Unsupported(format!("{id} is not installed")));
        };
        if !self.dry_run {
            if entry.target.is_dir() {
                fs::remove_dir_all(&entry.target)
                    .map_err(|e| InstallError::io(&entry.target, e))?;
            } else if entry.target.exists() {
                fs::remove_file(&entry.target)
                    .map_err(|e| InstallError::io(&entry.target, e))?;
            }
            let _ = fs::remove_dir_all(self.paths.data_dir(id).parent().unwrap_or(Path::new("/")));
            let _ = fs::remove_dir_all(self.paths.prefix_dir(id));
            let _ = fs::remove_dir_all(self.paths.staging.join(id.as_str()));
        }
        registry.apps.remove(id.as_str());
        if !self.dry_run {
            registry.save(&self.paths.registry)?;
        }
        Ok(())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn windows_manifest(file: &Path) -> Result<PackageManifest, InstallError> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "windows-app.exe".into());
    let stem = name
        .trim_end_matches(".exe")
        .trim_end_matches(".msi")
        .to_string();
    let id = AppId::new(format!("win.{}", stem.replace(' ', "-")))
        .map_err(ManifestError::Identity)?;
    let size = fs::metadata(file)
        .map_err(|e| InstallError::io(file, e))?
        .len();
    Ok(PackageManifest {
        id,
        name: stem,
        version: "1.0.0".into(),
        kind: PackageKind::WindowsInstaller,
        permissions: vec![
            Permission::Files {
                scope: crate::manifest::FileScope::AppFolder,
            },
            Permission::Network,
        ],
        entry: name,
        installed_bytes: size,
        signature: None,
        files: vec![FileEntry {
            path: file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            sha256: crate::manifest::sha256_file(file)?,
            size,
        }],
        description: Some("Windows installer (runs through the Windows engine)".into()),
        homepage: None,
    })
}

fn macos_manifest(file: &Path) -> Result<PackageManifest, InstallError> {
    let name = file
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "macos-app".into());
    let id = AppId::new(format!("mac.{}", name.replace(' ', "-")))
        .map_err(ManifestError::Identity)?;
    Ok(PackageManifest {
        id,
        name,
        version: "0.0.0".into(),
        kind: PackageKind::MacosPackage,
        permissions: vec![],
        entry: file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        installed_bytes: 0,
        signature: None,
        files: vec![],
        description: Some("macOS package; cannot run on AetherOS".into()),
        homepage: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{compute_files, write_manifest};
    use std::io::Write;

    fn temp_root(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-pkg-{tag}-{}-{}-{}",
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

    fn make_bundle(root: &Path, id: &str, permissions: Vec<Permission>) -> PackageManifest {
        let bundle = root.join("bundle");
        write(&bundle.join("bin/app"), "#!/bin/sh\necho hello\n");
        write(&bundle.join("share/icon.svg"), "<svg/>");
        let files = compute_files(&bundle).unwrap();
        let manifest = PackageManifest {
            id: AppId::new(id).unwrap(),
            name: "Hello".into(),
            version: "1.0.0".into(),
            kind: PackageKind::NativeBundle,
            permissions,
            entry: "bin/app".into(),
            installed_bytes: files.iter().map(|f| f.size).sum(),
            signature: None,
            files,
            description: None,
            homepage: None,
        };
        write_manifest(&bundle, &manifest).unwrap();
        manifest
    }

    fn installer(root: &Path, granted: &[&str]) -> Installer {
        Installer::new(
            Paths::at(root.join("store")),
            granted.iter().map(|p| p.to_string()).collect(),
        )
    }

    #[test]
    fn installs_launches_and_uninstalls_native_bundle() {
        let root = temp_root("native");
        let manifest = make_bundle(&root, "org.example.Hello", vec![]);
        let installer = installer(&root, &[]);
        let bundle = root.join("bundle");

        let inspected = installer.inspect(&bundle).unwrap();
        assert_eq!(inspected.id, manifest.id);
        let outcome = installer.install(&inspected, &bundle).unwrap();
        assert_eq!(outcome, InstallOutcome::Installed(manifest.id.clone()));

        let target = installer.paths.app_dir(&manifest.id);
        assert!(target.join("bin/app").exists());
        assert!(target.join("share/icon.svg").exists());
        assert!(installer.paths.data_dir(&manifest.id).is_dir());
        assert!(installer.registry().get(&manifest.id).is_some());

        installer.uninstall(&manifest.id).unwrap();
        assert!(!target.exists());
        assert!(!installer.paths.data_dir(&manifest.id).exists());
        assert!(installer.registry().get(&manifest.id).is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn permissions_are_requested_before_install() {
        let root = temp_root("perm");
        let manifest = make_bundle(&root, "org.example.Net", vec![Permission::Network]);
        let mut installer = installer(&root, &[]);
        let outcome = installer
            .install(&manifest, &root.join("bundle"))
            .unwrap();
        assert_eq!(
            outcome,
            InstallOutcome::NeedsPermission(vec![Permission::Network])
        );
        assert!(installer.registry().get(&manifest.id).is_none());

        installer.grant(&Permission::Network);
        let outcome = installer
            .install(&manifest, &root.join("bundle"))
            .unwrap();
        assert_eq!(outcome, InstallOutcome::Installed(manifest.id.clone()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checksum_mismatch_aborts_and_cleans_staging() {
        let root = temp_root("tamper");
        let manifest = make_bundle(&root, "org.example.Tamper", vec![]);
        let installer = installer(&root, &[]);
        let bundle = root.join("bundle");
        write(&bundle.join("bin/app"), "#!/bin/sh\necho tampered\n");
        let result = installer.install(&manifest, &bundle);
        assert!(result.is_err());
        assert!(installer.registry().get(&manifest.id).is_none());
        assert!(!installer.paths.staging.join("org.example.Tamper").exists());
        assert!(!installer.paths.app_dir(&manifest.id).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_upgrades_replace_the_previous_version() {
        let root = temp_root("upgrade");
        let manifest = make_bundle(&root, "org.example.Up", vec![]);
        let installer = installer(&root, &[]);
        let bundle = root.join("bundle");
        installer.install(&manifest, &bundle).unwrap();
        let target = installer.paths.app_dir(&manifest.id);
        assert!(target.exists());

        let mut upgraded = manifest.clone();
        upgraded.version = "2.0.0".into();
        write_manifest(&bundle, &upgraded).unwrap();
        installer.install(&upgraded, &bundle).unwrap();
        let entry = installer.registry();
        let app = entry.get(&manifest.id).unwrap();
        assert_eq!(app.manifest.version, "2.0.0");
        assert!(!target.with_extension("old").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn appimage_install_sets_executable_bit() {
        let root = temp_root("appimage");
        let path = root.join("Tool.AppImage");
        fs::write(&path, "binary").unwrap();
        let installer = installer(&root, &["network", "files:pickedbyuser"]);
        let manifest = installer.inspect(&path).unwrap();
        assert_eq!(
            installer.install(&manifest, &path).unwrap(),
            InstallOutcome::Installed(manifest.id.clone())
        );
        let target = installer.paths.app_file(&manifest.id);
        assert!(target.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn macos_packages_are_rejected_with_alternatives() {
        let root = temp_root("macos");
        let path = root.join("Thing.dmg");
        fs::write(&path, "dmg").unwrap();
        let installer = installer(&root, &[]);
        let manifest = installer.inspect(&path).unwrap();
        assert_eq!(manifest.kind, PackageKind::MacosPackage);
        match installer.install(&manifest, &path).unwrap() {
            InstallOutcome::Unsupported { reason, alternatives } => {
                assert!(reason.contains("cannot run"));
                assert!(!alternatives.is_empty());
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn signature_field_is_verified_when_present() {
        let root = temp_root("signature");
        let mut manifest = make_bundle(&root, "org.example.Signed", vec![]);
        let installer = installer(&root, &[]);
        let bundle = root.join("bundle");
        manifest.signature = Some("sha256:deadbeef".into());
        assert!(matches!(
            installer.install(&manifest, &bundle),
            Err(InstallError::BadSignature)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_survives_reload() {
        let root = temp_root("registry");
        let manifest = make_bundle(&root, "org.example.Persist", vec![]);
        let installer = installer(&root, &[]);
        installer
            .install(&manifest, &root.join("bundle"))
            .unwrap();
        let reloaded = Registry::load(&installer.paths.registry);
        assert!(reloaded.get(&manifest.id).is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn inspect_an_arbitrary_directory_is_rejected() {
        let root = temp_root("notbundle");
        let installer = installer(&root, &[]);
        assert!(matches!(
            installer.inspect(&root),
            Err(InstallError::Unsupported(_))
        ));
        let _ = fs::remove_dir_all(root);
    }
}
