//! Integration test for the RAUC A/B update wiring (P9-T01/T02).
//!
//! Walks the real flow the daemon runs: `rauc status` (parsed) → the bootloader
//! is authoritative → RAUC writes the inactive slot (`rauc install`) → the slot
//! boots pending → `rauc status mark-good` confirms it, or `mark-bad` rolls
//! back. It also checks that the shipped `system.conf` and the bundle manifest
//! template stay in the shape the daemon and `rauc-bundle.sh` expect.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aether_update::rauc::{
    bootname, bundle_manifest, parse_status, sync_state, validate_system_conf, RaucCommand,
    SYSTEM_CONF,
};
use aether_update::{
    health_check, on_boot, sha256_file, stage_pending, SlotId, SlotState, UpdateState,
};

/// `rauc status --output-format=json` with root A booted.
const BOOTED_A: &str = r#"{
  "compatible": "aetheros",
  "booted": "rootfs.0",
  "slots": {
    "rootfs.0": {"class":"rootfs","device":"/dev/disk/by-partlabel/aetheros-a","type":"raw","bootname":"A","state":"booted","status":"ok","boot_good":true},
    "rootfs.1": {"class":"rootfs","device":"/dev/disk/by-partlabel/aetheros-b","type":"raw","bootname":"B","state":"inactive","status":"ok","boot_good":true}
  }
}"#;

/// Same system after the update booted from root B.
const BOOTED_B: &str = r#"{
  "compatible": "aetheros",
  "booted": "rootfs.1",
  "slots": {
    "rootfs.0": {"class":"rootfs","device":"/dev/disk/by-partlabel/aetheros-a","type":"raw","bootname":"A","state":"inactive","status":"ok","boot_good":true},
    "rootfs.1": {"class":"rootfs","device":"/dev/disk/by-partlabel/aetheros-b","type":"raw","bootname":"B","state":"booted","status":"ok","boot_good":true}
  }
}"#;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aether-rauc-{tag}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn update_installs_marks_good_and_survives_a_reboot() {
    let mut state = UpdateState::default();

    // Booted A, B holds a good image and is the rollback target.
    let status = parse_status(BOOTED_A).unwrap();
    assert!(!sync_state(&mut state, &status), "first sync is not a switch");
    assert_eq!(state.active, SlotId::A);
    assert_eq!(state.slots["b"].state, SlotState::Good);

    // `aether-update rauc-install`: RAUC writes B, the daemon stages it.
    let install = RaucCommand::Install {
        bundle: "/var/cache/aether/aetheros-0.2.0.raucb".into(),
    };
    assert_eq!(
        install.argv(),
        vec!["rauc", "install", "/var/cache/aether/aetheros-0.2.0.raucb"]
    );
    let target = state.active.other();
    stage_pending(&mut state, target).unwrap();
    assert_eq!(state.slots["b"].state, SlotState::Pending);
    assert!(state.update_in_progress);

    // The next boot starts from B; the bootloader view keeps it pending until
    // the health check confirms it.
    let status = parse_status(BOOTED_B).unwrap();
    assert!(sync_state(&mut state, &status));
    assert_eq!(state.active, SlotId::B);
    assert_eq!(state.slots["b"].state, SlotState::Pending);
    assert_eq!(state.slots["a"].state, SlotState::Good);
    assert!(!on_boot(&mut state).unwrap(), "a pending slot may boot");
    assert_eq!(state.boot_attempts, 1);

    // `rauc status mark-good B` then the health pass.
    let command = RaucCommand::MarkGood {
        bootname: bootname(SlotId::B).into(),
    };
    assert_eq!(command.argv(), vec!["rauc", "status", "mark-good", "B"]);
    assert!(!health_check(&mut state, true).unwrap());
    assert_eq!(state.slots["b"].state, SlotState::Active);
    assert_eq!(state.slots["a"].state, SlotState::Good);
    assert!(!state.update_in_progress);
    assert_eq!(state.boot_attempts, 0);

    // State survives the reboot because the boot service reads the same file.
    let path = temp_root("state").join("update-state.json");
    state.save(&path).unwrap();
    let reloaded = UpdateState::load(&path).unwrap();
    assert_eq!(reloaded.active, SlotId::B);
    assert_eq!(reloaded.slots["a"].state, SlotState::Good);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn mark_bad_rolls_back_to_the_last_good_slot() {
    let mut state = UpdateState::default();
    sync_state(&mut state, &parse_status(BOOTED_A).unwrap());
    stage_pending(&mut state, SlotId::B).unwrap();
    sync_state(&mut state, &parse_status(BOOTED_B).unwrap());
    assert_eq!(state.active, SlotId::B);

    // `rauc status mark-bad B`: the health check fails on the new image.
    let command = RaucCommand::MarkBad {
        bootname: bootname(SlotId::B).into(),
    };
    assert_eq!(command.argv(), vec!["rauc", "status", "mark-bad", "B"]);
    assert!(health_check(&mut state, false).unwrap(), "health failure rolls back");
    assert_eq!(state.active, SlotId::A, "back on the last good slot");
    assert_eq!(state.slots["b"].state, SlotState::Bad);
    assert_eq!(state.slots["a"].state, SlotState::Active);
    assert!(!state.update_in_progress);

    // Repeated failed boots of a fresh update also roll back (boot budget).
    stage_pending(&mut state, SlotId::B).unwrap();
    aether_update::activate_pending(&mut state).unwrap();
    assert_eq!(state.active, SlotId::B);
    let max = state.max_boot_attempts;
    for attempt in 0..max {
        assert!(!on_boot(&mut state).unwrap(), "attempt {attempt} is within budget");
    }
    assert!(on_boot(&mut state).unwrap(), "the fourth boot rolls back");
    assert_eq!(state.active, SlotId::A);
    assert_eq!(state.slots["b"].state, SlotState::Bad);
}

#[test]
fn shipped_config_and_bundle_manifest_agree_with_the_daemon() {
    assert!(
        validate_system_conf().is_empty(),
        "system.conf is missing slots: {:?}",
        validate_system_conf()
    );
    for required in [
        "compatible=aetheros",
        "bootloader=grub",
        "/dev/disk/by-partlabel/aetheros-a",
        "/dev/disk/by-partlabel/aetheros-b",
        "bootname=A",
        "bootname=B",
    ] {
        assert!(SYSTEM_CONF.contains(required), "system.conf lost {required}");
    }

    let root = temp_root("bundle");
    let image = root.join("rootfs.squashfs");
    fs::write(&image, b"squashfs image bytes").unwrap();
    let digest = sha256_file(&image).unwrap();
    let size = fs::metadata(&image).unwrap().len();
    let manifest = bundle_manifest("0.2.0", "rootfs.squashfs", &digest, size, "AetherOS 0.2.0");
    assert!(manifest.contains("compatible=aetheros"));
    assert!(manifest.contains("version=0.2.0"));
    assert!(manifest.contains("filename=rootfs.squashfs"));
    assert!(manifest.contains(&format!("sha256={digest}")));
    assert!(manifest.contains(&format!("size={size}")));
    assert!(manifest.contains("description=AetherOS 0.2.0"));
    assert!(!manifest.contains('@'), "every template placeholder is filled");
    let _ = fs::remove_dir_all(root);
}
