//! Permission broker (P6-T04, D8 section 5).
//!
//! A permission is granted only when the user accepts it. Denied permissions
//! stay denied across restarts (they are persisted, not recomputed). The
//! broker never grants a permission because the manifest asked for it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aether_pkg::manifest::{AppId, FileScope, PackageManifest, Permission};

/// Asks the user about one permission.
pub trait PermissionPrompt {
    fn ask(&self, app: &AppId, permission: &Permission) -> bool;
}

/// Non-interactive prompt used by tests and `--yes` flows.
pub struct AutoPrompt {
    pub accept: bool,
}

impl PermissionPrompt for AutoPrompt {
    fn ask(&self, _app: &AppId, _permission: &Permission) -> bool {
        self.accept
    }
}

/// A prompt that accepts an explicit allow-list and denies the rest.
pub struct AllowListPrompt {
    pub allowed: BTreeSet<String>,
}

impl PermissionPrompt for AllowListPrompt {
    fn ask(&self, _app: &AppId, permission: &Permission) -> bool {
        self.allowed.contains(&permission.key())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionStore {
    /// app id -> granted permission keys
    pub grants: BTreeMap<String, BTreeSet<String>>,
    /// app id -> explicitly denied keys (kept so a restart never re-prompts
    /// silently)
    pub denied: BTreeMap<String, BTreeSet<String>>,
}

impl PermissionStore {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }

    pub fn granted(&self, id: &AppId) -> BTreeSet<String> {
        self.grants.get(id.as_str()).cloned().unwrap_or_default()
    }

    pub fn is_granted(&self, id: &AppId, permission: &Permission) -> bool {
        self.granted(id).contains(&permission.key())
    }

    pub fn is_denied(&self, id: &AppId, permission: &Permission) -> bool {
        self.denied
            .get(id.as_str())
            .map(|set| set.contains(&permission.key()))
            .unwrap_or(false)
    }

    pub fn grant(&mut self, id: &AppId, permission: &Permission) {
        let key = permission.key();
        self.grants
            .entry(id.as_str().to_string())
            .or_default()
            .insert(key.clone());
        if let Some(denied) = self.denied.get_mut(id.as_str()) {
            denied.remove(&key);
        }
    }

    pub fn revoke(&mut self, id: &AppId, permission: &Permission) {
        let key = permission.key();
        if let Some(grants) = self.grants.get_mut(id.as_str()) {
            grants.remove(&key);
        }
        self.denied
            .entry(id.as_str().to_string())
            .or_default()
            .insert(key);
    }

    /// Keys the manifest asks for but the store has not granted.
    pub fn missing(&self, manifest: &PackageManifest) -> Vec<Permission> {
        manifest
            .permissions
            .iter()
            .filter(|permission| !self.is_granted(&manifest.id, permission))
            .cloned()
            .collect()
    }
}

/// Permission broker.
pub struct Broker {
    pub path: PathBuf,
    pub store: PermissionStore,
}

impl Broker {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let store = PermissionStore::load(&path);
        Self { path, store }
    }

    /// Prompt for every missing permission; a denied permission is persisted
    /// as denied and is reported in the returned list.
    pub fn request(
        &mut self,
        manifest: &PackageManifest,
        prompt: &dyn PermissionPrompt,
    ) -> io::Result<Vec<Permission>> {
        let mut denied = Vec::new();
        for permission in &manifest.permissions {
            if self.store.is_granted(&manifest.id, permission) {
                continue;
            }
            if prompt.ask(&manifest.id, permission) {
                self.store.grant(&manifest.id, permission);
            } else {
                self.store.revoke(&manifest.id, permission);
                denied.push(permission.clone());
            }
        }
        self.store.save(&self.path)?;
        Ok(denied)
    }

    pub fn grant(&mut self, id: &AppId, permission: &Permission) -> io::Result<()> {
        self.store.grant(id, permission);
        self.store.save(&self.path)
    }

    pub fn revoke(&mut self, id: &AppId, permission: &Permission) -> io::Result<()> {
        self.store.revoke(id, permission);
        self.store.save(&self.path)
    }
}

/// Default file scopes shown by the prompt (F9 AP-AC-05).
pub fn describe(permission: &Permission) -> String {
    match permission {
        Permission::Files { scope } => match scope {
            FileScope::PickedByUser => {
                "Read files you pick in the file chooser (no folder access)".into()
            }
            FileScope::AppFolder => "Read and write only its own app data folder".into(),
            FileScope::Home => "Read your home folder (not recommended)".into(),
        },
        other => other.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(permissions: Vec<Permission>) -> PackageManifest {
        PackageManifest {
            id: AppId::new("org.example.App").unwrap(),
            name: "App".into(),
            version: "1".into(),
            kind: aether_pkg::PackageKind::NativeBundle,
            permissions,
            entry: "app".into(),
            installed_bytes: 0,
            signature: None,
            files: vec![],
            description: None,
            homepage: None,
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "aether-perm-{tag}-{}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn acceptance_grants_only_what_is_accepted() {
        let path = temp_path("accept");
        let mut broker = Broker::load(&path);
        let manifest = manifest(vec![Permission::Network, Permission::Camera]);
        let prompt = AllowListPrompt {
            allowed: ["network".to_string()].into_iter().collect(),
        };
        let denied = broker.request(&manifest, &prompt).unwrap();
        assert_eq!(denied, vec![Permission::Camera]);
        assert!(broker.store.is_granted(&manifest.id, &Permission::Network));
        assert!(!broker.store.is_granted(&manifest.id, &Permission::Camera));
        assert!(broker.store.is_denied(&manifest.id, &Permission::Camera));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn denial_persists_across_restarts() {
        let path = temp_path("persist");
        let manifest = manifest(vec![Permission::Microphone]);
        {
            let mut broker = Broker::load(&path);
            let prompt = AutoPrompt { accept: false };
            broker.request(&manifest, &prompt).unwrap();
        }
        let broker = Broker::load(&path);
        assert!(broker.store.is_denied(&manifest.id, &Permission::Microphone));
        assert!(!broker.store.is_granted(&manifest.id, &Permission::Microphone));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn grant_and_revoke_round_trip() {
        let path = temp_path("revoke");
        let mut broker = Broker::load(&path);
        let id = AppId::new("org.example.App").unwrap();
        broker.grant(&id, &Permission::Network).unwrap();
        assert!(Broker::load(&path).store.is_granted(&id, &Permission::Network));
        broker.revoke(&id, &Permission::Network).unwrap();
        let reloaded = Broker::load(&path);
        assert!(!reloaded.store.is_granted(&id, &Permission::Network));
        assert!(reloaded.store.is_denied(&id, &Permission::Network));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_lists_ungranted_permissions() {
        let path = temp_path("missing");
        let mut broker = Broker::load(&path);
        let manifest = manifest(vec![
            Permission::Network,
            Permission::Files {
                scope: FileScope::PickedByUser,
            },
        ]);
        broker.grant(&manifest.id, &Permission::Network).unwrap();
        assert_eq!(broker.store.missing(&manifest).len(), 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn describe_is_plain_language() {
        assert!(describe(&Permission::Camera).contains("Camera"));
        assert!(describe(&Permission::Files {
            scope: FileScope::PickedByUser
        })
        .contains("file chooser"));
    }
}
