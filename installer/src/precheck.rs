//! Windows pre-checks before resizing (P8-T07, F5 section 4).
//!
//! Every blocking check must pass before the plan may be applied. Checks are
//! deterministic and testable; the UI shows the same list.

use serde::{Deserialize, Serialize};

use crate::disk::WindowsInfo;

pub const MIN_FREE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const RECOMMENDED_FREE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreCheck {
    pub id: &'static str,
    pub title: String,
    pub status: CheckStatus,
    pub detail: String,
    /// True when the user can accept the warning and continue.
    pub acknowledgeable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallMode {
    Alongside,
    Erase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreCheckInput {
    pub windows: WindowsInfo,
    pub mode: InstallMode,
    pub backup_confirmed: bool,
    pub ac_power: bool,
    pub battery_percent: Option<u8>,
    pub free_bytes: u64,
    pub ram_gib: u32,
}

impl Default for PreCheckInput {
    fn default() -> Self {
        Self {
            windows: WindowsInfo::default(),
            mode: InstallMode::Alongside,
            backup_confirmed: false,
            ac_power: true,
            battery_percent: Some(100),
            free_bytes: MIN_FREE_BYTES,
            ram_gib: 8,
        }
    }
}

pub fn run(input: &PreCheckInput) -> Vec<PreCheck> {
    let mut checks = Vec::new();

    checks.push(PreCheck {
        id: "backup",
        title: "Backup done".into(),
        status: if input.mode == InstallMode::Alongside && !input.backup_confirmed {
            CheckStatus::Block
        } else {
            CheckStatus::Pass
        },
        detail: if input.mode == InstallMode::Alongside {
            "Partition resizing can fail. Confirm a backup exists before continuing.".into()
        } else {
            "Erase install does not resize Windows, but a backup is still wise.".into()
        },
        acknowledgeable: false,
    });

    if input.windows.present {
        checks.push(PreCheck {
            id: "bitlocker",
            title: "BitLocker recovery key".into(),
            status: if input.windows.bitlocker && !input.windows.recovery_key_saved {
                CheckStatus::Block
            } else if input.windows.bitlocker {
                CheckStatus::Pass
            } else {
                CheckStatus::Pass
            },
            detail: if input.windows.bitlocker {
                "BitLocker is on. Save the recovery key before resizing.".into()
            } else {
                "BitLocker is not detected.".into()
            },
            acknowledgeable: false,
        });

        checks.push(PreCheck {
            id: "fast-startup",
            title: "Fast Startup off".into(),
            status: if input.windows.fast_startup {
                CheckStatus::Block
            } else {
                CheckStatus::Pass
            },
            detail: if input.windows.fast_startup {
                "Windows Fast Startup keeps NTFS locked. Turn it off in Windows and restart."
                    .into()
            } else {
                "Fast Startup is off (or Windows is not installed).".into()
            },
            acknowledgeable: false,
        });

        checks.push(PreCheck {
            id: "ntfs",
            title: "NTFS health".into(),
            status: if input.windows.ntfs_clean {
                CheckStatus::Pass
            } else {
                CheckStatus::Block
            },
            detail: if input.windows.ntfs_clean {
                "The Windows volume reports clean.".into()
            } else {
                "The Windows volume has errors. Run chkdsk in Windows first.".into()
            },
            acknowledgeable: false,
        });

        checks.push(PreCheck {
            id: "updates",
            title: "Pending Windows updates".into(),
            status: if input.windows.pending_updates {
                CheckStatus::Warn
            } else {
                CheckStatus::Pass
            },
            detail: if input.windows.pending_updates {
                "Finish Windows updates; a feature update can reset the boot order.".into()
            } else {
                "No pending updates reported.".into()
            },
            acknowledgeable: true,
        });
    }

    checks.push(PreCheck {
        id: "space",
        title: "Free space".into(),
        status: if input.free_bytes >= MIN_FREE_BYTES {
            CheckStatus::Pass
        } else {
            CheckStatus::Block
        },
        detail: format!(
            "{} available; {} required, {} recommended.",
            crate::disk::human_size(input.free_bytes),
            crate::disk::human_size(MIN_FREE_BYTES),
            crate::disk::human_size(RECOMMENDED_FREE_BYTES)
        ),
        acknowledgeable: false,
    });

    checks.push(PreCheck {
        id: "power",
        title: "Power and battery".into(),
        status: if input.ac_power {
            CheckStatus::Pass
        } else if input.battery_percent.unwrap_or(100) < 20 {
            CheckStatus::Block
        } else {
            CheckStatus::Warn
        },
        detail: if input.ac_power {
            "On AC power.".into()
        } else {
            "Connect AC power; a power loss during a resize is dangerous.".into()
        },
        acknowledgeable: false,
    });

    if input.ram_gib > 64 {
        checks.push(PreCheck {
            id: "swap-file",
            title: "Swap file".into(),
            status: CheckStatus::Warn,
            detail: "Hibernation needs a swap file at least as large as RAM.".into(),
            acknowledgeable: true,
        });
    }

    checks
}

pub fn blocking(checks: &[PreCheck]) -> Vec<&PreCheck> {
    checks
        .iter()
        .filter(|check| check.status == CheckStatus::Block)
        .collect()
}

pub fn can_proceed(checks: &[PreCheck]) -> bool {
    blocking(checks).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_windows() -> WindowsInfo {
        WindowsInfo {
            present: true,
            version: Some("Windows 11".into()),
            ..Default::default()
        }
    }

    #[test]
    fn clean_alongside_install_passes() {
        let input = PreCheckInput {
            windows: clean_windows(),
            backup_confirmed: true,
            ..Default::default()
        };
        let checks = run(&input);
        assert!(can_proceed(&checks));
        assert!(checks.iter().any(|check| check.id == "bitlocker"));
        assert!(checks.iter().any(|check| check.id == "space"));
    }

    #[test]
    fn missing_backup_blocks_alongside() {
        let input = PreCheckInput {
            windows: clean_windows(),
            backup_confirmed: false,
            ..Default::default()
        };
        assert!(!can_proceed(&run(&input)));
        assert!(blocking(&run(&input)).iter().any(|check| check.id == "backup"));
    }

    #[test]
    fn fast_startup_blocks_and_clearing_it_passes() {
        let mut windows = clean_windows();
        windows.fast_startup = true;
        let blocked = run(&PreCheckInput {
            windows: windows.clone(),
            backup_confirmed: true,
            ..Default::default()
        });
        assert!(!can_proceed(&blocked));

        windows.fast_startup = false;
        let passed = run(&PreCheckInput {
            windows,
            backup_confirmed: true,
            ..Default::default()
        });
        assert!(can_proceed(&passed));
    }

    #[test]
    fn bitlocker_without_recovery_key_blocks() {
        let mut windows = clean_windows();
        windows.bitlocker = true;
        let checks = run(&PreCheckInput {
            windows: windows.clone(),
            backup_confirmed: true,
            ..Default::default()
        });
        assert!(!can_proceed(&checks));
        windows.recovery_key_saved = true;
        assert!(can_proceed(&run(&PreCheckInput {
            windows,
            backup_confirmed: true,
            ..Default::default()
        })));
    }

    #[test]
    fn low_battery_on_dc_blocks() {
        let input = PreCheckInput {
            windows: clean_windows(),
            backup_confirmed: true,
            ac_power: false,
            battery_percent: Some(10),
            ..Default::default()
        };
        assert!(!can_proceed(&run(&input)));
    }

    #[test]
    fn erase_install_does_not_require_backup_confirmation() {
        let input = PreCheckInput {
            windows: clean_windows(),
            mode: InstallMode::Erase,
            backup_confirmed: false,
            ..Default::default()
        };
        let checks = run(&input);
        assert!(checks
            .iter()
            .find(|check| check.id == "backup")
            .map(|check| check.status == CheckStatus::Pass)
            .unwrap_or(false));
    }
}
