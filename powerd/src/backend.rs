//! Mechanism implementations for powerd (D5 sections 2, 8 and 9).
//!
//! Profiles go through power-profiles-daemon (ADR-014) with the ACPI platform
//! profile as a fallback; sleep goes through systemd targets.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::{
    BackgroundPolicy, BackgroundPolicySink, PowerError, PowerProfile, ProfileBackend,
    SleepController,
};

/// `powerprofilesctl` backend (power-profiles-daemon).
#[derive(Debug)]
pub struct PowerProfilesCli {
    binary: PathBuf,
    dry_run: bool,
    last: std::sync::Mutex<Option<PowerProfile>>,
}

impl PowerProfilesCli {
    pub fn system() -> Self {
        Self::at("powerprofilesctl", false)
    }

    pub fn at(binary: impl Into<PathBuf>, dry_run: bool) -> Self {
        Self {
            binary: binary.into(),
            dry_run,
            last: std::sync::Mutex::new(None),
        }
    }
}

impl ProfileBackend for PowerProfilesCli {
    fn apply(&self, profile: PowerProfile) -> Result<(), PowerError> {
        if self.dry_run {
            *self.last.lock().unwrap() = Some(profile);
            return Ok(());
        }
        let status = Command::new(&self.binary)
            .args(["set", profile.as_str()])
            .status()
            .map_err(|e| PowerError::Backend(format!("powerprofilesctl: {e}")))?;
        if !status.success() {
            return Err(PowerError::Backend(format!(
                "powerprofilesctl set {} failed",
                profile.as_str()
            )));
        }
        *self.last.lock().unwrap() = Some(profile);
        Ok(())
    }

    fn current(&self) -> Result<Option<PowerProfile>, PowerError> {
        if let Some(profile) = *self.last.lock().unwrap() {
            return Ok(Some(profile));
        }
        if self.dry_run {
            return Ok(None);
        }
        let output = Command::new(&self.binary)
            .arg("get")
            .output()
            .map_err(|e| PowerError::Backend(format!("powerprofilesctl: {e}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(PowerProfile::from_str(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// ACPI platform profile fallback (`/sys/firmware/acpi/platform_profile`).
#[derive(Debug, Clone)]
pub struct PlatformProfile {
    path: PathBuf,
    dry_run: bool,
}

impl PlatformProfile {
    pub fn system() -> Self {
        Self::at(
            "/sys/firmware/acpi/platform_profile",
            false,
        )
    }

    pub fn at(path: impl Into<PathBuf>, dry_run: bool) -> Self {
        Self {
            path: path.into(),
            dry_run,
        }
    }

    fn value_for(profile: PowerProfile) -> &'static str {
        match profile {
            PowerProfile::Performance => "performance",
            PowerProfile::Balanced => "balanced",
            PowerProfile::Saver => "low-power",
        }
    }
}

impl ProfileBackend for PlatformProfile {
    fn apply(&self, profile: PowerProfile) -> Result<(), PowerError> {
        if self.dry_run {
            return Ok(());
        }
        fs::write(&self.path, Self::value_for(profile)).map_err(PowerError::Io)
    }

    fn current(&self) -> Result<Option<PowerProfile>, PowerError> {
        let text = fs::read_to_string(&self.path)?;
        Ok(PowerProfile::from_str(text.trim()))
    }
}

/// Tries power-profiles-daemon first, then the ACPI platform profile.
pub struct SystemProfileBackend {
    primary: PowerProfilesCli,
    fallback: PlatformProfile,
    applied: std::sync::Mutex<Option<PowerProfile>>,
}

impl SystemProfileBackend {
    pub fn system(dry_run: bool) -> Self {
        Self {
            primary: PowerProfilesCli::at("powerprofilesctl", dry_run),
            fallback: PlatformProfile::at("/sys/firmware/acpi/platform_profile", dry_run),
            applied: std::sync::Mutex::new(None),
        }
    }

    pub fn applied(&self) -> Option<PowerProfile> {
        *self.applied.lock().unwrap()
    }
}

impl ProfileBackend for SystemProfileBackend {
    fn apply(&self, profile: PowerProfile) -> Result<(), PowerError> {
        match self.primary.apply(profile) {
            Ok(()) => {}
            Err(primary_error) => self.fallback.apply(profile).map_err(|fallback_error| {
                PowerError::Backend(format!(
                    "profile backends failed: {primary_error}; {fallback_error}"
                ))
            })?,
        }
        *self.applied.lock().unwrap() = Some(profile);
        Ok(())
    }

    fn current(&self) -> Result<Option<PowerProfile>, PowerError> {
        if let Some(profile) = self.applied() {
            return Ok(Some(profile));
        }
        self.primary.current().or_else(|_| self.fallback.current())
    }
}

/// systemd sleep targets.
#[derive(Debug)]
pub struct SystemdSleep {
    binary: PathBuf,
    dry_run: bool,
    requested: std::sync::Mutex<Vec<&'static str>>,
}

impl SystemdSleep {
    pub fn system() -> Self {
        Self {
            binary: PathBuf::from("systemctl"),
            dry_run: false,
            requested: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn dry_run() -> Self {
        Self {
            binary: PathBuf::from("systemctl"),
            dry_run: true,
            requested: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn requested(&self) -> Vec<&'static str> {
        self.requested.lock().unwrap().clone()
    }

    fn run(&self, target: &'static str) -> Result<(), PowerError> {
        self.requested.lock().unwrap().push(target);
        if self.dry_run {
            return Ok(());
        }
        let status = Command::new(&self.binary)
            .arg(target)
            .status()
            .map_err(|e| PowerError::Backend(format!("systemctl {target}: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(PowerError::Backend(format!("systemctl {target} failed")))
        }
    }
}

impl SleepController for SystemdSleep {
    fn suspend(&self) -> Result<(), PowerError> {
        self.run("suspend")
    }

    fn hibernate(&self) -> Result<(), PowerError> {
        self.run("hibernate")
    }
}

/// Writes the background policy for `aether-lifecycled` to a runtime file.
/// The lifecycle daemon reads it on the next poll; a direct socket is a
/// follow-up integration task (documented in `powerd/README.md`).
#[derive(Debug, Clone)]
pub struct FileBackgroundSink {
    path: PathBuf,
}

impl FileBackgroundSink {
    pub fn system() -> Self {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/1000"))
            .join("aether");
        Self::at(dir.join("background-policy.json"))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl BackgroundPolicySink for FileBackgroundSink {
    fn set(&self, policy: BackgroundPolicy) -> Result<(), PowerError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&policy)
            .map_err(|e| PowerError::Backend(e.to_string()))?;
        fs::write(&self.path, text)?;
        Ok(())
    }
}

/// Read the background policy written by powerd (used by lifecycled on its
/// next poll; exposed here so the contract stays in one place).
pub fn read_background_policy(path: &Path) -> io::Result<BackgroundPolicy> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
