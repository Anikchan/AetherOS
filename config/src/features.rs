//! Optional feature pack manager (P7-T07, F15).
//!
//! Settings → Features lists every pack with status, disk use and idle cost
//! before install (OPT-AC-02). Installing a pack that would push the idle cost
//! past the budget is refused (G-OPT1/OPT-AC-04); removal clears the pack's
//! state so nothing is left behind (G-OPT3).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct FeaturePack {
    pub id: &'static str,
    pub name: &'static str,
    pub disk_mb: u64,
    pub memory_mb: u64,
    pub idle_cpu_percent: f64,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IdleBudget {
    pub cpu_percent: f64,
    pub memory_mb: u64,
}

impl Default for IdleBudget {
    fn default() -> Self {
        // Core budget headroom for packs (F6 RES-03/04).
        Self {
            cpu_percent: 1.0,
            memory_mb: 256,
        }
    }
}

/// The packs from F15 section 3.
pub fn catalog() -> Vec<FeaturePack> {
    vec![
        FeaturePack { id: "A", name: "Android apps", disk_mb: 1500, memory_mb: 150, idle_cpu_percent: 0.2, description: "Android container and tools; frozen when not in use" },
        FeaturePack { id: "B", name: "Windows apps", disk_mb: 1000, memory_mb: 0, idle_cpu_percent: 0.0, description: "Windows engine (Wine) and prefix tools" },
        FeaturePack { id: "C", name: "AI assistant", disk_mb: 3500, memory_mb: 0, idle_cpu_percent: 0.0, description: "Local model runtime; 0 MB while unloaded" },
        FeaturePack { id: "D", name: "Advanced monitoring", disk_mb: 20, memory_mb: 30, idle_cpu_percent: 0.5, description: "Per-app network, per-process GPU, 1 s sensor logs" },
        FeaturePack { id: "E", name: "Screen Time limits", disk_mb: 10, memory_mb: 10, idle_cpu_percent: 0.1, description: "Daily limits and downtime" },
        FeaturePack { id: "F", name: "Battery reports", disk_mb: 10, memory_mb: 0, idle_cpu_percent: 0.0, description: "Full text reports and long history" },
        FeaturePack { id: "G", name: "Storage analyzer", disk_mb: 15, memory_mb: 0, idle_cpu_percent: 0.0, description: "Disk usage map and large file finder" },
    ]
}

pub fn pack(id: &str) -> Option<FeaturePack> {
    catalog()
        .into_iter()
        .find(|pack| pack.id.eq_ignore_ascii_case(id))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureState {
    #[serde(default)]
    pub installed: BTreeSet<String>,
    #[serde(default)]
    pub enabled: BTreeSet<String>,
}

#[derive(Debug, Error)]
pub enum FeatureError {
    #[error("pack {0} does not exist")]
    Unknown(String),
    #[error("pack {0} is already installed")]
    AlreadyInstalled(String),
    #[error("pack {0} is not installed")]
    NotInstalled(String),
    #[error("idle budget would be exceeded: {reason}")]
    OverBudget { reason: String },
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("state error: {0}")]
    State(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureRow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub disk_used_mb: u64,
    pub idle_cpu_percent: f64,
    pub memory_mb: u64,
    pub description: String,
}

#[derive(Debug)]
pub struct FeatureManager {
    path: PathBuf,
    pub state: FeatureState,
    pub budget: IdleBudget,
}

impl FeatureManager {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let state = fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            state,
            budget: IdleBudget::default(),
        }
    }

    pub fn with_budget(mut self, budget: IdleBudget) -> Self {
        self.budget = budget;
        self
    }

    fn persist(&self) -> Result<(), FeatureError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&self.state)
            .map_err(|error| FeatureError::State(error.to_string()))?;
        fs::write(&self.path, text)?;
        Ok(())
    }

    fn installed_cost(&self) -> (f64, u64) {
        let mut cpu = 0.0;
        let mut memory = 0u64;
        for id in &self.state.installed {
            if let Some(pack) = pack(id) {
                cpu += pack.idle_cpu_percent;
                memory += pack.memory_mb;
            }
        }
        (cpu, memory)
    }

    /// Cost check before install (OPT-AC-02).
    pub fn cost_preview(&self, id: &str) -> Result<FeatureRow, FeatureError> {
        let pack = pack(id).ok_or_else(|| FeatureError::Unknown(id.to_string()))?;
        Ok(FeatureRow {
            id: pack.id.into(),
            name: pack.name.into(),
            status: if self.state.installed.contains(pack.id) {
                "Installed"
            } else {
                "Not installed"
            }
            .into(),
            disk_used_mb: pack.disk_mb,
            idle_cpu_percent: pack.idle_cpu_percent,
            memory_mb: pack.memory_mb,
            description: pack.description.into(),
        })
    }

    pub fn install(&mut self, id: &str) -> Result<FeatureRow, FeatureError> {
        let pack = pack(id).ok_or_else(|| FeatureError::Unknown(id.to_string()))?;
        if self.state.installed.contains(pack.id) {
            return Err(FeatureError::AlreadyInstalled(pack.id.into()));
        }
        let (cpu, memory) = self.installed_cost();
        let new_cpu = cpu + pack.idle_cpu_percent;
        let new_memory = memory + pack.memory_mb;
        if new_cpu > self.budget.cpu_percent {
            return Err(FeatureError::OverBudget {
                reason: format!(
                    "idle CPU would become {new_cpu:.1}% (budget {:.1}%)",
                    self.budget.cpu_percent
                ),
            });
        }
        if new_memory > self.budget.memory_mb {
            return Err(FeatureError::OverBudget {
                reason: format!(
                    "idle memory would become {new_memory} MB (budget {} MB)",
                    self.budget.memory_mb
                ),
            });
        }
        self.state.installed.insert(pack.id.to_string());
        self.state.enabled.insert(pack.id.to_string());
        self.persist()?;
        self.cost_preview(pack.id)
    }

    /// Remove the pack state (G-OPT3). Pack payload removal is delegated to
    /// the package manager, which owns those files.
    pub fn remove(&mut self, id: &str) -> Result<(), FeatureError> {
        let pack = pack(id).ok_or_else(|| FeatureError::Unknown(id.to_string()))?;
        if !self.state.installed.remove(pack.id) {
            return Err(FeatureError::NotInstalled(pack.id.into()));
        }
        self.state.enabled.remove(pack.id);
        self.persist()
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), FeatureError> {
        let pack = pack(id).ok_or_else(|| FeatureError::Unknown(id.to_string()))?;
        if !self.state.installed.contains(pack.id) {
            return Err(FeatureError::NotInstalled(pack.id.into()));
        }
        if enabled {
            self.state.enabled.insert(pack.id.to_string());
        } else {
            self.state.enabled.remove(pack.id);
        }
        self.persist()
    }

    pub fn rows(&self) -> Vec<FeatureRow> {
        catalog()
            .into_iter()
            .map(|pack| {
                let status = if !self.state.installed.contains(pack.id) {
                    "Not installed"
                } else if self.state.enabled.contains(pack.id) {
                    "Installed"
                } else {
                    "Disabled"
                };
                FeatureRow {
                    id: pack.id.into(),
                    name: pack.name.into(),
                    status: status.into(),
                    disk_used_mb: if self.state.installed.contains(pack.id) {
                        pack.disk_mb
                    } else {
                        0
                    },
                    idle_cpu_percent: pack.idle_cpu_percent,
                    memory_mb: pack.memory_mb,
                    description: pack.description.into(),
                }
            })
            .collect()
    }

    pub fn total_disk_mb(&self) -> u64 {
        self.state
            .installed
            .iter()
            .filter_map(|id| pack(id))
            .map(|pack| pack.disk_mb)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-features-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("features.toml")
    }

    #[test]
    fn catalog_matches_f15() {
        let catalog = catalog();
        assert_eq!(catalog.len(), 7);
        for id in ["A", "B", "C", "D", "E", "F", "G"] {
            assert!(pack(id).is_some(), "missing pack {id}");
        }
        assert_eq!(pack("a").unwrap().name, "Android apps");
    }

    #[test]
    fn install_remove_round_trip() {
        let path = temp_path("roundtrip");
        let mut manager = FeatureManager::load(&path);
        let row = manager.install("D").unwrap();
        assert_eq!(row.status, "Installed");
        assert_eq!(manager.total_disk_mb(), 20);

        let reloaded = FeatureManager::load(&path);
        assert!(reloaded.state.installed.contains("D"));

        manager.remove("D").unwrap();
        assert!(manager.state.installed.is_empty());
        let reloaded = FeatureManager::load(&path);
        assert!(reloaded.state.installed.is_empty());
        assert!(matches!(
            manager.remove("D"),
            Err(FeatureError::NotInstalled(_))
        ));
    }

    #[test]
    fn over_budget_install_is_refused() {
        let path = temp_path("budget");
        let mut manager = FeatureManager::load(&path)
            .with_budget(IdleBudget { cpu_percent: 0.6, memory_mb: 256 });
        manager.install("D").unwrap(); // 0.5% CPU
        assert!(matches!(
            manager.install("A"),
            Err(FeatureError::OverBudget { .. })
        ));
        assert!(!manager.state.installed.contains("A"));
    }

    #[test]
    fn cost_preview_shows_disk_and_idle_cost() {
        let path = temp_path("preview");
        let manager = FeatureManager::load(&path);
        let row = manager.cost_preview("C").unwrap();
        assert_eq!(row.status, "Not installed");
        assert_eq!(row.disk_used_mb, 3500);
        assert_eq!(row.memory_mb, 0);
        assert!(row.description.contains("model"));
    }

    #[test]
    fn rows_cover_every_pack() {
        let path = temp_path("rows");
        let manager = FeatureManager::load(&path);
        let rows = manager.rows();
        assert_eq!(rows.len(), 7);
        assert!(rows.iter().all(|row| row.status == "Not installed"));
    }

    #[test]
    fn disable_keeps_the_pack_installed() {
        let path = temp_path("disable");
        let mut manager = FeatureManager::load(&path);
        manager.install("E").unwrap();
        manager.set_enabled("E", false).unwrap();
        assert!(manager.state.installed.contains("E"));
        assert!(!manager.state.enabled.contains("E"));
        assert_eq!(
            manager.rows().iter().find(|row| row.id == "E").unwrap().status,
            "Disabled"
        );
    }
}
