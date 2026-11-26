//! RAUC A/B integration (P9-T01/T02).
//!
//! RAUC performs the image write and the bootloader switch. This module maps
//! RAUC's slot view onto [`UpdateState`], renders the shipped `system.conf` and
//! generates the `manifest.raucm` for a bundle, so the state machine is wired to
//! the real A/B configuration instead of a parallel model.
//!
//! Boot-name mapping (D4 §6, `update/config/system.conf`):
//! `A` = `rootfs.0` = `/dev/disk/by-partlabel/aetheros-a`,
//! `B` = `rootfs.1` = `/dev/disk/by-partlabel/aetheros-b`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SlotId, SlotState, UpdateState};

/// The shipped RAUC system configuration.
pub const SYSTEM_CONF: &str = include_str!("../config/system.conf");
/// Template for a bundle manifest.
pub const MANIFEST_TEMPLATE: &str = include_str!("../config/manifest.raucm.tmpl");

#[derive(Debug, Error)]
pub enum RaucError {
    #[error("invalid rauc status JSON: {0}")]
    Json(String),
    #[error("rauc status has no booted rootfs slot")]
    NoBootedSlot,
    #[error("rauc status has no slot with bootname {0}")]
    UnknownBootname(String),
}

/// One rootfs slot as reported by `rauc status --output-format=json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaucSlot {
    pub id: SlotId,
    pub bootname: String,
    pub device: String,
    pub state: String,
    pub status: Option<String>,
    pub boot_good: Option<bool>,
}

/// Parsed RAUC status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaucStatus {
    pub compatible: Option<String>,
    pub booted: SlotId,
    pub slots: Vec<RaucSlot>,
}

impl RaucStatus {
    pub fn slot(&self, id: SlotId) -> Option<&RaucSlot> {
        self.slots.iter().find(|slot| slot.id == id)
    }
}

#[derive(Debug, Deserialize)]
struct RawStatus {
    compatible: Option<String>,
    booted: Option<String>,
    slots: BTreeMap<String, RawSlot>,
}

#[derive(Debug, Deserialize)]
struct RawSlot {
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    bootname: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    boot_good: Option<bool>,
}

fn slot_id(bootname: &str) -> Option<SlotId> {
    match bootname.trim().to_ascii_uppercase().as_str() {
        "A" | "ROOTFS.0" | "0" => Some(SlotId::A),
        "B" | "ROOTFS.1" | "1" => Some(SlotId::B),
        _ => None,
    }
}

/// Parse `rauc status --output-format=json`.
pub fn parse_status(text: &str) -> Result<RaucStatus, RaucError> {
    let raw: RawStatus = serde_json::from_str(text).map_err(|error| RaucError::Json(error.to_string()))?;
    let mut slots = Vec::new();
    let mut booted: Option<SlotId> = None;
    let booted_key = raw.booted.clone();
    for (key, slot) in &raw.slots {
        if let Some(class) = &slot.class {
            if class != "rootfs" {
                continue;
            }
        }
        let bootname = slot
            .bootname
            .clone()
            .or_else(|| {
                // Older RAUC output omits bootname for ext4 slots; the slot
                // key is `rootfs.0`/`rootfs.1`.
                key.strip_prefix("rootfs.").map(|index| {
                    if index == "0" {
                        "A".to_string()
                    } else {
                        "B".to_string()
                    }
                })
            })
            .unwrap_or_else(|| key.clone());
        let Some(id) = slot_id(&bootname) else {
            continue;
        };
        let is_booted = booted_key.as_deref() == Some(key.as_str())
            || booted_key
                .as_deref()
                .map(|value| slot_id(value) == Some(id))
                .unwrap_or(false);
        if is_booted {
            booted = Some(id);
        }
        slots.push(RaucSlot {
            id,
            bootname,
            device: slot.device.clone().unwrap_or_default(),
            state: slot.state.clone().unwrap_or_else(|| "inactive".into()),
            status: slot.status.clone(),
            boot_good: slot.boot_good,
        });
    }
    slots.sort_by_key(|slot| slot.id);
    let booted = booted.ok_or(RaucError::NoBootedSlot)?;
    Ok(RaucStatus {
        compatible: raw.compatible,
        booted,
        slots,
    })
}

fn rauc_state(slot: &RaucSlot) -> SlotState {
    if slot.state.eq_ignore_ascii_case("booted") || slot.state.eq_ignore_ascii_case("active") {
        return SlotState::Active;
    }
    if slot.state.eq_ignore_ascii_case("bad") || slot.boot_good == Some(false) {
        return SlotState::Bad;
    }
    if slot.status.as_deref() == Some("ok") && slot.boot_good != Some(false) {
        return SlotState::Good;
    }
    SlotState::Empty
}

/// Make the bootloader's view authoritative: the booted slot is active and the
/// other slot is good/bad/empty according to RAUC. Returns `true` when the
/// active slot changed.
pub fn sync_state(state: &mut UpdateState, status: &RaucStatus) -> bool {
    let previous = state.active;
    state.active = status.booted;
    for slot in &status.slots {
        let mapped = rauc_state(slot);
        let entry = state
            .slots
            .entry(match slot.id {
                SlotId::A => "a".to_string(),
                SlotId::B => "b".to_string(),
            })
            .or_insert_with(|| crate::Slot::empty(slot.id));
        if slot.id == status.booted {
            // A pending slot stays pending until the health check confirms it,
            // so the boot-attempt counter is not reset by the sync itself.
            if entry.state != SlotState::Pending {
                entry.state = SlotState::Active;
            }
        } else if entry.state != SlotState::Bad || mapped != SlotState::Empty {
            entry.state = mapped;
        }
    }
    let changed = previous != status.booted;
    if changed {
        state.boot_attempts = 0;
        // Only a pending slot needs the health handshake; switching to a
        // previously confirmed slot is not an update in progress.
        let pending = state
            .slot(status.booted)
            .map(|slot| slot.state == SlotState::Pending)
            .unwrap_or(false);
        state.update_in_progress = pending;
    }
    changed
}

/// Arguments for the RAUC commands the daemon runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaucCommand {
    Status,
    Install { bundle: String },
    MarkGood { bootname: String },
    MarkBad { bootname: String },
}

impl RaucCommand {
    /// Full argv, starting with the executable.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec!["rauc".to_string()];
        match self {
            Self::Status => {
                argv.push("status".into());
                argv.push("--output-format=json".into());
            }
            Self::Install { bundle } => {
                argv.push("install".into());
                argv.push(bundle.clone());
            }
            Self::MarkGood { bootname } => {
                argv.extend(["status".into(), "mark-good".into(), bootname.clone()]);
            }
            Self::MarkBad { bootname } => {
                argv.extend(["status".into(), "mark-bad".into(), bootname.clone()]);
            }
        }
        argv
    }
}

pub fn bootname(id: SlotId) -> &'static str {
    match id {
        SlotId::A => "A",
        SlotId::B => "B",
    }
}

/// Render `manifest.raucm` for a bundle from the shipped template.
pub fn bundle_manifest(
    version: &str,
    image_filename: &str,
    image_sha256: &str,
    image_size: u64,
    description: &str,
) -> String {
    MANIFEST_TEMPLATE
        .replace("@VERSION@", version)
        .replace("@IMAGE@", image_filename)
        .replace("@IMAGE_SHA256@", image_sha256)
        .replace("@IMAGE_SIZE@", &image_size.to_string())
        .replace("@DESCRIPTION@", description)
}

/// Check that the shipped config still names both A/B slots.
pub fn validate_system_conf() -> Vec<String> {
    let mut problems = Vec::new();
    for required in [
        "compatible=aetheros",
        "bootloader=grub",
        "[slot.rootfs.0]",
        "[slot.rootfs.1]",
        "bootname=A",
        "bootname=B",
        "aetheros-a",
        "aetheros-b",
    ] {
        if !SYSTEM_CONF.contains(required) {
            problems.push(required.to_string());
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{install, UpdateManifest};

    const STATUS_JSON: &str = r#"{
      "compatible": "aetheros",
      "booted": "rootfs.0",
      "slots": {
        "rootfs.0": {"class":"rootfs","device":"/dev/disk/by-partlabel/aetheros-a","type":"raw","bootname":"A","state":"booted","status":"ok","boot_good":true},
        "rootfs.1": {"class":"rootfs","device":"/dev/disk/by-partlabel/aetheros-b","type":"raw","bootname":"B","state":"inactive","status":"ok","boot_good":null}
      }
    }"#;

    #[test]
    fn parses_rauc_status() {
        let status = parse_status(STATUS_JSON).unwrap();
        assert_eq!(status.booted, SlotId::A);
        assert_eq!(status.slots.len(), 2);
        assert_eq!(status.slot(SlotId::A).unwrap().device, "/dev/disk/by-partlabel/aetheros-a");
        assert_eq!(status.slot(SlotId::B).unwrap().state, "inactive");
    }

    #[test]
    fn parses_bootname_style_booted_field() {
        let json = r#"{"booted":"B","slots":{
          "rootfs.0":{"class":"rootfs","bootname":"A","state":"inactive","status":"ok"},
          "rootfs.1":{"class":"rootfs","bootname":"B","state":"booted","status":"ok"}}}"#;
        assert_eq!(parse_status(json).unwrap().booted, SlotId::B);
    }

    #[test]
    fn sync_uses_the_bootloader_as_authority() {
        let status = parse_status(STATUS_JSON).unwrap();
        let mut state = UpdateState::default();
        assert!(!sync_state(&mut state, &status));
        assert_eq!(state.active, SlotId::A);
        assert_eq!(
            state.slots.get("b").unwrap().state,
            SlotState::Good,
            "inactive slot with status ok is a rollback target"
        );

        let booted_b = r#"{"booted":"rootfs.1","slots":{
          "rootfs.0":{"class":"rootfs","bootname":"A","state":"inactive","status":"ok"},
          "rootfs.1":{"class":"rootfs","bootname":"B","state":"booted","status":"ok"}}}"#;
        let status = parse_status(booted_b).unwrap();
        assert!(sync_state(&mut state, &status));
        assert_eq!(state.active, SlotId::B);
    }

    #[test]
    fn pending_slot_stays_pending_after_sync() {
        let mut state = UpdateState::default();
        state.slots.get_mut("a").unwrap().version = Some("1.0.0".into());
        state.slots.get_mut("a").unwrap().state = SlotState::Active;
        let manifest = UpdateManifest {
            version: "1.1.0".into(),
            image_sha256: "abc".into(),
            signature: None,
            min_version: None,
            notes: String::new(),
        };
        install(&mut state, &manifest, "abc", true).unwrap();
        state.slots.get_mut("b").unwrap().state = SlotState::Pending;
        let booted_b = r#"{"booted":"rootfs.1","slots":{
          "rootfs.0":{"class":"rootfs","bootname":"A","state":"inactive","status":"ok"},
          "rootfs.1":{"class":"rootfs","bootname":"B","state":"booted","status":"ok"}}}"#;
        let status = parse_status(booted_b).unwrap();
        sync_state(&mut state, &status);
        assert_eq!(state.slots.get("b").unwrap().state, SlotState::Pending);
    }

    #[test]
    fn bad_slot_is_marked_bad() {
        let json = r#"{"booted":"rootfs.0","slots":{
          "rootfs.0":{"class":"rootfs","bootname":"A","state":"booted","status":"ok"},
          "rootfs.1":{"class":"rootfs","bootname":"B","state":"inactive","status":"bad","boot_good":false}}}"#;
        let status = parse_status(json).unwrap();
        let mut state = UpdateState::default();
        sync_state(&mut state, &status);
        assert_eq!(state.slots.get("b").unwrap().state, SlotState::Bad);
    }

    #[test]
    fn command_lines_are_explicit() {
        assert_eq!(
            RaucCommand::Status.argv(),
            vec!["rauc", "status", "--output-format=json"]
        );
        assert_eq!(
            RaucCommand::MarkGood { bootname: "B".into() }.argv(),
            vec!["rauc", "status", "mark-good", "B"]
        );
        assert_eq!(
            RaucCommand::Install { bundle: "/tmp/a.raucb".into() }.argv(),
            vec!["rauc", "install", "/tmp/a.raucb"]
        );
    }

    #[test]
    fn shipped_config_names_both_slots() {
        assert!(validate_system_conf().is_empty(), "{:?}", validate_system_conf());
        assert!(SYSTEM_CONF.contains("compatible=aetheros"));
    }

    #[test]
    fn bundle_manifest_fills_the_template() {
        let manifest = bundle_manifest("1.2.0", "rootfs.squashfs", "deadbeef", 1234, "test");
        assert!(manifest.contains("version=1.2.0"));
        assert!(manifest.contains("filename=rootfs.squashfs"));
        assert!(manifest.contains("sha256=deadbeef"));
        assert!(manifest.contains("size=1234"));
        assert!(!manifest.contains("@VERSION@"));
    }
}
