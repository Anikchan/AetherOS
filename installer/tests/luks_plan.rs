//! P9-T03 — integration tests for the optional LUKS2 install plan (F5 INS-08).
//!
//! These tests run the planner the way the live image does (disk report in,
//! plan out) and check the container/filesystem ordering, the mapper paths, the
//! `/etc/crypttab` entry and the boot command line. No disk is touched.

use std::path::PathBuf;

use aether_installer::boot::EntryKind;
use aether_installer::disk::{Disk, OperatingSystem, Partition, PartitionTable};
use aether_installer::{
    plan_install, plan_install_with_options, InstallMode, LuksEncryption, OperationKind,
    PlanOptions, WindowsInfo, GIB, LUKS2_HEADER_BYTES, LUKS_UUID_TOKEN,
};

fn dual_boot_disk() -> Disk {
    Disk {
        device: "/dev/nvme0n1".into(),
        size_bytes: 512 * GIB,
        model: "Test SSD".into(),
        table: PartitionTable::Gpt,
        partitions: vec![
            Partition {
                number: 1,
                name: "nvme0n1p1".into(),
                size_bytes: 512 * 1024 * 1024,
                fs: Some("vfat".into()),
                mountpoint: Some(PathBuf::from("/boot/efi")),
                os: OperatingSystem::Efi,
                encrypted: false,
            },
            Partition {
                number: 2,
                name: "nvme0n1p2".into(),
                size_bytes: 100 * GIB,
                fs: Some("ntfs".into()),
                mountpoint: None,
                os: OperatingSystem::Windows,
                encrypted: false,
            },
        ],
        free_bytes: 200 * GIB,
        removable: false,
    }
}

fn windows() -> WindowsInfo {
    WindowsInfo {
        present: true,
        version: Some("Windows 11".into()),
        ntfs_clean: true,
        recovery_key_saved: true,
        ..Default::default()
    }
}

fn encrypted_opts() -> PlanOptions {
    PlanOptions::new(64, 16).encrypted(LuksEncryption::default())
}

#[test]
fn encrypted_alongside_install_plans_a_luks2_root() {
    let plan = plan_install_with_options(
        &dual_boot_disk(),
        &windows(),
        InstallMode::Alongside,
        None,
        &encrypted_opts(),
    )
    .expect("plan");

    assert!(plan.dry_run);
    assert_eq!(plan.encryption, Some(LuksEncryption::default()));

    // The ESP stays shared with Windows; only one Windows boot entry is added.
    assert_eq!(
        plan.allocations.iter().filter(|a| a.role == "ESP").count(),
        1
    );
    assert!(plan.boot_entries.iter().any(|e| e.kind == EntryKind::Windows));

    // Root is the mapper device, the container is the physical partition.
    let container = plan
        .allocations
        .iter()
        .find(|a| a.role == "root-luks")
        .expect("container allocation");
    assert_eq!(container.fs, "crypt-luks2");
    assert_eq!(container.device, "/dev/nvme0n1p3");
    let root = plan
        .allocations
        .iter()
        .find(|a| a.role == "root")
        .expect("root allocation");
    assert_eq!(root.device, "/dev/mapper/aether-root");
    assert_eq!(root.fs, "ext4");
    assert_eq!(root.size_bytes + LUKS2_HEADER_BYTES, container.size_bytes);

    // Order: partition, luksFormat, luksOpen, crypttab, mkfs, image, swap.
    let kinds: Vec<OperationKind> = plan.operations.iter().map(|op| op.kind).collect();
    let index = |kind: OperationKind| {
        kinds
            .iter()
            .position(|candidate| *candidate == kind)
            .unwrap_or_else(|| panic!("missing {kind:?}"))
    };
    assert!(index(OperationKind::CreatePartition) < index(OperationKind::CreateLuksContainer));
    assert!(index(OperationKind::CreateLuksContainer) < index(OperationKind::OpenLuksContainer));
    assert!(index(OperationKind::OpenLuksContainer) < index(OperationKind::WriteCrypttab));
    assert!(index(OperationKind::WriteCrypttab) < index(OperationKind::WriteSystemImage));

    // Only the root filesystem is formatted; the ESP is untouched.
    let formatted: Vec<&str> = plan
        .operations
        .iter()
        .filter(|op| op.kind == OperationKind::FormatPartition && op.detail == "Format ext4")
        .map(|op| op.target.as_str())
        .collect();
    assert_eq!(formatted, vec!["/dev/mapper/aether-root"]);

    let crypttab = plan
        .operations
        .iter()
        .find(|op| op.kind == OperationKind::WriteCrypttab)
        .unwrap();
    assert_eq!(crypttab.target, "/etc/crypttab");
    assert!(crypttab.detail.contains("aether-root UUID=<LUKS-UUID> none luks"));

    // The dry run shows the encryption and the LUKS-specific warnings.
    let text = plan.render_text();
    assert!(text.contains("Encryption: LUKS2 aes-xts-plain64 512"));
    assert!(text.contains("recovery key"));
    assert!(text.contains("initramfs"));
}

#[test]
fn erase_install_encrypts_the_new_root_and_updates_the_boot_entry() {
    let mut plan = plan_install_with_options(
        &dual_boot_disk(),
        &windows(),
        InstallMode::Erase,
        Some("/dev/nvme0n1"),
        &encrypted_opts(),
    )
    .expect("plan");

    assert!(plan
        .operations
        .iter()
        .any(|op| op.kind == OperationKind::WipeDisk));
    assert!(plan
        .operations
        .iter()
        .any(|op| op.kind == OperationKind::CreateLuksContainer));

    let entry = plan
        .boot_entries
        .iter()
        .find(|entry| entry.kind == EntryKind::AetherOs)
        .expect("AetherOS entry");
    assert!(entry.options.contains(&format!("rd.luks.name={LUKS_UUID_TOKEN}=aether-root")));
    assert!(entry.options.contains("root=/dev/mapper/aether-root"));

    plan.set_luks_uuid("11111111-2222-3333-4444-555555555555");
    let entry = plan
        .boot_entries
        .iter()
        .find(|entry| entry.kind == EntryKind::AetherOs)
        .unwrap();
    assert!(!entry.options.contains(LUKS_UUID_TOKEN));
    assert!(entry
        .options
        .contains("rd.luks.name=11111111-2222-3333-4444-555555555555=aether-root"));
}

#[test]
fn unencrypted_plans_are_unchanged() {
    let plan = plan_install(
        &dual_boot_disk(),
        &windows(),
        InstallMode::Alongside,
        64,
        16,
        None,
    )
    .expect("plan");
    assert!(plan.encryption.is_none());
    assert!(!plan
        .operations
        .iter()
        .any(|op| matches!(
            op.kind,
            OperationKind::CreateLuksContainer
                | OperationKind::OpenLuksContainer
                | OperationKind::WriteCrypttab
        )));
    let root = plan
        .allocations
        .iter()
        .find(|a| a.role == "root")
        .unwrap();
    assert!(root.device.starts_with("/dev/nvme0n1p"));
}

#[test]
fn the_plan_serializes_with_the_encryption_section() {
    let plan = plan_install_with_options(
        &dual_boot_disk(),
        &windows(),
        InstallMode::Alongside,
        None,
        &encrypted_opts(),
    )
    .expect("plan");
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains("\"encryption\""));
    assert!(json.contains("\"luks_version\":2"));
    let back: aether_installer::InstallPlan = serde_json::from_str(&json).unwrap();
    assert_eq!(back, plan);
}

#[test]
fn an_invalid_mapper_name_is_refused_before_any_operation() {
    let options = PlanOptions::new(64, 16).encrypted(LuksEncryption {
        mapper_name: "../etc".into(),
        ..LuksEncryption::default()
    });
    let error = plan_install_with_options(
        &dual_boot_disk(),
        &windows(),
        InstallMode::Alongside,
        None,
        &options,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        aether_installer::PlanError::InvalidEncryption(_)
    ));
}
