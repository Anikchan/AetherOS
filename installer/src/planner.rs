//! Partition planner and dry run (P8-T03, F5 INS-03/04/05/12).
//!
//! The planner is pure: it produces operations and a boot menu from a disk
//! report. Applying is a separate, guarded step.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::boot::{BootEntry, EntryKind};
use crate::disk::{human_size, Disk, OperatingSystem, PartitionTable, WindowsInfo};
use crate::precheck::{InstallMode, MIN_FREE_BYTES, RECOMMENDED_FREE_BYTES};

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;
pub const ESP_BYTES: u64 = 512 * MIB;
/// LUKS2 stores its header at the start of the container; the filesystem
/// inside is that much smaller than the partition.
pub const LUKS2_HEADER_BYTES: u64 = 16 * MIB;
/// Token left in the kernel command line until `cryptsetup luksFormat` reports
/// the real container UUID (see [`InstallPlan::set_luks_uuid`]).
pub const LUKS_UUID_TOKEN: &str = "<LUKS-UUID>";

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
    CreateLuksContainer,
    OpenLuksContainer,
    WriteCrypttab,
}

/// Optional LUKS2 encryption of the root partition (P9-T03, F5 INS-08).
///
/// Only the *plan* lives here: `cryptsetup luksFormat` and the key ceremony run
/// from the live image, never from a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuksEncryption {
    /// `cryptsetup --cipher` value.
    pub cipher: String,
    /// Key size in bits (`--key-size`).
    pub key_size_bits: u16,
    /// Key derivation function (`--pbkdf`).
    pub pbkdf: String,
    /// LUKS2 only; v1 is refused.
    pub luks_version: u8,
    /// Device mapper name; the root filesystem is formatted on `/dev/mapper/<name>`.
    pub mapper_name: String,
}

impl Default for LuksEncryption {
    fn default() -> Self {
        Self {
            cipher: "aes-xts-plain64".into(),
            key_size_bits: 512,
            pbkdf: "argon2id".into(),
            luks_version: 2,
            mapper_name: "aether-root".into(),
        }
    }
}

impl LuksEncryption {
    /// The mapper device the root filesystem is created on.
    pub fn mapper_device(&self) -> String {
        format!("/dev/mapper/{}", self.mapper_name)
    }

    /// cryptsetup arguments for `luksFormat` (passphrase is prompted, never an
    /// argument, so it cannot leak through the process list).
    pub fn format_args(&self, device: &str) -> Vec<String> {
        vec![
            "luksFormat".into(),
            "--type".into(),
            "luks2".into(),
            "--cipher".into(),
            self.cipher.clone(),
            "--key-size".into(),
            self.key_size_bits.to_string(),
            "--pbkdf".into(),
            self.pbkdf.clone(),
            device.into(),
        ]
    }

    /// cryptsetup arguments for opening the container.
    pub fn open_args(&self, device: &str) -> Vec<String> {
        vec![
            "luksOpen".into(),
            "--type".into(),
            "luks2".into(),
            device.into(),
            self.mapper_name.clone(),
        ]
    }

    /// `/etc/crypttab` line template; the UUID is resolved after `luksFormat`.
    pub fn crypttab_line(&self, luks_uuid: &str) -> String {
        format!("{} UUID={} none luks,discard\n", self.mapper_name, luks_uuid)
    }

    fn validate(&self) -> Result<(), PlanError> {
        if self.luks_version != 2 {
            return Err(PlanError::InvalidEncryption(format!(
                "LUKS{} is not supported; INS-08 requires LUKS2",
                self.luks_version
            )));
        }
        if self.mapper_name.is_empty()
            || !self
                .mapper_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(PlanError::InvalidEncryption(format!(
                "mapper name {:?} must be alphanumeric, '-' or '_'",
                self.mapper_name
            )));
        }
        if self.cipher.trim().is_empty() || self.pbkdf.trim().is_empty() {
            return Err(PlanError::InvalidEncryption(
                "cipher and pbkdf must not be empty".into(),
            ));
        }
        if !matches!(self.key_size_bits, 256 | 512) {
            return Err(PlanError::InvalidEncryption(format!(
                "key size {} is not 256 or 512 bits",
                self.key_size_bits
            )));
        }
        Ok(())
    }
}

/// Planner inputs other than the disk report itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanOptions {
    pub desired_root_gib: u64,
    pub ram_gib: u32,
    #[serde(default)]
    pub encryption: Option<LuksEncryption>,
}

impl PlanOptions {
    pub fn new(desired_root_gib: u64, ram_gib: u32) -> Self {
        Self {
            desired_root_gib,
            ram_gib,
            encryption: None,
        }
    }

    pub fn encrypted(mut self, encryption: LuksEncryption) -> Self {
        self.encryption = Some(encryption);
        self
    }
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
    /// Root encryption, when the user asked for it (P9-T03).
    #[serde(default)]
    pub encryption: Option<LuksEncryption>,
    /// Always true: this plan is the dry run (INS-12).
    pub dry_run: bool,
}

impl InstallPlan {
    /// Fill in the real container UUID after `cryptsetup luksFormat` and rewrite
    /// the kernel command line and `/etc/crypttab` accordingly.
    pub fn set_luks_uuid(&mut self, luks_uuid: &str) {
        if let Some(entry) = self
            .boot_entries
            .iter_mut()
            .find(|entry| entry.kind == EntryKind::AetherOs)
        {
            if entry.options.contains(LUKS_UUID_TOKEN) {
                entry.options = entry.options.replace(LUKS_UUID_TOKEN, luks_uuid);
            } else if !entry.options.contains("rd.luks.name=") {
                let mapper = self
                    .encryption
                    .as_ref()
                    .map(|encryption| encryption.mapper_name.clone())
                    .unwrap_or_else(|| "aether-root".into());
                entry.options = format!(
                    "{} rd.luks.name={luks_uuid}={mapper} root={}",
                    entry.options.trim_end(),
                    self.encryption
                        .as_ref()
                        .map(|encryption| encryption.mapper_device())
                        .unwrap_or_else(|| "/dev/mapper/aether-root".into())
                );
            }
        }
        for operation in self.operations.iter_mut() {
            if operation.kind == OperationKind::WriteCrypttab {
                operation.detail = operation.detail.replace(LUKS_UUID_TOKEN, luks_uuid);
            }
        }
    }

    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Install plan (dry run) — {} — {:?}\n",
            self.disk_label, self.mode
        ));
        out.push_str(&format!("Partition table: {:?}\n\n", self.table));
        match &self.encryption {
            Some(encryption) => out.push_str(&format!(
                "Encryption: LUKS{} {} {} / {} -> {}\n\n",
                encryption.luks_version,
                encryption.cipher,
                encryption.key_size_bits,
                encryption.pbkdf,
                encryption.mapper_device()
            )),
            None => out.push_str("Encryption: none\n\n"),
        }
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
    #[error("invalid encryption settings: {0}")]
    InvalidEncryption(String),
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
    plan_with_options(
        disk,
        windows,
        mode,
        confirmed_disk,
        &PlanOptions::new(desired_root_gib, ram_gib),
    )
}

/// Build the plan with optional root encryption (P9-T03).
pub fn plan_with_options(
    disk: &Disk,
    windows: &WindowsInfo,
    mode: InstallMode,
    confirmed_disk: Option<&str>,
    options: &PlanOptions,
) -> Result<InstallPlan, PlanError> {
    if let Some(encryption) = &options.encryption {
        encryption.validate()?;
    }
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
    let root_size = options
        .desired_root_gib
        .saturating_mul(GIB)
        .min(available.saturating_sub(ESP_BYTES));
    if root_size < MIN_FREE_BYTES {
        return Err(PlanError::NotEnoughSpace {
            available,
            required: MIN_FREE_BYTES,
        });
    }
    let luks_overhead = if options.encryption.is_some() {
        LUKS2_HEADER_BYTES
    } else {
        0
    };
    if root_size <= luks_overhead + MIN_FREE_BYTES / 2 {
        return Err(PlanError::NotEnoughSpace {
            available,
            required: MIN_FREE_BYTES + luks_overhead,
        });
    }
    let filesystem_bytes = root_size - luks_overhead;

    operations.push(Operation {
        kind: OperationKind::CreatePartition,
        target: root_device.clone(),
        detail: format!("Create root partition ({})", human_size(root_size)),
    });

    let format_target = match &options.encryption {
        Some(encryption) => {
            operations.push(Operation {
                kind: OperationKind::CreateLuksContainer,
                target: root_device.clone(),
                detail: format!(
                    "cryptsetup {} (LUKS{}, {}, {}-bit, {})",
                    encryption.format_args(&root_device).join(" "),
                    encryption.luks_version,
                    encryption.cipher,
                    encryption.key_size_bits,
                    encryption.pbkdf
                ),
            });
            operations.push(Operation {
                kind: OperationKind::OpenLuksContainer,
                target: encryption.mapper_device(),
                detail: "Prompt for the passphrase, generate the recovery key once (INS-08), \
                         then cryptsetup open"
                    .into(),
            });
            operations.push(Operation {
                kind: OperationKind::WriteCrypttab,
                target: "/etc/crypttab".into(),
                detail: format!(
                    "Write {}",
                    encryption.crypttab_line(LUKS_UUID_TOKEN).trim_end()
                ),
            });
            allocations.push(PartitionAllocation {
                role: "root-luks".into(),
                device: root_device.clone(),
                size_bytes: root_size,
                fs: "crypt-luks2".into(),
                mount: "-".into(),
            });
            allocations.push(PartitionAllocation {
                role: "root".into(),
                device: encryption.mapper_device(),
                size_bytes: filesystem_bytes,
                fs: "ext4".into(),
                mount: "/".into(),
            });
            encryption.mapper_device()
        }
        None => {
            allocations.push(PartitionAllocation {
                role: "root".into(),
                device: root_device.clone(),
                size_bytes: root_size,
                fs: "ext4".into(),
                mount: "/".into(),
            });
            root_device
        }
    };
    operations.push(Operation {
        kind: OperationKind::FormatPartition,
        target: format_target,
        detail: "Format ext4".into(),
    });
    operations.push(Operation {
        kind: OperationKind::WriteSystemImage,
        target: "root".into(),
        detail: "Write the read-only AetherOS image and tmpfs /var, /home".into(),
    });

    let swap_file_gib = options.ram_gib.min(8) as u64;
    operations.push(Operation {
        kind: OperationKind::CreateSwapFile,
        target: "root".into(),
        detail: format!("Create a {swap_file_gib} GiB swap file for hibernation"),
    });

    let mut boot_entries = vec![BootEntry::aetheros("AetherOS")];
    if let Some(encryption) = &options.encryption {
        if let Some(entry) = boot_entries.first_mut() {
            entry.options = format!(
                "{} rd.luks.name={LUKS_UUID_TOKEN}={} root={}",
                entry.options.trim_end(),
                encryption.mapper_name,
                encryption.mapper_device()
            );
        }
        warnings.push(
            "LUKS2: show the generated recovery key once and store it offline; \
             without it a forgotten passphrase loses all data."
                .into(),
        );
        warnings.push(
            "LUKS2: the initramfs must carry the cryptsetup/udev hooks and the \
             kernel command line needs rd.luks.name=<uuid>=aether-root; \
             verify an encrypted boot before wiping the live USB."
                .into(),
        );
        warnings.push(
            "LUKS2: hibernation needs the swap file on the encrypted root plus a \
             resume offset in the kernel command line."
                .into(),
        );
    }
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
        encryption: options.encryption.clone(),
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

    #[test]
    fn plain_plans_have_no_encryption() {
        let plan = plan(
            &disk(200 * GIB, 512 * GIB),
            &windows(),
            InstallMode::Alongside,
            64,
            8,
            None,
        )
        .unwrap();
        assert!(plan.encryption.is_none());
        assert!(!plan
            .operations
            .iter()
            .any(|op| op.kind == OperationKind::CreateLuksContainer));
        assert!(plan.render_text().contains("Encryption: none"));
    }

    fn encrypted_options() -> PlanOptions {
        PlanOptions::new(64, 8).encrypted(LuksEncryption::default())
    }

    #[test]
    fn encryption_creates_the_container_before_the_filesystem() {
        let plan = plan_with_options(
            &disk(200 * GIB, 512 * GIB),
            &windows(),
            InstallMode::Alongside,
            None,
            &encrypted_options(),
        )
        .unwrap();
        let kinds: Vec<_> = plan.operations.iter().map(|op| op.kind).collect();
        let format = kinds
            .iter()
            .position(|kind| *kind == OperationKind::CreateLuksContainer)
            .unwrap();
        let open = kinds
            .iter()
            .position(|kind| *kind == OperationKind::OpenLuksContainer)
            .unwrap();
        let crypttab = kinds
            .iter()
            .position(|kind| *kind == OperationKind::WriteCrypttab)
            .unwrap();
        let fs = plan
            .operations
            .iter()
            .position(|op| op.kind == OperationKind::FormatPartition && op.detail == "Format ext4")
            .unwrap();
        assert!(format < open && open < crypttab && crypttab < fs);

        let format_op = plan
            .operations
            .iter()
            .find(|op| op.kind == OperationKind::FormatPartition && op.detail == "Format ext4")
            .unwrap();
        assert_eq!(format_op.target, "/dev/mapper/aether-root");
        assert!(plan
            .operations
            .iter()
            .any(|op| op.kind == OperationKind::CreateLuksContainer
                && op.detail.contains("--type luks2")
                && op.detail.contains("aes-xts-plain64")));

        let container = plan
            .allocations
            .iter()
            .find(|allocation| allocation.role == "root-luks")
            .unwrap();
        assert_eq!(container.fs, "crypt-luks2");
        let root = plan
            .allocations
            .iter()
            .find(|allocation| allocation.role == "root")
            .unwrap();
        assert_eq!(root.device, "/dev/mapper/aether-root");
        assert_eq!(root.size_bytes, container.size_bytes - LUKS2_HEADER_BYTES);
        assert_eq!(plan.encryption, Some(LuksEncryption::default()));
        assert!(plan.warnings.iter().any(|warning| warning.contains("recovery key")));
        assert!(plan.render_text().contains("Encryption: LUKS2 aes-xts-plain64 512"));
    }

    #[test]
    fn encryption_rewrites_the_kernel_command_line_once_the_uuid_is_known() {
        let mut plan = plan_with_options(
            &disk(200 * GIB, 512 * GIB),
            &windows(),
            InstallMode::Erase,
            Some("/dev/nvme0n1"),
            &encrypted_options(),
        )
        .unwrap();
        let entry = plan
            .boot_entries
            .iter()
            .find(|entry| entry.kind == EntryKind::AetherOs)
            .unwrap()
            .clone();
        assert!(entry.options.contains("rd.luks.name=<LUKS-UUID>=aether-root"));
        assert!(entry.options.contains("root=/dev/mapper/aether-root"));

        plan.set_luks_uuid("0f0e0d0c-1111-2222-3333-444455556666");
        let entry = plan
            .boot_entries
            .iter()
            .find(|entry| entry.kind == EntryKind::AetherOs)
            .unwrap();
        assert!(entry
            .options
            .contains("rd.luks.name=0f0e0d0c-1111-2222-3333-444455556666=aether-root"));
        assert!(!entry.options.contains(LUKS_UUID_TOKEN));
        let crypttab = plan
            .operations
            .iter()
            .find(|op| op.kind == OperationKind::WriteCrypttab)
            .unwrap();
        assert!(crypttab.detail.contains("UUID=0f0e0d0c-1111-2222-3333-444455556666"));
        assert!(!crypttab.detail.contains(LUKS_UUID_TOKEN));
    }

    #[test]
    fn encryption_settings_are_validated() {
        let mut options = encrypted_options();
        options.encryption = Some(LuksEncryption {
            mapper_name: "root/../x".into(),
            ..LuksEncryption::default()
        });
        assert!(matches!(
            plan_with_options(
                &disk(200 * GIB, 512 * GIB),
                &windows(),
                InstallMode::Alongside,
                None,
                &options
            ),
            Err(PlanError::InvalidEncryption(_))
        ));

        let mut options = encrypted_options();
        options.encryption = Some(LuksEncryption {
            luks_version: 1,
            ..LuksEncryption::default()
        });
        assert!(matches!(
            plan_with_options(
                &disk(200 * GIB, 512 * GIB),
                &windows(),
                InstallMode::Alongside,
                None,
                &options
            ),
            Err(PlanError::InvalidEncryption(_))
        ));

        let mut options = encrypted_options();
        options.encryption = Some(LuksEncryption {
            key_size_bits: 1024,
            ..LuksEncryption::default()
        });
        assert!(matches!(
            plan_with_options(
                &disk(200 * GIB, 512 * GIB),
                &windows(),
                InstallMode::Alongside,
                None,
                &options
            ),
            Err(PlanError::InvalidEncryption(_))
        ));
    }

    #[test]
    fn encryption_works_inside_a_minimum_size_root() {
        // Exactly the minimum root: the LUKS2 header must still fit.
        let plan = plan_with_options(
            &disk(200 * GIB, 512 * GIB),
            &windows(),
            InstallMode::Alongside,
            None,
            &PlanOptions::new(MIN_FREE_BYTES / GIB, 8).encrypted(LuksEncryption::default()),
        )
        .unwrap();
        let container = plan
            .allocations
            .iter()
            .find(|allocation| allocation.role == "root-luks")
            .unwrap();
        assert_eq!(container.size_bytes, MIN_FREE_BYTES);
        assert!(plan.swap_file_gib >= 1);
    }
}
