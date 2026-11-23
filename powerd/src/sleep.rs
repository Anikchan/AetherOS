//! Sleep policy (P5-T13, D5 section 8).
//!
//! Suspend-to-idle is the default; deep sleep is used only when the firmware
//! offers it; suspend-then-hibernate needs a disk swap file at least as large
//! as RAM (zram cannot hibernate).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SleepMode {
    /// `s2idle` — default.
    S2Idle,
    /// `deep` — only if firmware offers it.
    Deep,
}

impl SleepMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S2Idle => "s2idle",
            Self::Deep => "deep",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepConfig {
    pub prefer_deep: bool,
    pub suspend_then_hibernate: bool,
    /// Delay between suspend and hibernate.
    pub hibernate_delay_secs: u64,
    /// Swap file size in GiB, when one is configured.
    pub swap_gib: Option<u32>,
    /// RAM in GiB, used to require `swap >= RAM` for hibernation.
    pub ram_gib: u32,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            prefer_deep: false,
            suspend_then_hibernate: false,
            hibernate_delay_secs: 3 * 60 * 60,
            swap_gib: None,
            ram_gib: 8,
        }
    }
}

impl SleepConfig {
    /// Hibernation requires a disk swap file at least as large as RAM.
    pub fn hibernate_supported(&self) -> bool {
        self.swap_gib
            .map(|swap| swap >= self.ram_gib)
            .unwrap_or(false)
    }

    /// Effective behaviour for a lid close or low-battery action.
    pub fn sleep_action(&self) -> SleepAction {
        if self.suspend_then_hibernate && self.hibernate_supported() {
            SleepAction::SuspendThenHibernate
        } else {
            SleepAction::Suspend
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepAction {
    Suspend,
    SuspendThenHibernate,
    Hibernate,
}

#[derive(Debug, Error)]
pub enum SleepError {
    #[error("no supported sleep modes in {0}")]
    NoModes(String),
    #[error("mode {0} is not offered by the firmware")]
    ModeUnavailable(String),
    #[error("hibernation needs a disk swap file at least as large as RAM")]
    HibernationUnsupported,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Reads and writes `/sys/power/mem_sleep` and `/sys/power/state`.
#[derive(Debug, Clone)]
pub struct SleepManager {
    mem_sleep: PathBuf,
    power_state: PathBuf,
    dry_run: bool,
}

impl SleepManager {
    pub fn system() -> Self {
        Self {
            mem_sleep: PathBuf::from("/sys/power/mem_sleep"),
            power_state: PathBuf::from("/sys/power/state"),
            dry_run: false,
        }
    }

    pub fn at(mem_sleep: impl Into<PathBuf>, power_state: impl Into<PathBuf>) -> Self {
        Self {
            mem_sleep: mem_sleep.into(),
            power_state: power_state.into(),
            dry_run: false,
        }
    }

    pub fn dry_run() -> Self {
        Self {
            mem_sleep: PathBuf::from("/sys/power/mem_sleep"),
            power_state: PathBuf::from("/sys/power/state"),
            dry_run: true,
        }
    }

    pub fn mem_sleep_path(&self) -> &Path {
        &self.mem_sleep
    }

    /// Parse `"s2idle [deep]"` into the available modes and the current one.
    pub fn modes(&self) -> Result<(Vec<String>, Option<String>), SleepError> {
        let text = fs::read_to_string(&self.mem_sleep)?;
        let (modes, current) = parse_mem_sleep(&text);
        if modes.is_empty() {
            return Err(SleepError::NoModes(text));
        }
        Ok((modes, current))
    }

    /// Pick the mode to use: deep only when preferred and offered, else s2idle.
    pub fn choose_mode(&self, prefer_deep: bool) -> Result<SleepMode, SleepError> {
        let (modes, _) = self.modes()?;
        if prefer_deep && modes.iter().any(|m| m == "deep") {
            Ok(SleepMode::Deep)
        } else if modes.iter().any(|m| m == "s2idle") {
            Ok(SleepMode::S2Idle)
        } else if modes.iter().any(|m| m == "deep") {
            Ok(SleepMode::Deep)
        } else {
            Err(SleepError::NoModes(modes.join(" ")))
        }
    }

    pub fn select(&self, mode: SleepMode) -> Result<(), SleepError> {
        let (modes, _) = self.modes()?;
        if !modes.iter().any(|m| m == mode.as_str()) {
            return Err(SleepError::ModeUnavailable(mode.as_str().into()));
        }
        if !self.dry_run {
            fs::write(&self.mem_sleep, mode.as_str())?;
        }
        Ok(())
    }

    /// `disk` in `/sys/power/state` means the firmware/kernel offers hibernate.
    pub fn kernel_hibernate_available(&self) -> bool {
        fs::read_to_string(&self.power_state)
            .map(|text| text.split_whitespace().any(|s| s == "disk"))
            .unwrap_or(false)
    }
}

/// Parse `s2idle [deep]` style sysfs content.
pub fn parse_mem_sleep(text: &str) -> (Vec<String>, Option<String>) {
    let mut modes = Vec::new();
    let mut current = None;
    for token in text.split_whitespace() {
        let (name, selected) = if token.starts_with('[') && token.ends_with(']') {
            (token.trim_matches(['[', ']']), true)
        } else {
            (token, false)
        };
        if name.is_empty() {
            continue;
        }
        modes.push(name.to_string());
        if selected {
            current = Some(name.to_string());
        }
    }
    (modes, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-sleep-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_mem_sleep_output() {
        let (modes, current) = parse_mem_sleep("s2idle [deep]\n");
        assert_eq!(modes, vec!["s2idle", "deep"]);
        assert_eq!(current.as_deref(), Some("deep"));

        let (modes, current) = parse_mem_sleep("[s2idle]");
        assert_eq!(modes, vec!["s2idle"]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn chooses_deep_only_when_preferred_and_offered() {
        let dir = temp_dir();
        let mem = dir.join("mem_sleep");
        fs::write(&mem, "s2idle [deep]").unwrap();
        let manager = SleepManager::at(&mem, dir.join("state"));
        assert_eq!(manager.choose_mode(false).unwrap(), SleepMode::S2Idle);
        assert_eq!(manager.choose_mode(true).unwrap(), SleepMode::Deep);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn selecting_an_unavailable_mode_fails() {
        let dir = temp_dir();
        let mem = dir.join("mem_sleep");
        fs::write(&mem, "[s2idle]").unwrap();
        let manager = SleepManager::at(&mem, dir.join("state"));
        assert!(matches!(
            manager.select(SleepMode::Deep),
            Err(SleepError::ModeUnavailable(_))
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hibernate_needs_swap_at_least_ram() {
        let mut config = SleepConfig {
            ram_gib: 8,
            swap_gib: Some(8),
            suspend_then_hibernate: true,
            ..Default::default()
        };
        assert!(config.hibernate_supported());
        assert_eq!(config.sleep_action(), SleepAction::SuspendThenHibernate);

        config.swap_gib = Some(4);
        assert!(!config.hibernate_supported());
        assert_eq!(config.sleep_action(), SleepAction::Suspend);

        config.swap_gib = None;
        assert!(!config.hibernate_supported());
    }

    #[test]
    fn kernel_hibernate_flag_is_detected() {
        let dir = temp_dir();
        let state = dir.join("state");
        fs::write(&state, "freeze mem disk").unwrap();
        let manager = SleepManager::at(dir.join("mem_sleep"), &state);
        assert!(manager.kernel_hibernate_available());
        fs::write(&state, "freeze mem").unwrap();
        assert!(!manager.kernel_hibernate_available());
        let _ = fs::remove_dir_all(dir);
    }
}
