//! Runtime power-management tuner with exclusions and automatic revert
//! (P5-T10, D5 section 9).
//!
//! Safe list: PCIe ASPM, USB autosuspend, Wi-Fi power save, audio codec power
//! save and SATA link power. Each setting records its previous value before
//! writing. A device error within 10 minutes reverts that device, adds it to
//! the exclusion list and logs the event. More than 10 exclusions disable
//! tuning for the session.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// One tunable device setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunerRule {
    pub device: String,
    pub path: PathBuf,
    pub enabled_value: String,
}

impl TunerRule {
    pub fn new(device: impl Into<String>, path: impl Into<PathBuf>, enabled: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            path: path.into(),
            enabled_value: enabled.into(),
        }
    }
}

/// The standard safe list (paths exist on a typical AetherOS laptop).
pub fn default_rules() -> Vec<TunerRule> {
    vec![
        TunerRule::new(
            "pcie-aspm",
            "/sys/module/pcie_aspm/parameters/policy",
            "powersupersave",
        ),
        TunerRule::new(
            "sata-link-power",
            "/sys/class/scsi_host/host0/link_power_management_policy",
            "min_power",
        ),
        TunerRule::new(
            "audio-codec",
            "/sys/module/snd_hda_intel/parameters/power_save",
            "1",
        ),
        TunerRule::new(
            "usb-autosuspend",
            "/sys/bus/usb/devices/usb1/power/control",
            "auto",
        ),
        TunerRule::new(
            "wifi-powersave",
            "/sys/module/cfg80211/parameters/ieee80211_default_rc_algo",
            "minstrel_ht",
        ),
    ]
}

/// Device-level read/write used by the tuner.
pub trait TunerBackend {
    fn read(&self, path: &Path) -> io::Result<String>;
    fn write(&self, path: &Path, value: &str) -> io::Result<()>;
}

/// Real sysfs backend. `dry_run` skips writes (benchmarks, macOS smoke test).
#[derive(Debug, Clone, Copy)]
pub struct SysfsBackend {
    pub dry_run: bool,
}

impl SysfsBackend {
    pub fn system() -> Self {
        Self { dry_run: false }
    }

    pub fn dry_run() -> Self {
        Self { dry_run: true }
    }
}

impl TunerBackend for SysfsBackend {
    fn read(&self, path: &Path) -> io::Result<String> {
        Ok(fs::read_to_string(path)?.trim().to_string())
    }

    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        if self.dry_run {
            return Ok(());
        }
        fs::write(path, value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedSetting {
    old_value: String,
    applied_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TunerReport {
    pub applied: Vec<String>,
    pub reverted: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<(String, String)>,
    pub disabled: Option<String>,
}

/// Outcome of a reported device error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceErrorOutcome {
    /// Reverted and excluded.
    Reverted { device: String, old_value: String },
    /// Revered, excluded, and tuning is now disabled (too many exclusions).
    Disabled { reason: String },
    /// Error outside the watch window: logged only.
    Ignored { device: String },
}

pub struct Tuner {
    backend: Box<dyn TunerBackend>,
    rules: Vec<TunerRule>,
    exclusions: BTreeSet<String>,
    applied: BTreeMap<String, AppliedSetting>,
    max_exclusions: usize,
    error_window: Duration,
    disabled: Option<String>,
    log: Vec<String>,
}

impl Tuner {
    pub fn new(
        backend: impl TunerBackend + 'static,
        rules: Vec<TunerRule>,
        exclusions: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            backend: Box::new(backend),
            rules,
            exclusions: exclusions.into_iter().collect(),
            applied: BTreeMap::new(),
            max_exclusions: 10,
            error_window: Duration::from_secs(10 * 60),
            disabled: None,
            log: Vec::new(),
        }
    }

    pub fn with_limits(mut self, max_exclusions: usize, error_window: Duration) -> Self {
        self.max_exclusions = max_exclusions;
        self.error_window = error_window;
        self
    }

    pub fn exclusions(&self) -> &BTreeSet<String> {
        &self.exclusions
    }

    pub fn is_tuned(&self, device: &str) -> bool {
        self.applied.contains_key(device)
    }

    pub fn applied_devices(&self) -> Vec<String> {
        self.applied.keys().cloned().collect()
    }

    pub fn disabled(&self) -> Option<&str> {
        self.disabled.as_deref()
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    fn revert(&mut self, device: &str, report: &mut TunerReport) {
        if let Some(setting) = self.applied.remove(device) {
            let rule = self.rules.iter().find(|r| r.device == device);
            if let Some(rule) = rule {
                match self.backend.write(&rule.path, &setting.old_value) {
                    Ok(()) => report.reverted.push(device.to_string()),
                    Err(e) => report
                        .errors
                        .push((device.to_string(), format!("revert failed: {e}"))),
                }
            }
        }
    }

    /// Enable (Saver) or disable (Balanced/Performance) the safe list.
    pub fn set_enabled(&mut self, enabled: bool) -> TunerReport {
        let mut report = TunerReport {
            disabled: self.disabled.clone(),
            ..Default::default()
        };
        if self.disabled.is_some() {
            report.skipped.push("*".into());
            return report;
        }
        if !enabled {
            let devices: Vec<String> = self.applied.keys().cloned().collect();
            for device in devices {
                self.revert(&device, &mut report);
            }
            self.log.push("tuning disabled: reverted all devices".into());
            return report;
        }
        for rule in &self.rules {
            if self.exclusions.contains(&rule.device) {
                report.skipped.push(rule.device.clone());
                continue;
            }
            if self.applied.contains_key(&rule.device) {
                continue;
            }
            let old_value = match self.backend.read(&rule.path) {
                Ok(value) => value,
                Err(e) => {
                    report
                        .errors
                        .push((rule.device.clone(), format!("read failed: {e}")));
                    continue;
                }
            };
            match self.backend.write(&rule.path, &rule.enabled_value) {
                Ok(()) => {
                    self.applied.insert(
                        rule.device.clone(),
                        AppliedSetting {
                            old_value,
                            applied_at: Instant::now(),
                        },
                    );
                    report.applied.push(rule.device.clone());
                }
                Err(e) => report
                    .errors
                    .push((rule.device.clone(), format!("write failed: {e}"))),
            }
        }
        report
    }

    /// Watch window: an error within 10 minutes of tuning reverts and excludes.
    pub fn on_device_error(&mut self, device: &str, now: Instant) -> DeviceErrorOutcome {
        let Some(setting) = self.applied.get(device).cloned() else {
            return DeviceErrorOutcome::Ignored {
                device: device.to_string(),
            };
        };
        if now.saturating_duration_since(setting.applied_at) > self.error_window {
            self.log
                .push(format!("device error for {device} outside watch window; not reverting"));
            return DeviceErrorOutcome::Ignored {
                device: device.to_string(),
            };
        }

        let mut report = TunerReport::default();
        self.revert(device, &mut report);
        self.exclusions.insert(device.to_string());
        self.log.push(format!(
            "reverted {device} to {:?} and added to exclusions",
            setting.old_value
        ));

        if self.exclusions.len() > self.max_exclusions {
            let reason = format!(
                "exclusion list grew past {} devices; tuning disabled",
                self.max_exclusions
            );
            self.disabled = Some(reason.clone());
            let devices: Vec<String> = self.applied.keys().cloned().collect();
            for device in devices {
                let mut restore = TunerReport::default();
                self.revert(&device, &mut restore);
            }
            self.log.push(reason.clone());
            return DeviceErrorOutcome::Disabled { reason };
        }

        DeviceErrorOutcome::Reverted {
            device: device.to_string(),
            old_value: setting.old_value,
        }
    }

    /// Load exclusions from a TOML file (`~/.config/aether/power/exclusions.toml`).
    pub fn load_exclusions(path: &Path) -> io::Result<Vec<String>> {
        let text = fs::read_to_string(path)?;
        let parsed: ExclusionsFile = toml::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(parsed.exclude)
    }

    /// Save exclusions; keeps a single `exclude = [...]` array.
    pub fn save_exclusions(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = ExclusionsFile {
            exclude: self.exclusions.iter().cloned().collect(),
        };
        let text = toml::to_string(&file).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }

    /// Apply a `DeviceError` event and persist the exclusion list.
    pub fn handle_device_error(
        &mut self,
        device: &str,
        now: Instant,
        exclusions_path: &Path,
    ) -> DeviceErrorOutcome {
        let outcome = self.on_device_error(device, now);
        let _ = self.save_exclusions(exclusions_path);
        outcome
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ExclusionsFile {
    #[serde(default)]
    exclude: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeBackend {
        values: Arc<Mutex<HashMap<PathBuf, String>>>,
        writes: Arc<Mutex<Vec<(PathBuf, String)>>>,
        fail_writes: Arc<Mutex<BTreeSet<PathBuf>>>,
    }

    impl FakeBackend {
        fn set(&self, path: &str, value: &str) {
            self.values
                .lock()
                .unwrap()
                .insert(PathBuf::from(path), value.to_string());
        }
        fn value(&self, path: &str) -> String {
            self.values.lock().unwrap().get(Path::new(path)).cloned().unwrap_or_default()
        }
    }

    impl TunerBackend for FakeBackend {
        fn read(&self, path: &Path) -> io::Result<String> {
            self.values
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing"))
        }
        fn write(&self, path: &Path, value: &str) -> io::Result<()> {
            if self.fail_writes.lock().unwrap().contains(path) {
                return Err(io::Error::new(io::ErrorKind::Other, "write failed"));
            }
            self.values
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), value.to_string());
            self.writes
                .lock()
                .unwrap()
                .push((path.to_path_buf(), value.to_string()));
            Ok(())
        }
    }

    fn rules() -> Vec<TunerRule> {
        vec![
            TunerRule::new("pcie", "/sys/pcie", "powersupersave"),
            TunerRule::new("usb", "/sys/usb", "auto"),
        ]
    }

    #[test]
    fn enabling_writes_and_records_old_values() {
        let backend = FakeBackend::default();
        backend.set("/sys/pcie", "default");
        backend.set("/sys/usb", "on");
        let mut tuner = Tuner::new(backend.clone(), rules(), Vec::new());
        let report = tuner.set_enabled(true);
        assert_eq!(report.applied, vec!["pcie".to_string(), "usb".to_string()]);
        assert_eq!(backend.value("/sys/pcie"), "powersupersave");
        assert!(tuner.is_tuned("pcie"));
    }

    #[test]
    fn disabling_restores_previous_values() {
        let backend = FakeBackend::default();
        backend.set("/sys/pcie", "default");
        backend.set("/sys/usb", "on");
        let mut tuner = Tuner::new(backend.clone(), rules(), Vec::new());
        tuner.set_enabled(true);
        let report = tuner.set_enabled(false);
        assert_eq!(report.reverted.len(), 2);
        assert_eq!(backend.value("/sys/pcie"), "default");
        assert_eq!(backend.value("/sys/usb"), "on");
        assert!(!tuner.is_tuned("pcie"));
    }

    #[test]
    fn device_error_reverts_and_excludes() {
        let backend = FakeBackend::default();
        backend.set("/sys/pcie", "default");
        backend.set("/sys/usb", "on");
        let mut tuner = Tuner::new(backend.clone(), rules(), Vec::new());
        tuner.set_enabled(true);
        let applied_at = Instant::now();
        // Simulate the recorded time by using the real Instant::now close to it.
        let outcome = tuner.on_device_error("pcie", applied_at);
        assert!(matches!(outcome, DeviceErrorOutcome::Reverted { .. }));
        assert_eq!(backend.value("/sys/pcie"), "default");
        assert!(tuner.exclusions().contains("pcie"));

        // Re-applying skips the excluded device.
        let report = tuner.set_enabled(true);
        assert!(report.skipped.contains(&"pcie".to_string()));
        assert_eq!(backend.value("/sys/pcie"), "default");
    }

    #[test]
    fn errors_outside_the_window_do_not_exclude() {
        let backend = FakeBackend::default();
        backend.set("/sys/pcie", "default");
        let mut tuner = Tuner::new(backend, vec![rules()[0].clone()], Vec::new());
        tuner.set_enabled(true);
        let outcome = tuner.on_device_error("pcie", Instant::now() + Duration::from_secs(601));
        assert!(matches!(outcome, DeviceErrorOutcome::Ignored { .. }));
        assert!(!tuner.exclusions().contains("pcie"));
    }

    #[test]
    fn too_many_exclusions_disable_tuning() {
        let backend = FakeBackend::default();
        let mut all_rules = Vec::new();
        let mut exclusions = Vec::new();
        for index in 0..12 {
            let device = format!("dev{index}");
            let path = format!("/sys/dev{index}");
            backend.set(&path, "old");
            all_rules.push(TunerRule::new(device.clone(), path, "new"));
        }
        let mut tuner = Tuner::new(backend.clone(), all_rules, Vec::new())
            .with_limits(10, Duration::from_secs(600));
        tuner.set_enabled(true);
        let now = Instant::now();
        let mut disabled = false;
        for index in 0..12 {
            let outcome = tuner.on_device_error(&format!("dev{index}"), now);
            if matches!(outcome, DeviceErrorOutcome::Disabled { .. }) {
                disabled = true;
                exclusions = tuner.exclusions().iter().cloned().collect();
                break;
            }
        }
        assert!(disabled, "tuning should disable after 10 exclusions");
        assert_eq!(exclusions.len(), 11);
        assert!(tuner.disabled().is_some());
        // Further enable attempts are skipped.
        let report = tuner.set_enabled(true);
        assert_eq!(report.skipped, vec!["*".to_string()]);
    }

    #[test]
    fn write_failures_are_reported_not_fatal() {
        let backend = FakeBackend::default();
        backend.set("/sys/pcie", "default");
        backend.fail_writes.lock().unwrap().insert(PathBuf::from("/sys/pcie"));
        let mut tuner = Tuner::new(backend, vec![rules()[0].clone()], Vec::new());
        let report = tuner.set_enabled(true);
        assert_eq!(report.errors.len(), 1);
        assert!(report.applied.is_empty());
    }

    #[test]
    fn exclusions_round_trip_through_toml() {
        let backend = FakeBackend::default();
        backend.set("/sys/pcie", "default");
        let mut tuner = Tuner::new(backend, vec![rules()[0].clone()], Vec::new());
        tuner.set_enabled(true);
        tuner.on_device_error("pcie", Instant::now());
        let dir = std::env::temp_dir().join(format!("aether-tuner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("exclusions.toml");
        tuner.save_exclusions(&path).unwrap();
        let loaded = Tuner::load_exclusions(&path).unwrap();
        assert_eq!(loaded, vec!["pcie".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
