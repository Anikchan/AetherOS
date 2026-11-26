//! A/B update slots, health checks and rollback (P9-T01/T02).
//!
//! The model is deliberately independent of RAUC: RAUC performs the image
//! write, this module decides which slot is active, when an update may be
//! installed, and when to roll back. A failed health check or too many boot
//! attempts on a pending slot returns to the last good slot.

pub mod rauc;

pub use rauc::{
    bootname, bundle_manifest, parse_status, sync_state, validate_system_conf, RaucCommand,
    RaucError, RaucSlot, RaucStatus, MANIFEST_TEMPLATE, SYSTEM_CONF,
};

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotId {
    A,
    B,
}

impl SlotId {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotState {
    /// Running slot.
    Active,
    /// Contains a good, older image.
    Good,
    /// Newly written, not yet confirmed.
    Pending,
    /// Failed to boot or failed health checks.
    Bad,
    /// Empty slot.
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub id: SlotId,
    pub version: Option<String>,
    pub state: SlotState,
    pub checksum: Option<String>,
}

impl Slot {
    pub fn empty(id: SlotId) -> Self {
        Self {
            id,
            version: None,
            state: SlotState::Empty,
            checksum: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub image_sha256: String,
    /// Checksum-signature placeholder; the signing pipeline is deferred.
    #[serde(default)]
    pub signature: Option<String>,
    /// Refuse to downgrade below this version.
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    pub active: SlotId,
    pub slots: BTreeMap<String, Slot>,
    pub boot_attempts: u32,
    pub max_boot_attempts: u32,
    pub last_good_version: Option<String>,
    pub update_in_progress: bool,
}

impl Default for UpdateState {
    fn default() -> Self {
        let mut slots = BTreeMap::new();
        let mut a = Slot::empty(SlotId::A);
        // The running system always has one active slot, even before a
        // version is recorded.
        a.state = SlotState::Active;
        slots.insert("a".into(), a);
        slots.insert("b".into(), Slot::empty(SlotId::B));
        Self {
            active: SlotId::A,
            slots,
            boot_attempts: 0,
            max_boot_attempts: 3,
            last_good_version: None,
            update_in_progress: false,
        }
    }
}

impl UpdateState {
    fn slot(&self, id: SlotId) -> Option<&Slot> {
        self.slots.get(&format!("{}", if id == SlotId::A { "a" } else { "b" }))
    }

    fn slot_mut(&mut self, id: SlotId) -> &mut Slot {
        self.slots
            .entry(if id == SlotId::A { "a".into() } else { "b".into() })
            .or_insert_with(|| Slot::empty(id))
    }

    pub fn active_slot(&self) -> Option<&Slot> {
        self.slot(self.active)
    }

    pub fn inactive_slot(&self) -> Option<&Slot> {
        self.slot(self.active.other())
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("checksum mismatch: manifest {expected}, image {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("signature is not valid")]
    BadSignature,
    #[error("update {new} is older than the installed {installed}")]
    Downgrade { new: String, installed: String },
    #[error("the inactive slot is not writable: {0}")]
    NoInactiveSlot(String),
    #[error("no good slot to roll back to")]
    NoRollbackTarget,
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

/// SHA-256 of an image file.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Compare dotted versions numerically.
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |value: &str| -> Vec<u64> {
        value
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let left = parse(a);
    let right = parse(b);
    for index in 0..left.len().max(right.len()) {
        match left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Stage an update into the inactive slot.
pub fn install(
    state: &mut UpdateState,
    manifest: &UpdateManifest,
    image_sha256: &str,
    signature_valid: bool,
) -> Result<(), UpdateError> {
    if image_sha256 != manifest.image_sha256 {
        return Err(UpdateError::ChecksumMismatch {
            expected: manifest.image_sha256.clone(),
            actual: image_sha256.to_string(),
        });
    }
    if manifest.signature.is_some() && !signature_valid {
        return Err(UpdateError::BadSignature);
    }
    if let Some(min) = &manifest.min_version {
        if version_cmp(&manifest.version, min) == std::cmp::Ordering::Less {
            return Err(UpdateError::Downgrade {
                new: manifest.version.clone(),
                installed: min.clone(),
            });
        }
    }
    if let Some(active) = state.active_slot() {
        if let Some(version) = &active.version {
            if version_cmp(&manifest.version, version) == std::cmp::Ordering::Less {
                return Err(UpdateError::Downgrade {
                    new: manifest.version.clone(),
                    installed: version.clone(),
                });
            }
        }
    }

    let target = state.active.other();
    let slot = state.slot_mut(target);
    slot.version = Some(manifest.version.clone());
    slot.state = SlotState::Pending;
    slot.checksum = Some(manifest.image_sha256.clone());
    state.update_in_progress = true;
    state.boot_attempts = 0;
    Ok(())
}

/// Mark the inactive slot pending after RAUC has written it (P9-T01).
///
/// RAUC owns the image write, so there is no manifest to compare against here;
/// the bootloader and `rauc status` are the source of truth on the next boot.
pub fn stage_pending(state: &mut UpdateState, slot: SlotId) -> Result<(), UpdateError> {
    if slot == state.active {
        return Err(UpdateError::NoInactiveSlot(
            "cannot stage the running slot".into(),
        ));
    }
    let entry = state.slot_mut(slot);
    entry.state = SlotState::Pending;
    state.update_in_progress = true;
    state.boot_attempts = 0;
    Ok(())
}

/// Switch to the inactive slot after a successful write.
pub fn activate_pending(state: &mut UpdateState) -> Result<(), UpdateError> {    let target = state.active.other();
    let slot = state
        .slot(target)
        .ok_or_else(|| UpdateError::NoInactiveSlot("missing slot".into()))?
        .clone();
    if slot.state != SlotState::Pending {
        return Err(UpdateError::NoInactiveSlot(
            "the inactive slot has no pending image".into(),
        ));
    }
    state.active = target;
    state.boot_attempts = 0;
    Ok(())
}

/// Called by the boot service when a pending slot starts.
///
/// More than `max_boot_attempts` starts without a health pass rolls back.
pub fn on_boot(state: &mut UpdateState) -> Result<bool, UpdateError> {
    let active = state
        .slot(state.active)
        .cloned()
        .ok_or_else(|| UpdateError::NoInactiveSlot("missing active slot".into()))?;
    if active.state != SlotState::Pending {
        // A confirmed slot starts normally.
        return Ok(false);
    }
    state.boot_attempts += 1;
    if state.boot_attempts > state.max_boot_attempts {
        rollback(state)?;
        return Ok(true);
    }
    Ok(false)
}

/// Health check result for the running pending slot.
pub fn health_check(state: &mut UpdateState, passed: bool) -> Result<bool, UpdateError> {
    let active = state
        .slot(state.active)
        .cloned()
        .ok_or_else(|| UpdateError::NoInactiveSlot("missing active slot".into()))?;
    if active.state != SlotState::Pending {
        return Ok(false);
    }
    if passed {
        let version = active.version.clone();
        let slot = state.slot_mut(state.active);
        slot.state = SlotState::Active;
        // Keep the previous slot as the rollback target.
        let previous = state.active.other();
        let previous_slot = state.slot_mut(previous);
        if previous_slot.state == SlotState::Active
            || previous_slot.state == SlotState::Pending
        {
            previous_slot.state = SlotState::Good;
        }
        state.last_good_version = version;
        state.update_in_progress = false;
        state.boot_attempts = 0;
        Ok(false)
    } else {
        rollback(state)?;
        Ok(true)
    }
}

/// Return to the last good slot.
pub fn rollback(state: &mut UpdateState) -> Result<(), UpdateError> {
    let other = state.active.other();
    let candidate = state.slot(other).cloned();
    let rollback_to = match candidate {
        Some(slot) if matches!(slot.state, SlotState::Good | SlotState::Active) => Some(slot.id),
        _ => None,
    };
    let target = rollback_to.ok_or(UpdateError::NoRollbackTarget)?;

    let failed = state.active;
    {
        let failed_slot = state.slot_mut(failed);
        failed_slot.state = SlotState::Bad;
    }
    state.active = target;
    {
        let good = state.slot_mut(target);
        good.state = SlotState::Active;
    }
    state.boot_attempts = 0;
    state.update_in_progress = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> UpdateManifest {
        UpdateManifest {
            version: version.into(),
            image_sha256: "abc123".into(),
            signature: None,
            min_version: None,
            notes: String::new(),
        }
    }

    fn seeded() -> UpdateState {
        let mut state = UpdateState::default();
        {
            let slot = state.slot_mut(SlotId::A);
            slot.version = Some("1.0.0".into());
            slot.state = SlotState::Active;
            slot.checksum = Some("old".into());
        }
        state.last_good_version = Some("1.0.0".into());
        state
    }

    #[test]
    fn install_writes_pending_to_the_inactive_slot() {
        let mut state = seeded();
        install(&mut state, &manifest("1.1.0"), "abc123", true).unwrap();
        let slot = state.inactive_slot().unwrap();
        assert_eq!(slot.state, SlotState::Pending);
        assert_eq!(slot.version.as_deref(), Some("1.1.0"));
        assert_eq!(state.active, SlotId::A);
    }

    #[test]
    fn checksum_and_signature_failures_stop_the_update() {
        let mut state = seeded();
        assert!(matches!(
            install(&mut state, &manifest("1.1.0"), "wrong", true),
            Err(UpdateError::ChecksumMismatch { .. })
        ));
        let mut signed = manifest("1.1.0");
        signed.signature = Some("deadbeef".into());
        assert!(matches!(
            install(&mut state, &signed, "abc123", false),
            Err(UpdateError::BadSignature)
        ));
        assert_eq!(state.inactive_slot().unwrap().state, SlotState::Empty);
    }

    #[test]
    fn downgrades_are_rejected() {
        let mut state = seeded();
        assert!(matches!(
            install(&mut state, &manifest("0.9.0"), "abc123", true),
            Err(UpdateError::Downgrade { .. })
        ));
    }

    #[test]
    fn too_many_failed_boots_roll_back() {
        let mut state = seeded();
        install(&mut state, &manifest("1.1.0"), "abc123", true).unwrap();
        activate_pending(&mut state).unwrap();
        assert_eq!(state.active, SlotId::B);
        assert!(!on_boot(&mut state).unwrap());
        assert!(!on_boot(&mut state).unwrap());
        assert!(!on_boot(&mut state).unwrap());
        assert!(on_boot(&mut state).unwrap(), "fourth boot must roll back");
        assert_eq!(state.active, SlotId::A);
        assert_eq!(state.active_slot().unwrap().state, SlotState::Active);
        assert_eq!(state.slot(SlotId::B).unwrap().state, SlotState::Bad);
    }

    #[test]
    fn successful_health_check_confirms_the_update() {
        let mut state = seeded();
        install(&mut state, &manifest("1.1.0"), "abc123", true).unwrap();
        activate_pending(&mut state).unwrap();
        on_boot(&mut state).unwrap();
        assert!(!health_check(&mut state, true).unwrap());
        assert_eq!(state.active_slot().unwrap().state, SlotState::Active);
        assert_eq!(
            state.slot(SlotId::A).unwrap().state,
            SlotState::Good,
            "previous slot becomes the rollback target"
        );
        assert!(!state.update_in_progress);
    }

    #[test]
    fn failed_health_check_rolls_back() {
        let mut state = seeded();
        install(&mut state, &manifest("1.1.0"), "abc123", true).unwrap();
        activate_pending(&mut state).unwrap();
        assert!(health_check(&mut state, false).unwrap());
        assert_eq!(state.active, SlotId::A);
    }

    #[test]
    fn rollback_without_a_good_slot_fails() {
        let mut state = UpdateState::default();
        assert!(matches!(
            rollback(&mut state),
            Err(UpdateError::NoRollbackTarget)
        ));
    }

    #[test]
    fn state_round_trips() {
        let mut state = seeded();
        install(&mut state, &manifest("1.1.0"), "abc123", true).unwrap();
        let dir = std::env::temp_dir().join(format!("aether-update-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        state.save(&path).unwrap();
        let loaded = UpdateState::load(&path).unwrap();
        assert_eq!(loaded, state);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn staging_the_inactive_slot_marks_it_pending() {
        let mut state = seeded();
        stage_pending(&mut state, SlotId::B).unwrap();
        assert_eq!(state.slots.get("b").unwrap().state, SlotState::Pending);
        assert!(state.update_in_progress);
        assert!(matches!(
            stage_pending(&mut state, SlotId::A),
            Err(UpdateError::NoInactiveSlot(_))
        ));
    }

    #[test]
    fn version_compare_is_numeric() {
        assert!(version_cmp("1.10", "1.9") == std::cmp::Ordering::Greater);
        assert!(version_cmp("2.0", "2.0.0") == std::cmp::Ordering::Equal);
    }
}
