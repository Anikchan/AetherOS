//! Software app UI contract (P6-T09, D8 §9).
//!
//! The shell (session S3) binds to this module instead of calling the store
//! directly: one JSON request in, one JSON response out. Every operation the
//! basic Software app needs is here — list, search, install, update, remove —
//! plus the compatibility tier and the Tier D dual-boot guidance (APP-AC-07/08).
//!
//! The contract is UI-agnostic and headless, so it is exercised by the crate
//! tests and by `aether-store --json`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use aether_pkg::manifest::{AppId, PackageManifest};
use aether_pkg::{InstallOutcome, InstalledApp};

use crate::{version_cmp, IndexEntry, Store};

/// Shown for Tier D Windows apps (APP-AC-08, APP-11).
pub const DUAL_BOOT_GUIDANCE: &str = "This Windows app does not work on AetherOS yet. \
Keep Windows installed (dual boot) to run it.";

/// One UI action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum StoreRequest {
    List,
    Search {
        #[serde(default)]
        query: String,
    },
    Install {
        id: String,
        /// Permissions the user already accepted in the UI.
        #[serde(default)]
        granted: Vec<String>,
    },
    /// Empty `ids` means every app with an update available.
    Update {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        granted: Vec<String>,
    },
    Remove {
        id: String,
        #[serde(default)]
        granted: Vec<String>,
    },
}

/// One row in the Software app list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRow {
    pub id: String,
    pub name: String,
    /// Version offered by the repository.
    pub version: String,
    pub kind: String,
    #[serde(default)]
    pub description: String,
    /// Installed version, when the app is installed.
    #[serde(default)]
    pub installed: Option<String>,
    /// Newer version available, when an update exists.
    #[serde(default)]
    pub update: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Compatibility tier for Windows apps: `a`, `b`, `c`, `d` or absent.
    #[serde(default)]
    pub compat_tier: Option<String>,
    /// Dual-boot guidance, present only for Tier D (APP-AC-08).
    #[serde(default)]
    pub guidance: Option<String>,
}

impl AppRow {
    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    pub fn needs_dual_boot(&self) -> bool {
        self.compat_tier
            .as_deref()
            .map(|tier| tier.eq_ignore_ascii_case("d"))
            .unwrap_or(false)
    }
}

/// Result of one update/install in an `Update` batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRow {
    pub id: String,
    /// `updated`, `needs-permission`, `skipped` or `failed`.
    pub status: String,
    #[serde(default)]
    pub detail: String,
}

/// The UI answer. Always exactly one variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum StoreResponse {
    /// Reply to `List` and `Search`.
    Apps { apps: Vec<AppRow> },
    /// Reply to a successful `Install`.
    Installed { app: AppRow },
    /// Reply to `Install` when the user must accept permissions first.
    NeedsPermission {
        id: String,
        permissions: Vec<String>,
    },
    /// Reply to `Update`; each app reports its own outcome.
    Updated { changes: Vec<ChangeRow> },
    /// Reply to a successful `Remove`.
    Removed { id: String },
    /// Any failure; the UI shows the message and keeps the previous state.
    Failed { message: String },
}

/// Compatibility tiers by app id and by app name, filled from the runner's
/// database (P6-T10) so the store can show them without depending on the
/// runner crate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatIndex {
    pub tiers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct CompatFile {
    #[serde(default)]
    entries: Vec<CompatFileRow>,
}

#[derive(Debug, Deserialize)]
struct CompatFileRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    tier: String,
}

impl CompatIndex {
    pub fn insert(&mut self, key: impl Into<String>, tier: impl Into<String>) {
        let key = key.into().trim().to_ascii_lowercase();
        let tier = tier.into().trim().to_ascii_lowercase();
        if !key.is_empty() && !tier.is_empty() {
            self.tiers.insert(key, tier);
        }
    }

    /// Read a compatibility database written by `aether-runner compat`.
    /// Unknown fields are ignored, so the database can grow without breaking
    /// the store.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let file: CompatFile = serde_json::from_str(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut index = Self::default();
        for row in file.entries {
            index.insert(&row.name, &row.tier);
        }
        Ok(index)
    }

    /// Tier for an app, by id first, then by name.
    pub fn tier_for(&self, id: &str, name: &str) -> Option<&str> {
        self.tiers
            .get(&id.to_ascii_lowercase())
            .or_else(|| self.tiers.get(&name.to_ascii_lowercase()))
            .map(|tier| tier.as_str())
    }
}

fn row_for(
    entry: &IndexEntry,
    installed: Option<&InstalledApp>,
    newer: Option<&str>,
    compat: &CompatIndex,
) -> AppRow {
    let manifest = &entry.manifest;
    let installed_version = installed.map(|app| app.manifest.version.clone());
    let tier = compat
        .tier_for(manifest.id.as_str(), &manifest.name)
        .map(|tier| tier.to_string());
    let guidance = tier
        .as_deref()
        .filter(|tier| tier.eq_ignore_ascii_case("d"))
        .map(|_| DUAL_BOOT_GUIDANCE.to_string());
    AppRow {
        id: manifest.id.as_str().to_string(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        kind: manifest.kind.as_str().to_string(),
        description: manifest.description.clone().unwrap_or_default(),
        installed: installed_version,
        update: newer.map(|version| version.to_string()),
        permissions: permissions_of(manifest),
        compat_tier: tier,
        guidance,
    }
}

fn permissions_of(manifest: &PackageManifest) -> Vec<String> {
    manifest
        .permissions
        .iter()
        .map(|permission| permission.label())
        .collect()
}

fn installed_map(store: &Store) -> BTreeMap<String, InstalledApp> {
    store
        .installed()
        .into_iter()
        .map(|app| (app.manifest.id.as_str().to_string(), app))
        .collect()
}

/// Rows for `List` and `Search`, newest version per app, sorted by name.
pub fn rows(store: &Store, query: &str, compat: &CompatIndex) -> Vec<AppRow> {
    let installed = installed_map(store);
    store
        .index
        .search(query)
        .into_iter()
        .map(|entry| {
            let id = entry.manifest.id.as_str().to_string();
            let app = installed.get(&id);
            let newer = app.and_then(|app| {
                if version_cmp(&entry.manifest.version, &app.manifest.version)
                    == std::cmp::Ordering::Greater
                {
                    Some(entry.manifest.version.as_str())
                } else {
                    None
                }
            });
            row_for(entry, app, newer, compat)
        })
        .collect()
}

/// Build a single row from the index, falling back to an installed app that is
/// no longer in the repository.
fn row_for_id(store: &Store, id: &str, compat: &CompatIndex) -> Option<AppRow> {
    let installed = installed_map(store);
    if let Some(entry) = store.index.latest(id) {
        let app = installed.get(id);
        let newer = app.and_then(|app| {
            if version_cmp(&entry.manifest.version, &app.manifest.version)
                == std::cmp::Ordering::Greater
            {
                Some(entry.manifest.version.as_str())
            } else {
                None
            }
        });
        return Some(row_for(entry, app, newer, compat));
    }
    let app = installed.get(id)?;
    let manifest = &app.manifest;
    let tier = compat
        .tier_for(manifest.id.as_str(), &manifest.name)
        .map(|tier| tier.to_string());
    let guidance = tier
        .as_deref()
        .filter(|tier| tier.eq_ignore_ascii_case("d"))
        .map(|_| DUAL_BOOT_GUIDANCE.to_string());
    Some(AppRow {
        id: manifest.id.as_str().to_string(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        kind: manifest.kind.as_str().to_string(),
        description: manifest.description.clone().unwrap_or_default(),
        installed: Some(manifest.version.clone()),
        update: None,
        permissions: permissions_of(manifest),
        compat_tier: tier,
        guidance,
    })
}

/// Serve one request. Never panics: every failure becomes [`StoreResponse::Failed`].
pub fn handle_with_compat(store: &Store, request: &StoreRequest, compat: &CompatIndex) -> StoreResponse {
    match request {
        StoreRequest::List => StoreResponse::Apps {
            apps: rows(store, "", compat),
        },
        StoreRequest::Search { query } => StoreResponse::Apps {
            apps: rows(store, query, compat),
        },
        StoreRequest::Install { id, granted } => {
            match store.install(id, granted.iter().cloned()) {
                Ok(InstallOutcome::Installed(id)) => match row_for_id(store, id.as_str(), compat) {
                    Some(app) => StoreResponse::Installed { app },
                    None => StoreResponse::Failed {
                        message: format!("{id} was installed but is not listed"),
                    },
                },
                Ok(InstallOutcome::NeedsPermission(permissions)) => StoreResponse::NeedsPermission {
                    id: id.clone(),
                    permissions: permissions
                        .iter()
                        .map(|permission| permission.label())
                        .collect(),
                },
                Ok(InstallOutcome::Unsupported { reason, alternatives }) => StoreResponse::Failed {
                    message: if alternatives.is_empty() {
                        reason
                    } else {
                        format!("{reason} Alternatives: {}", alternatives.join(", "))
                    },
                },
                Err(error) => StoreResponse::Failed {
                    message: error.to_string(),
                },
            }
        }
        StoreRequest::Update { ids, granted } => {
            let targets: Vec<String> = if ids.is_empty() {
                store
                    .updates()
                    .into_iter()
                    .map(|(id, _, _)| id.as_str().to_string())
                    .collect()
            } else {
                ids.clone()
            };
            let mut changes = Vec::new();
            for id in targets {
                let result = store.install(&id, granted.iter().cloned());
                let change = match result {
                    Ok(InstallOutcome::Installed(_)) => ChangeRow {
                        id,
                        status: "updated".into(),
                        detail: String::new(),
                    },
                    Ok(InstallOutcome::NeedsPermission(permissions)) => ChangeRow {
                        id,
                        status: "needs-permission".into(),
                        detail: permissions
                            .iter()
                            .map(|permission| permission.label())
                            .collect::<Vec<_>>()
                            .join(", "),
                    },
                    Ok(InstallOutcome::Unsupported { reason, .. }) => ChangeRow {
                        id,
                        status: "skipped".into(),
                        detail: reason,
                    },
                    Err(error) => ChangeRow {
                        id,
                        status: "failed".into(),
                        detail: error.to_string(),
                    },
                };
                changes.push(change);
            }
            StoreResponse::Updated { changes }
        }
        StoreRequest::Remove { id, granted } => match AppId::new(id.clone()) {
            Ok(id) => match store.remove(&id, granted.iter().cloned()) {
                Ok(()) => StoreResponse::Removed {
                    id: id.as_str().to_string(),
                },
                Err(error) => StoreResponse::Failed {
                    message: error.to_string(),
                },
            },
            Err(error) => StoreResponse::Failed {
                message: error.to_string(),
            },
        },
    }
}

/// Serve one request without compatibility data.
pub fn handle(store: &Store, request: &StoreRequest) -> StoreResponse {
    handle_with_compat(store, request, &CompatIndex::default())
}

/// Parse a JSON request and answer with JSON. A malformed request still gets a
/// well-formed `failed` response, so the UI never has to handle a parse error.
pub fn handle_json(store: &Store, json: &str, compat: &CompatIndex) -> String {
    let response = match serde_json::from_str::<StoreRequest>(json) {
        Ok(request) => handle_with_compat(store, &request, compat),
        Err(error) => StoreResponse::Failed {
            message: format!("invalid request: {error}"),
        },
    };
    serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(r#"{{"status":"failed","message":"cannot encode response: {error}"}}"#)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{bundle, temp_root};
    use crate::StoreIndex;
    use aether_pkg::Paths;

    fn seeded(tag: &str) -> (std::path::PathBuf, Store) {
        let root = temp_root(tag);
        let repo = root.join("repo");
        bundle(&repo, "org.example.Text", "Text", "1.0.0");
        bundle(&repo, "org.example.Paint", "Paint", "2.0.0");
        let index = StoreIndex::scan(&repo).unwrap();
        let store = Store::new(Paths::at(root.join("store")), index);
        (root, store)
    }

    #[test]
    fn list_and_search_return_rows_with_install_state() {
        let (root, store) = seeded("contract-list");
        let response = handle(&store, &StoreRequest::List);
        let StoreResponse::Apps { apps } = response else {
            panic!("expected apps");
        };
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Paint");
        assert!(!apps[0].is_installed());

        store.install("org.example.Text", Vec::new()).unwrap();
        let StoreResponse::Apps { apps } = handle(
            &store,
            &StoreRequest::Search {
                query: "text".into(),
            },
        ) else {
            panic!("expected apps");
        };
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].installed.as_deref(), Some("1.0.0"));
        assert_eq!(apps[0].update, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_permissions_and_remove_round_trip() {
        let (root, store) = seeded("contract-install");
        // `Paint` has no permissions, so the flow installs straight away.
        let response = handle(
            &store,
            &StoreRequest::Install {
                id: "org.example.Paint".into(),
                granted: Vec::new(),
            },
        );
        let StoreResponse::Installed { app } = response else {
            panic!("expected install, got {response:?}");
        };
        assert_eq!(app.id, "org.example.Paint");
        assert_eq!(app.installed.as_deref(), Some("2.0.0"));

        let response = handle(
            &store,
            &StoreRequest::Remove {
                id: "org.example.Paint".into(),
                granted: Vec::new(),
            },
        );
        assert_eq!(
            response,
            StoreResponse::Removed {
                id: "org.example.Paint".into()
            }
        );
        assert!(store.installed().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn update_reports_a_newer_version_per_app() {
        let (root, store) = seeded("contract-update");
        let repo = root.join("repo");
        store.install("org.example.Text", Vec::new()).unwrap();
        bundle(&repo, "org.example.Text", "Text", "1.1.0");
        let index = StoreIndex::scan(&repo).unwrap();
        let store = Store::new(store.paths.clone(), index);

        let StoreResponse::Apps { apps } = handle(&store, &StoreRequest::List) else {
            panic!("expected apps");
        };
        let text = apps.iter().find(|row| row.name == "Text").unwrap();
        assert_eq!(text.update.as_deref(), Some("1.1.0"));

        let StoreResponse::Updated { changes } = handle(
            &store,
            &StoreRequest::Update {
                ids: Vec::new(),
                granted: Vec::new(),
            },
        ) else {
            panic!("expected updated");
        };
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].status, "updated");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failures_are_reported_not_panicked() {
        let (root, store) = seeded("contract-fail");
        assert!(matches!(
            handle(
                &store,
                &StoreRequest::Install {
                    id: "org.example.Nope".into(),
                    granted: Vec::new()
                }
            ),
            StoreResponse::Failed { .. }
        ));
        assert!(matches!(
            handle(
                &store,
                &StoreRequest::Remove {
                    id: "bad id".into(),
                    granted: Vec::new()
                }
            ),
            StoreResponse::Failed { .. }
        ));
        let json = handle_json(&store, "{not json", &CompatIndex::default());
        assert!(json.contains("\"status\":\"failed\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tier_d_rows_carry_dual_boot_guidance() {
        let (root, store) = seeded("contract-tier");
        let mut compat = CompatIndex::default();
        compat.insert("paint", "D");
        compat.insert("org.example.Text", "b");

        let StoreResponse::Apps { apps } = handle_with_compat(&store, &StoreRequest::List, &compat)
        else {
            panic!("expected apps");
        };
        let paint = apps.iter().find(|row| row.name == "Paint").unwrap();
        assert_eq!(paint.compat_tier.as_deref(), Some("d"));
        assert!(paint.needs_dual_boot());
        assert_eq!(paint.guidance.as_deref(), Some(DUAL_BOOT_GUIDANCE));

        let text = apps.iter().find(|row| row.name == "Text").unwrap();
        assert_eq!(text.compat_tier.as_deref(), Some("b"));
        assert!(text.guidance.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn compat_index_loads_the_runner_database() {
        let dir = std::env::temp_dir().join(format!("aether-compat-index-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("compat.json");
        fs::write(
            &path,
            r#"{"entries":[
                {"name":"Games app 1","category":"Games","tier":"d","notes":"anti-cheat"},
                {"name":"Browser app 1","tier":"a","score":100}
            ]}"#,
        )
        .unwrap();
        let index = CompatIndex::load(&path).unwrap();
        assert_eq!(index.tier_for("games-app-1", "Games app 1"), Some("d"));
        assert_eq!(index.tier_for("browser-app-1", "Browser app 1"), Some("a"));
        assert_eq!(index.tier_for("x", "y"), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn json_contract_round_trips() {
        let (root, store) = seeded("contract-json");
        let request = r#"{"action":"search","query":"paint"}"#;
        let answer = handle_json(&store, request, &CompatIndex::default());
        let response: StoreResponse = serde_json::from_str(&answer).unwrap();
        let StoreResponse::Apps { apps } = response else {
            panic!("expected apps");
        };
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Paint");

        let installed = handle_json(
            &store,
            r#"{"action":"install","id":"org.example.Text"}"#,
            &CompatIndex::default(),
        );
        assert!(installed.contains("\"status\":\"installed\""), "{installed}");
        let _ = fs::remove_dir_all(root);
    }
}
