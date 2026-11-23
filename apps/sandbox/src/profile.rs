//! Sandbox profiles built from manifest permissions (P6-T04, D8 section 5).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use aether_pkg::manifest::{FileScope, PackageManifest, Permission};

/// Filesystem, network and device access granted to one app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProfile {
    pub file_read: Vec<PathBuf>,
    pub file_write: Vec<PathBuf>,
    pub network: bool,
    pub devices: Vec<PathBuf>,
    pub home: PathBuf,
}

/// Builds profiles from the persisted permission grants, never from the
/// manifest alone.
#[derive(Debug, Clone)]
pub struct Sandbox {
    granted: BTreeSet<String>,
    app_data_dir: PathBuf,
    picked_files: Vec<PathBuf>,
    home: PathBuf,
}

impl Sandbox {
    pub fn new(
        granted: impl IntoIterator<Item = String>,
        app_data_dir: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
    ) -> Self {
        Self {
            granted: granted.into_iter().collect(),
            app_data_dir: app_data_dir.into(),
            picked_files: Vec::new(),
            home: home.into(),
        }
    }

    /// Files returned by the file picker for this launch.
    pub fn with_picked_files(mut self, files: impl IntoIterator<Item = PathBuf>) -> Self {
        self.picked_files = files.into_iter().collect();
        self
    }

    fn has(&self, key: &str) -> bool {
        self.granted.contains(key)
    }

    /// Base runtime directories every app needs to start.
    fn base_read() -> Vec<PathBuf> {
        ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/run", "/dev"]
            .iter()
            .map(PathBuf::from)
            .collect()
    }

    pub fn profile_for(&self, manifest: &PackageManifest) -> SandboxProfile {
        let mut file_read = Self::base_read();
        let mut file_write = vec![self.app_data_dir.clone()];
        let mut devices = Vec::new();

        for permission in &manifest.permissions {
            if !self.has(&permission.key()) {
                // Denied stays denied: the profile never widens because the
                // manifest asked for something.
                continue;
            }
            match permission {
                Permission::Network => {}
                Permission::Files { scope } => match scope {
                    FileScope::PickedByUser => {
                        file_read.extend(self.picked_files.iter().cloned());
                    }
                    FileScope::AppFolder => {
                        // Already writable by default.
                    }
                    FileScope::Home => {
                        file_read.push(self.home.clone());
                        file_write.push(self.home.clone());
                    }
                },
                Permission::Camera => {
                    for index in 0..4 {
                        devices.push(PathBuf::from(format!("/dev/video{index}")));
                    }
                }
                Permission::Microphone => devices.push(PathBuf::from("/dev/snd")),
                Permission::Location | Permission::Notifications | Permission::Background => {}
            }
        }

        file_read.sort();
        file_read.dedup();
        file_write.sort();
        file_write.dedup();
        devices.sort();
        devices.dedup();

        SandboxProfile {
            file_read,
            file_write,
            network: self.has("network"),
            devices,
            home: self.app_data_dir.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_pkg::{AppId, PackageKind};

    fn manifest(permissions: Vec<Permission>) -> PackageManifest {
        PackageManifest {
            id: AppId::new("org.example.App").unwrap(),
            name: "App".into(),
            version: "1".into(),
            kind: PackageKind::NativeBundle,
            permissions,
            entry: "app".into(),
            installed_bytes: 0,
            signature: None,
            files: vec![],
            description: None,
            homepage: None,
        }
    }

    #[test]
    fn network_is_off_unless_granted() {
        let manifest = manifest(vec![Permission::Network]);
        let sandbox = Sandbox::new(Vec::new(), "/data/app", "/home/user");
        assert!(!sandbox.profile_for(&manifest).network);

        let sandbox = Sandbox::new(["network".to_string()], "/data/app", "/home/user");
        assert!(sandbox.profile_for(&manifest).network);
    }

    #[test]
    fn write_access_is_the_app_folder_only() {
        let manifest = manifest(vec![
            Permission::Files {
                scope: FileScope::AppFolder,
            },
            Permission::Network,
        ]);
        let sandbox = Sandbox::new(["network".to_string()], "/data/app", "/home/user");
        let profile = sandbox.profile_for(&manifest);
        assert_eq!(profile.file_write, vec![PathBuf::from("/data/app")]);
        assert!(profile.file_read.contains(&PathBuf::from("/usr")));
    }

    #[test]
    fn picked_files_are_read_only() {
        let manifest = manifest(vec![Permission::Files {
            scope: FileScope::PickedByUser,
        }]);
        let sandbox = Sandbox::new(
            ["files:pickedbyuser".to_string()],
            "/data/app",
            "/home/user",
        )
        .with_picked_files([PathBuf::from("/home/user/Documents/report.pdf")]);
        let profile = sandbox.profile_for(&manifest);
        assert!(profile
            .file_read
            .contains(&PathBuf::from("/home/user/Documents/report.pdf")));
        assert_eq!(profile.file_write, vec![PathBuf::from("/data/app")]);
    }

    #[test]
    fn denied_home_access_is_not_added() {
        let manifest = manifest(vec![Permission::Files {
            scope: FileScope::Home,
        }]);
        let sandbox = Sandbox::new(Vec::new(), "/data/app", "/home/user");
        let profile = sandbox.profile_for(&manifest);
        assert!(!profile.file_read.contains(&PathBuf::from("/home/user")));
        assert!(!profile.file_write.contains(&PathBuf::from("/home/user")));
    }

    #[test]
    fn devices_follow_permissions() {
        let manifest = manifest(vec![Permission::Camera, Permission::Microphone]);
        let sandbox = Sandbox::new(
            ["camera".to_string(), "microphone".to_string()],
            "/data/app",
            "/home/user",
        );
        let profile = sandbox.profile_for(&manifest);
        assert!(profile.devices.contains(&PathBuf::from("/dev/video0")));
        assert!(profile.devices.contains(&PathBuf::from("/dev/snd")));
    }
}
