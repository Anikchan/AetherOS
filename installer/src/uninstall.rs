//! Uninstall plan (P8-T06, F5 INS-09).
//!
//! Removes only AetherOS partitions and its boot entry; Windows partitions and
//! the Windows Boot Manager are untouched. Free space can grow the Windows
//! partition or stay unallocated.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::boot::BootEntry;
use crate::disk::{human_size, Disk, Partition};
use crate::planner::OperationKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UninstallMode {
    KeepUnallocated,
    GrowWindows,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallOperation {
    pub kind: OperationKind,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallPlan {
    pub disk: String,
    pub removed: Vec<Partition>,
    pub removed_boot_entries: Vec<BootEntry>,
    pub restore_windows_boot: bool,
    pub grow_windows: bool,
    pub operations: Vec<UninstallOperation>,
    pub freed_bytes: u64,
    pub dry_run: bool,
}

impl UninstallPlan {
    pub fn render_text(&self) -> String {
        let mut out = format!(
            "Uninstall plan (dry run) — {}\nFreed: {}\n\n",
            self.disk,
            human_size(self.freed_bytes)
        );
        for partition in &self.removed {
            out.push_str(&format!(
                "  remove {} ({} , {})\n",
                partition.name,
                human_size(partition.size_bytes),
                partition.fs.clone().unwrap_or_else(|| "unknown".into())
            ));
        }
        out.push_str(&format!(
            "  Windows boot restored: {}\n  Grow Windows: {}\n\nOperations:\n",
            self.restore_windows_boot, self.grow_windows
        ));
        for operation in &self.operations {
            out.push_str(&format!(
                "  {:?} {} — {}\n",
                operation.kind, operation.target, operation.detail
            ));
        }
        out
    }
}

#[derive(Debug, Error)]
pub enum UninstallError {
    #[error("no AetherOS installation found on {0}")]
    NotInstalled(String),
    #[error("Windows is not present; cannot grow its partition")]
    NoWindows,
}

pub fn plan(disk: &Disk, mode: UninstallMode) -> Result<UninstallPlan, UninstallError> {
    let removed: Vec<Partition> = disk
        .aetheros_partitions()
        .into_iter()
        .cloned()
        .collect();
    if removed.is_empty() {
        return Err(UninstallError::NotInstalled(disk.device.clone()));
    }
    let windows_present = disk.windows_partition().is_some();
    if mode == UninstallMode::GrowWindows && !windows_present {
        return Err(UninstallError::NoWindows);
    }

    let freed_bytes = removed.iter().map(|partition| partition.size_bytes).sum();
    let mut operations = Vec::new();
    // Remove in reverse number order so later partitions never shift earlier.
    let mut ordered = removed.clone();
    ordered.sort_by(|a, b| b.number.cmp(&a.number));
    for partition in &ordered {
        operations.push(UninstallOperation {
            kind: OperationKind::WipeDisk,
            target: partition.name.clone(),
            detail: "Delete the AetherOS partition".into(),
        });
    }
    operations.push(UninstallOperation {
        kind: OperationKind::InstallBootloader,
        target: "ESP".into(),
        detail: "Remove the AetherOS boot entry; keep the Windows Boot Manager".into(),
    });
    if windows_present {
        operations.push(UninstallOperation {
            kind: OperationKind::ShrinkPartition,
            target: "windows".into(),
            detail: if mode == UninstallMode::GrowWindows {
                format!("Grow the Windows partition by {}", human_size(freed_bytes))
            } else {
                "Leave the freed space unallocated".into()
            },
        });
    }

    Ok(UninstallPlan {
        disk: disk.device.clone(),
        removed,
        removed_boot_entries: vec![BootEntry::aetheros("AetherOS")],
        restore_windows_boot: windows_present,
        grow_windows: mode == UninstallMode::GrowWindows,
        operations,
        freed_bytes,
        dry_run: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::{OperatingSystem, PartitionTable};
    use crate::planner::GIB;

    fn installed_disk() -> Disk {
        Disk {
            device: "/dev/nvme0n1".into(),
            size_bytes: 512 * GIB,
            model: "Test".into(),
            table: PartitionTable::Gpt,
            partitions: vec![
                Partition { number: 1, name: "nvme0n1p1".into(), size_bytes: 512 * 1024 * 1024, fs: Some("vfat".into()), mountpoint: None, os: OperatingSystem::Efi, encrypted: false },
                Partition { number: 2, name: "nvme0n1p2".into(), size_bytes: 200 * GIB, fs: Some("ntfs".into()), mountpoint: None, os: OperatingSystem::Windows, encrypted: false },
                Partition { number: 3, name: "nvme0n1p3".into(), size_bytes: 100 * GIB, fs: Some("ext4".into()), mountpoint: Some("/".into()), os: OperatingSystem::AetherOs, encrypted: false },
            ],
            free_bytes: 0,
            removable: false,
        }
    }

    #[test]
    fn removes_only_aetheros_and_keeps_windows() {
        let plan = plan(&installed_disk(), UninstallMode::GrowWindows).unwrap();
        assert_eq!(plan.removed.len(), 1);
        assert_eq!(plan.removed[0].number, 3);
        assert!(plan.restore_windows_boot);
        assert!(plan.grow_windows);
        assert!(plan.operations.iter().any(|operation| operation
            .detail
            .contains("keep the Windows Boot Manager")));
        let text = plan.render_text();
        assert!(text.contains("Uninstall plan (dry run)"));
        assert!(text.contains("Windows boot restored: true"));
    }

    #[test]
    fn missing_installation_is_reported() {
        let mut disk = installed_disk();
        disk.partitions.retain(|partition| partition.os != OperatingSystem::AetherOs);
        assert!(matches!(
            plan(&disk, UninstallMode::KeepUnallocated),
            Err(UninstallError::NotInstalled(_))
        ));
    }

    #[test]
    fn grow_without_windows_is_rejected() {
        let mut disk = installed_disk();
        disk.partitions.retain(|partition| partition.os != OperatingSystem::Windows);
        assert!(matches!(
            plan(&disk, UninstallMode::GrowWindows),
            Err(UninstallError::NoWindows)
        ));
    }

    #[test]
    fn partitions_are_removed_in_reverse_order() {
        let mut disk = installed_disk();
        disk.partitions.push(Partition {
            number: 4,
            name: "nvme0n1p4".into(),
            size_bytes: 20 * GIB,
            fs: Some("ext4".into()),
            mountpoint: None,
            os: OperatingSystem::AetherOs,
            encrypted: false,
        });
        let plan = plan(&disk, UninstallMode::KeepUnallocated).unwrap();
        let removals: Vec<&str> = plan
            .operations
            .iter()
            .filter(|operation| operation.detail.contains("Delete the AetherOS"))
            .map(|operation| operation.target.as_str())
            .collect();
        assert_eq!(removals, vec!["nvme0n1p4", "nvme0n1p3"]);
    }
}
