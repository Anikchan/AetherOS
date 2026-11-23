//! Partition planner and dry run (P8-T03, F5 INS-03/04/05/12).
//!
//! The planner is pure: it produces operations and a boot menu from a disk
//! report. Applying is a separate, guarded step.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::boot::BootEntry;
use crate::disk::{human_size, Disk, OperatingSystem, PartitionTable, WindowsInfo};
use crate::precheck::{InstallMode, MIN_FREE_BYTES, RECOMMENDED_FREE_BYTES};

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;
pub const ESP_BYTES: u64 = 512 * MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationKind {
    ShrinkPartition,
    CreatePartition,
    FormatPartition,
    WipeDisk,
    InstallBootloader,
    CreateSwapFile,
    WriteSystemImage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub kind: OperationKind,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionAllocation {
    pub role: String,
    pub device: String,
    pub size_bytes: u64,
    pub fs: String,
    pub mount: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub disk: String,
    pub disk_label: String,
    pub mode: InstallMode,
    pub table: PartitionTable,
    pub allocations: Vec<PartitionAllocation>,
    pub operations: Vec<Operation>,
    pub boot_entries: Vec<BootEntry>,
    pub swap_file_gib: u32,
    pub shrink_bytes: u64,
    pub warnings: Vec<String>,
    /// Always true: this plan is the dry run (INS-12).
    pub dry_run: bool,
}

impl InstallPlan {
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Install plan (dry run) — {} — {:?}\n",
            self.disk_label, self.mode
        ));
        out.push_str(&format!("Partition table: {:?}\n\n", self.table));
        out.push_str("Allocations:\n");
        for allocation in &self.allocations {
            out.push_str(&format!(
                "  {:<8} {:<16} {:>10}  {:<6} {}\n",
                allocation.role,
                allocation.device,
                human_size(allocation.size_bytes),
                allocation.fs,
                allocation.mount
            ));
        }
        out.push_str("\nOperations:\n");
        for (index, operation) in self.operations.iter().enumerate() {
            out.push_str(&format!(
                "  {}. {:<20} {} — {}\n",
                index + 1,
                format!("{:?}", operation.kind),
                operation.target,
                operation.detail
            ));
        }
        out.push_str(&format!(
            "\nSwap file: {} GiB (hibernation needs swap >= RAM)\n",
            self.swap_file_gib
        ));
        out.push_str("\nBoot menu:\n");
        for entry in &self.boot_entries {
            out.push_str(&format!("  {} -> {}\n", entry.title, entry.efi_path));
        }
        if !self.warnings.is_empty() {
            out.push_str("\nWarnings:\n");
            for warning in &self.warnings {
                out.push_str(&format!("  - {warning}\n"));
            }
        }
        out
    }
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("not enough free space: {available} available, {required} required")]
    NotEnoughSpace { available: u64, required: u64 },
    #[error("the disk has no EFI System Partition and cannot host a UEFI install")]
    NoEsp,
    #[error("the disk already contains AetherOS; run the uninstall mode first")]
    AlreadyInstalled,
    #[error("erase install needs an explicit disk confirmation")]
    EraseNotConfirmed,
    #[error("the disk is removable; refusing to install to {0}")]
    Removable(String),
}

/// Build the plan. `desired_root_gib` is clamped to the available space.
pub fn plan(
    disk: &Disk,
    windows: &WindowsInfo,
    mode: InstallMode,
    desired_root_gib: u64,
    ram_gib: u32,
    confirmed_disk: Option<&str>,
) -> Result<InstallPlan, PlanError> {
    if disk.removable {
        return Err(PlanError::Removable(disk.device.clone()));
    }
    if !disk.aetheros_partitions().is_empty() {
        return Err(PlanError::AlreadyInstalled);
    }
    if mode == InstallMode::Erase
        && confirmed_disk
            .map(|device| device != disk.device)
            .unwrap_or(true)
    {
        return Err(PlanError::EraseNotConfirmed);
    }

    let existing_esp = disk.esp().cloned();
    let mut operations = Vec::new();
    let mut allocations = Vec::new();
    let mut warnings = Vec::new();
    let mut shrink_bytes = 0u64;

    let available = match mode {
        InstallMode::Erase => disk.size_bytes,
        InstallMode::Alongside => {
            let mut available = disk.free_bytes;
            if available < MIN_FREE_BYTES {
                if let Some(windows_partition) = disk.windows_partition() {
                    // We cannot know NTFS used space from lsblk; assume at
                    // most 20% can be recovered and warn.
                    let shrinkable = windows_partition.size_bytes / 5;
                    available += shrinkable;
                    shrink_bytes = shrinkable;
                    if shrinkable > 0 {
                        warnings.push(format!(
                            "Windows will be shrunk by {}; run the NTFS check first.",
                            human_size(shrinkable)
                        ));
                    }
                }
            }
            available
        }
    };

    if available < MIN_FREE_BYTES {
        return Err(PlanError::NotEnoughSpace {
            available,
            required: MIN_FREE_BYTES,
        });
    }
    if mode == InstallMode::Alongside && existing_esp.is_none() {
        return Err(PlanError::NoEsp);
    }
    if available < RECOMMENDED_FREE_BYTES {
        warnings.push(format!(
            "{} available; {} recommended for comfortable use.",
            human_size(available),
            human_size(RECOMMENDED_FREE_BYTES)
        ));
    }

    if mode == InstallMode::Erase {
        operations.push(Operation {
            kind: OperationKind::WipeDisk,
            target: disk.device.clone(),
            detail: "Erase the partition table and all data on this disk".into(),
        });
    }

    if let Some(esp) = &existing_esp {
        allocations.push(PartitionAllocation {
            role: "ESP".into(),
            device: format!("{}p{}", disk.device, esp.number),
            size_bytes: esp.size_bytes,
            fs: "vfat".into(),
            mount: "/boot/efi".into(),
        });
        operations.push(Operation {
            kind: OperationKind::InstallBootloader,
            target: format!("{}p{}", disk.device, esp.number),
            detail: "Install systemd-boot; keep the Windows Boot Manager".into(),
        });
    } else {
        let esp_device = if mode == InstallMode::Erase {
            format!("{}p1", disk.device)
        } else {
            next_partition(disk, mode)
        };
        allocations.push(PartitionAllocation {
            role: "ESP".into(),
            device: esp_device.clone(),
            size_bytes: ESP_BYTES,
            fs: "vfat".into(),
            mount: "/boot/efi".into(),
        });
        operations.push(Operation {
            kind: OperationKind::CreatePartition,
            target: esp_device.clone(),
            detail: format!("Create a {} EFI System Partition", human_size(ESP_BYTES)),
        });
        operations.push(Operation {
            kind: OperationKind::FormatPartition,
            target: esp_device.clone(),
            detail: "Format vfat".into(),
        });
        operations.push(Operation {
            kind: OperationKind::InstallBootloader,
            target: esp_device,
            detail: "Install systemd-boot".into(),
        });
    }

    if shrink_bytes > 0 {
        if let Some(windows_partition) = disk.windows_partition() {
            operations.insert(
                0,
                Operation {
                    kind: OperationKind::ShrinkPartition,
                    target: format!("{}p{}", disk.device, windows_partition.number),
                    detail: format!("Shrink NTFS by {}", human_size(shrink_bytes)),
                },
            );
        }
    }

    let root_device = if mode == InstallMode::Erase {
        format!("{}p2", disk.device)
    } else if existing_esp.is_some() {
        next_partition(disk, mode)
    } else {
        format!("{}p{}", disk.device, disk.partitions.len() + 2)
    };
    let root_size = desired_root_gib
        .saturating_mul(GIB)
        .min(available.saturating_sub(ESP_BYTES));
    if root_size < MIN_FREE_BYTES {
        return Err(PlanError::NotEnoughSpace {
            available,
            required: MIN_FREE_BYTES,
        });
    }
    allocations.push(PartitionAllocation {
        role: "root".into(),
        device: root_device.clone(),
        size_bytes: root_size,
        fs: "ext4".into(),
        mount: "/".into(),
    });
    operations.push(Operation {
        kind: OperationKind::CreatePartition,
        target: root_device.clone(),
        detail: format!("Create root partition ({})", human_size(root_size)),
    });
    operations.push(Operation {
        kind: OperationKind::FormatPartition,
        target: root_device,
        detail: "Format ext4".into(),
    });
    operations.push(Operation {
        kind: OperationKind::WriteSystemImage,
        target: "root".into(),
        detail: "Write the read-only AetherOS image and tmpfs /var, /home".into(),
    });

    let swap_file_gib = ram_gib.min(8) as u64;
    operations.push(Operation {
        kind: OperationKind::CreateSwapFile,
        target: "root".into(),
        detail: format!("Create a {swap_file_gib} GiB swap file for hibernation"),
    });

    let mut boot_entries = vec![BootEntry::aetheros("AetherOS")];
    if windows.present || disk.windows_partition().is_some() {
        boot_entries.push(BootEntry::windows("Windows Boot Manager"));
    }

    Ok(InstallPlan {
        disk: disk.device.clone(),
        disk_label: disk.label(),
        mode,
        table: if mode == InstallMode::Erase {
            PartitionTable::Gpt
        } else {
            disk.table
        },
        allocations,
        operations,
        boot_entries,
        swap_file_gib: swap_file_gib.min(u32::MAX as u64) as u32,
        shrink_bytes,
        warnings,
        dry_run: true,
    })
}

fn next_partition(disk: &Disk, mode: InstallMode) -> String {
    if mode == InstallMode::Erase {
        return format!("{}p1", disk.device);
    }
    let next = disk
        .partitions
        .iter()
        .map(|partition| partition.number)
        .max()
        .unwrap_or(0)
        + 1;
    format!("{}p{}", disk.device, next)
}

/// Mark operating system rows for a plan summary.
pub fn aetheros_roles(disk: &Disk) -> Vec<(u32, OperatingSystem)> {
    disk.partitions
        .iter()
        .map(|partition| (partition.number, partition.os))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::{OperatingSystem, Partition, PartitionTable};

    fn disk(free_bytes: u64, size: u64) -> Disk {
        Disk {
            device: "/dev/nvme0n1".into(),
            size_bytes: size,
            model: "Test SSD".into(),
            table: PartitionTable::Gpt,
            partitions: vec![
                Partition { number: 1, name: "nvme0n1p1".into(), size_bytes: ESP_BYTES, fs: Some("vfat".into()), mountpoint: Some("/boot/efi".into()), os: OperatingSystem::Efi, encrypted: false },
                Partition { number: 2, name: "nvme0n1p2".into(), size_bytes: 40 * GIB, fs: Some("ntfs".into()), mountpoint: None, os: OperatingSystem::Windows, encrypted: false },
            ],
            free_bytes,
            removable: false,
        }
    }

    fn windows() -> WindowsInfo {
        WindowsInfo {
            present: true,
            version: Some("Windows 11".into()),
            ..Default::default()
        }
    }

    #[test]
    fn alongside_with_free_space_reuses_the_esp() {
        let plan = plan(
            &disk(200 * GIB, 512 * GIB),
            &windows(),
            InstallMode::Alongside,
            100,
            16,
            None,
        )
        .unwrap();
        assert!(plan.operations.iter().any(|op| op.kind == OperationKind::InstallBootloader));
        assert!(!plan
            .operations
            .iter()
            .any(|op| op.kind == OperationKind::ShrinkPartition));
        assert_eq!(plan.allocations.iter().filter(|a| a.role == "ESP").count(), 1);
        assert_eq!(plan.boot_entries.len(), 2);
        assert!(plan.dry_run);
        let text = plan.render_text();
        assert!(text.contains("Install plan (dry run)"));
        assert!(text.contains("systemd-boot"));
    }

    #[test]
    fn alongside_without_free_space_plans_a_shrink() {
        let mut d = disk(0, 512 * GIB);
        d.partitions[1].size_bytes = 200 * GIB;
        let plan = plan(&d, &windows(), InstallMode::Alongside, 64, 8, None).unwrap();
        assert!(plan.shrink_bytes > 0);
        assert!(plan
            .operations
            .iter()
            .any(|op| op.kind == OperationKind::ShrinkPartition));
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn too_little_space_is_rejected() {
        let result = plan(
            &disk(0, 100 * GIB),
            &windows(),
            InstallMode::Alongside,
            64,
            8,
            None,
        );
        assert!(matches!(result, Err(PlanError::NotEnoughSpace { .. })));
    }

    #[test]
    fn erase_requires_disk_confirmation() {
        let d = disk(400 * GIB, 512 * GIB);
        assert!(matches!(
            plan(&d, &windows(), InstallMode::Erase, 100, 8, None),
            Err(PlanError::EraseNotConfirmed)
        ));
        let plan = plan(
            &d,
            &windows(),
            InstallMode::Erase,
            100,
            8,
            Some("/dev/nvme0n1"),
        )
        .unwrap();
        assert!(plan
            .operations
            .iter()
            .any(|op| op.kind == OperationKind::WipeDisk));
        assert_eq!(plan.allocations[0].role, "ESP");
    }

    #[test]
    fn removable_and_existing_installs_are_rejected() {
        let mut d = disk(200 * GIB, 512 * GIB);
        d.removable = true;
        assert!(matches!(
            plan(&d, &windows(), InstallMode::Alongside, 64, 8, None),
            Err(PlanError::Removable(_))
        ));
        let mut d = disk(200 * GIB, 512 * GIB);
        d.partitions[1].os = OperatingSystem::Windows;
        d.partitions.push(Partition {
            number: 3,
            name: "nvme0n1p3".into(),
            size_bytes: 100 * GIB,
            fs: Some("ext4".into()),
            mountpoint: Some("/".into()),
            os: OperatingSystem::AetherOs,
            encrypted: false,
        });
        assert!(matches!(
            plan(&d, &windows(), InstallMode::Alongside, 64, 8, None),
            Err(PlanError::AlreadyInstalled)
        ));
    }

    #[test]
    fn swap_file_follows_ram_up_to_eight_gib() {
        let plan = plan(
            &disk(200 * GIB, 512 * GIB),
            &windows(),
            InstallMode::Alongside,
            64,
            32,
            None,
        )
        .unwrap();
        assert_eq!(plan.swap_file_gib, 8);
    }
}
