//! Sandbox enforcement (P6-T04, D8 section 5 failure mode).
//!
//! Enforcement uses bubblewrap (user + mount + pid + net namespaces). When the
//! tool or the kernel feature is missing, launch **stops with an explanation**;
//! an app is never started unsandboxed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::profile::SandboxProfile;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox cannot be applied: {0}")]
    Unavailable(String),
    #[error("the app entry point {0} does not exist")]
    MissingEntry(PathBuf),
    #[error("the app data folder {0} is not writable")]
    DataFolder(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCapabilities {
    pub bwrap: Option<PathBuf>,
    pub user_namespaces: bool,
    pub landlock: bool,
}

impl SandboxCapabilities {
    /// Probe the running system.
    pub fn detect() -> Self {
        Self {
            bwrap: find_in_path("bwrap"),
            user_namespaces: fs::read_to_string("/proc/sys/user/max_user_namespaces")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|v| v > 0)
                .unwrap_or(false),
            landlock: fs::read_to_string("/sys/kernel/security/lsm")
                .map(|text| text.split(',').any(|name| name.trim() == "landlock"))
                .unwrap_or(false),
        }
    }

    pub fn usable(&self) -> bool {
        self.bwrap.is_some() && self.user_namespaces
    }
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Builds the sandboxed launch command.
#[derive(Debug, Clone)]
pub struct Enforcer {
    bwrap: PathBuf,
    pub capabilities: SandboxCapabilities,
}

impl Enforcer {
    /// Detect the system and return an error when enforcement is impossible.
    pub fn system() -> Result<Self, SandboxError> {
        let capabilities = SandboxCapabilities::detect();
        let Some(bwrap) = capabilities.bwrap.clone() else {
            return Err(SandboxError::Unavailable(
                "bubblewrap (bwrap) is not installed; install it and retry. The app will not start unsandboxed."
                    .into(),
            ));
        };
        if !capabilities.user_namespaces {
            return Err(SandboxError::Unavailable(
                "user namespaces are disabled (kernel.unprivileged_userns_clone=0); the app will not start unsandboxed."
                    .into(),
            ));
        }
        Ok(Self {
            bwrap,
            capabilities,
        })
    }

    /// Test seam: use a specific bwrap binary and capabilities.
    pub fn with_bwrap(bwrap: impl Into<PathBuf>, capabilities: SandboxCapabilities) -> Self {
        Self {
            bwrap: bwrap.into(),
            capabilities,
        }
    }

    pub fn command(
        &self,
        profile: &SandboxProfile,
        entry: &Path,
    ) -> Result<Command, SandboxError> {
        if !entry.exists() {
            return Err(SandboxError::MissingEntry(entry.to_path_buf()));
        }
        if !profile.home.exists() {
            return Err(SandboxError::DataFolder(
                profile.home.display().to_string(),
            ));
        }

        let mut command = Command::new(&self.bwrap);
        command
            .arg("--die-with-parent")
            .arg("--new-session")
            .arg("--unshare-user")
            .arg("--unshare-pid")
            .arg("--unshare-ipc")
            .arg("--unshare-uts")
            .arg("--unshare-cgroup-try");
        if !profile.network {
            command.arg("--unshare-net");
        }
        command
            .arg("--proc")
            .arg("/proc")
            .arg("--dev")
            .arg("/dev")
            .arg("--tmpfs")
            .arg("/tmp");

        for path in &profile.file_read {
            if path.exists() {
                command.arg("--ro-bind").arg(path).arg(path);
            }
        }
        for path in &profile.file_write {
            if path.exists() {
                command.arg("--bind").arg(path).arg(path);
            }
        }
        for device in &profile.devices {
            if device.exists() {
                command.arg("--dev-bind").arg(device).arg(device);
            }
        }

        command
            .arg("--setenv")
            .arg("HOME")
            .arg(&profile.home)
            .arg("--setenv")
            .arg("XDG_DATA_HOME")
            .arg(&profile.home)
            .arg("--chdir")
            .arg(entry.parent().unwrap_or(Path::new("/")));
        command.arg(entry);
        Ok(command)
    }

    /// Human-readable explanation of the active restrictions.
    pub fn describe(&self, profile: &SandboxProfile) -> Vec<String> {
        let mut lines = vec![
            format!(
                "Network: {}",
                if profile.network {
                    "allowed (permission granted)"
                } else {
                    "blocked"
                }
            ),
            "Filesystem: read-only system directories, writable app data folder".into(),
        ];
        if !profile.devices.is_empty() {
            lines.push(format!(
                "Devices: {}",
                profile
                    .devices
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines.push(format!(
            "Landlock available: {} (namespace enforcement via bwrap)",
            self.capabilities.landlock
        ));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-sbx-{tag}-{}-{}-{}",
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

    fn profile(network: bool, app: &Path) -> SandboxProfile {
        SandboxProfile {
            file_read: vec![PathBuf::from("/usr")],
            file_write: vec![app.to_path_buf()],
            network,
            devices: vec![],
            home: app.to_path_buf(),
        }
    }

    #[test]
    fn command_contains_expected_restrictions() {
        let dir = temp_dir("cmd");
        let app = dir.join("app-data");
        fs::create_dir_all(&app).unwrap();
        let entry = dir.join("app");
        let mut file = fs::File::create(&entry).unwrap();
        writeln!(file, "#!/bin/sh\ntrue").unwrap();

        let enforcer = Enforcer::with_bwrap(
            "/usr/bin/bwrap",
            SandboxCapabilities {
                bwrap: Some(PathBuf::from("/usr/bin/bwrap")),
                user_namespaces: true,
                landlock: true,
            },
        );
        let command = enforcer.command(&profile(false, &app), &entry).unwrap();
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--unshare-net".to_string()));
        assert!(args.contains(&"--unshare-user".to_string()));
        assert!(args.contains(&"--bind".to_string()));
        assert!(args.contains(&"--ro-bind".to_string()));
        assert_eq!(args.last().unwrap(), &entry.to_string_lossy());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn network_permission_removes_unshare_net() {
        let dir = temp_dir("net");
        let app = dir.join("app-data");
        fs::create_dir_all(&app).unwrap();
        let entry = dir.join("app");
        fs::write(&entry, "#!/bin/sh\ntrue").unwrap();
        let enforcer = Enforcer::with_bwrap(
            "/usr/bin/bwrap",
            SandboxCapabilities {
                bwrap: Some(PathBuf::from("/usr/bin/bwrap")),
                user_namespaces: true,
                landlock: false,
            },
        );
        let command = enforcer.command(&profile(true, &app), &entry).unwrap();
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(!args.contains(&"--unshare-net".to_string()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_entry_stops_launch() {
        let dir = temp_dir("missing");
        let app = dir.join("app-data");
        fs::create_dir_all(&app).unwrap();
        let enforcer = Enforcer::with_bwrap(
            "/usr/bin/bwrap",
            SandboxCapabilities {
                bwrap: None,
                user_namespaces: true,
                landlock: false,
            },
        );
        assert!(matches!(
            enforcer.command(&profile(false, &app), &dir.join("nope")),
            Err(SandboxError::MissingEntry(_))
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn system_without_bwrap_refuses_to_launch() {
        // On macOS there is no bwrap; the error must be explicit.
        if find_in_path("bwrap").is_none() {
            assert!(matches!(
                Enforcer::system(),
                Err(SandboxError::Unavailable(_))
            ));
        }
    }

    #[test]
    fn description_mentions_network_state() {
        let dir = temp_dir("desc");
        let app = dir.join("app-data");
        fs::create_dir_all(&app).unwrap();
        let enforcer = Enforcer::with_bwrap(
            "bwrap",
            SandboxCapabilities {
                bwrap: None,
                user_namespaces: false,
                landlock: false,
            },
        );
        let lines = enforcer.describe(&profile(false, &app));
        assert!(lines.iter().any(|line| line.contains("blocked")));
        let _ = fs::remove_dir_all(dir);
    }
}
