//! Settings → Features (P7-T07, F15 OPT-AC-01..05).
//!
//! A thin presentation layer over `aether-config`'s [`FeatureManager`] (S4-owned
//! API, read-only here): the catalog, install/remove/enable rules and the idle
//! budget all live there. This module only turns manager rows into the surface's
//! `FeatureRowEntry`, formats the cost/budget labels, and keeps the last
//! user-facing message (including a refused over-budget install).
//!
//! Storage is the same file `aetherctl features` uses:
//! `~/.config/aether/features/features.toml`.

use std::path::PathBuf;

use aether_config::{
    feature_pack, FeatureError, FeatureManager, FeatureRow, IdleBudget,
};

use crate::FeatureRowEntry;

pub struct FeaturesPage {
    manager: FeatureManager,
    message: String,
    failed: bool,
}

impl FeaturesPage {
    /// Load the manager from the F4 user layer (`.../features/features.toml`).
    pub fn load(path: impl Into<PathBuf>) -> FeaturesPage {
        FeaturesPage {
            manager: FeatureManager::load(path),
            message: String::new(),
            failed: false,
        }
    }

    /// Tests and the renderer use a custom budget (F6 RES-03/04).
    pub fn with_budget(mut self, budget: IdleBudget) -> FeaturesPage {
        self.manager = self.manager.with_budget(budget);
        self
    }

    pub fn manager(&self) -> &FeatureManager {
        &self.manager
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    /// One row per pack, in catalog order (A–G).
    pub fn rows(&self) -> Vec<FeatureRowEntry> {
        self.manager.rows().into_iter().map(row_entry).collect()
    }

    pub fn install(&mut self, id: &str) {
        match self.manager.install(id) {
            Ok(row) => self.succeed(format!(
                "Installed {} — {} MB on disk, {:.1}% idle CPU",
                row.name, row.disk_used_mb, row.idle_cpu_percent
            )),
            Err(error) => self.fail(error),
        }
    }

    pub fn remove(&mut self, id: &str) {
        match self.manager.remove(id) {
            Ok(()) => {
                let name = feature_pack(id)
                    .map(|pack| pack.name.to_string())
                    .unwrap_or_else(|| id.to_string());
                self.succeed(format!("Removed {name}"));
            }
            Err(error) => self.fail(error),
        }
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        match self.manager.set_enabled(id, enabled) {
            Ok(()) => {
                let name = feature_pack(id)
                    .map(|pack| pack.name.to_string())
                    .unwrap_or_else(|| id.to_string());
                self.succeed(format!(
                    "{name} {}",
                    if enabled { "enabled" } else { "disabled" }
                ));
            }
            Err(error) => self.fail(error),
        }
    }

    /// OPT-AC-02: cost is visible before installing.
    pub fn budget_label(&self) -> String {
        format!(
            "Idle budget {:.1}% CPU · {} MB memory",
            self.manager.budget.cpu_percent, self.manager.budget.memory_mb
        )
    }

    /// What the installed packs consume right now.
    pub fn usage_label(&self) -> String {
        let (cpu, memory) = installed_cost(&self.manager);
        format!(
            "{} pack{} installed · {:.1}% of {:.1}% idle CPU · {} MB of {} MB · {} MB disk",
            self.manager.state.installed.len(),
            if self.manager.state.installed.len() == 1 { "" } else { "s" },
            cpu,
            self.manager.budget.cpu_percent,
            memory,
            self.manager.budget.memory_mb,
            self.manager.total_disk_mb()
        )
    }

    fn succeed(&mut self, message: String) {
        self.message = message;
        self.failed = false;
    }

    fn fail(&mut self, error: FeatureError) {
        self.message = error.to_string();
        self.failed = true;
    }
}

fn row_entry(row: FeatureRow) -> FeatureRowEntry {
    let installed = row.status != "Not installed";
    // The manager reports 0 MB for packs that are not installed; the catalog
    // always has the cost so OPT-AC-02 can show it before installing.
    let pack = feature_pack(&row.id);
    let disk_mb = pack.as_ref().map(|pack| pack.disk_mb).unwrap_or(row.disk_used_mb);
    let memory_mb = pack
        .as_ref()
        .map(|pack| pack.memory_mb)
        .unwrap_or(row.memory_mb);
    FeatureRowEntry {
        id: row.id.into(),
        name: row.name.into(),
        status: row.status.clone().into(),
        description: row.description.into(),
        // Cost is always shown, whether or not the pack is installed
        // (OPT-AC-02: know the cost before installing).
        disk: format!("{disk_mb} MB disk").into(),
        cpu: format!("{:.1}% idle CPU", row.idle_cpu_percent).into(),
        memory: if memory_mb == 0 {
            "0 MB while idle".into()
        } else {
            format!("{memory_mb} MB memory").into()
        },
        installed,
        enabled: row.status == "Installed",
    }
}

fn installed_cost(manager: &FeatureManager) -> (f64, u64) {
    let mut cpu = 0.0;
    let mut memory = 0u64;
    for id in &manager.state.installed {
        if let Some(pack) = feature_pack(id) {
            cpu += pack.idle_cpu_percent;
            memory += pack.memory_mb;
        }
    }
    (cpu, memory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-features-ui-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("features.toml")
    }

    #[test]
    fn rows_show_the_catalog_with_costs() {
        let page = FeaturesPage::load(temp_path("rows"));
        let rows = page.rows();
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0].id, "A");
        assert_eq!(rows[0].name, "Android apps");
        assert!(!rows[0].installed);
        assert!(rows[0].disk.contains("1500 MB"));
        assert!(rows[0].cpu.contains("0.2%"));
        assert!(rows[0].memory.contains("150 MB"));
        // Pack B reports 0 MB idle memory as a clear label, not "0 MB memory".
        assert!(rows[1].memory.contains("0 MB"));
        assert_eq!(rows[2].name, "AI assistant");
    }

    #[test]
    fn install_disable_remove_round_trip() {
        let path = temp_path("roundtrip");
        let mut page = FeaturesPage::load(&path);
        assert!(page.message().is_empty());

        page.install("D");
        assert!(!page.failed());
        assert!(page.message().contains("Advanced monitoring"));
        let rows = page.rows();
        let row = rows.iter().find(|row| row.id == "D").unwrap();
        assert!(row.installed);
        assert!(row.enabled);
        assert_eq!(row.status, "Installed");
        assert!(path.exists(), "state is persisted for aetherctl");

        page.set_enabled("D", false);
        assert!(page.message().contains("disabled"));
        let rows = page.rows();
        let row = rows.iter().find(|row| row.id == "D").unwrap();
        assert!(row.installed);
        assert!(!row.enabled);
        assert_eq!(row.status, "Disabled");

        page.remove("D");
        assert!(page.message().contains("Removed"));
        assert!(!page.rows().iter().find(|row| row.id == "D").unwrap().installed);
    }

    #[test]
    fn over_budget_install_is_refused_with_the_reason() {
        let path = temp_path("budget");
        let mut page = FeaturesPage::load(&path)
            .with_budget(IdleBudget { cpu_percent: 0.6, memory_mb: 256 });
        page.install("D"); // 0.5% idle
        assert!(!page.failed());
        page.install("A"); // would exceed the CPU budget
        assert!(page.failed());
        assert!(page.message().contains("idle budget"));
        assert!(page.message().contains("CPU"));
        assert!(!page.rows().iter().find(|row| row.id == "A").unwrap().installed);
    }

    #[test]
    fn unknown_pack_and_remove_before_install_are_reported() {
        let mut page = FeaturesPage::load(temp_path("errors"));
        page.install("Z");
        assert!(page.failed());
        assert!(page.message().contains("does not exist"));
        page.remove("E");
        assert!(page.failed());
        assert!(page.message().contains("not installed"));

        // The failure flag clears on the next successful action.
        page.install("E");
        assert!(!page.failed());
    }

    #[test]
    fn budget_and_usage_labels_track_the_state() {
        let mut page = FeaturesPage::load(temp_path("labels"));
        assert_eq!(page.budget_label(), "Idle budget 1.0% CPU · 256 MB memory");
        assert!(page.usage_label().starts_with("0 packs installed"));
        assert!(page.usage_label().contains("0.0% of 1.0% idle CPU"));

        page.install("D");
        page.install("E");
        let usage = page.usage_label();
        assert!(usage.contains("2 packs installed"));
        assert!(usage.contains("0.6% of 1.0% idle CPU"));
        assert!(usage.contains("40 MB of 256 MB"));
        assert!(usage.contains("30 MB disk"));
    }

    #[test]
    fn rows_mirror_the_aether_config_catalog_and_manager() {
        let page = FeaturesPage::load(temp_path("catalog"));
        let catalog = aether_config::feature_catalog();
        let rows = page.rows();
        assert_eq!(rows.len(), catalog.len());
        for (row, pack) in rows.iter().zip(catalog.iter()) {
            assert_eq!(row.id, pack.id);
            assert_eq!(row.name, pack.name);
            assert_eq!(row.description, pack.description);
            assert!(row.disk.starts_with(&format!("{} MB", pack.disk_mb)));
            assert!(row.cpu.starts_with(&format!("{:.1}%", pack.idle_cpu_percent)));
        }
        // The page never invents rows: it is a projection of the manager.
        let manager_rows = page.manager().rows();
        assert_eq!(manager_rows.len(), rows.len());
        for (row, managed) in rows.iter().zip(manager_rows.iter()) {
            assert_eq!(row.id, managed.id);
            assert_eq!(row.status, managed.status);
            assert_eq!(row.installed, managed.status != "Not installed");
        }
    }

    #[test]
    fn a_fresh_page_sees_the_persisted_install_state() {
        let path = temp_path("reload");
        let mut page = FeaturesPage::load(&path);
        page.install("D");
        page.install("E");
        page.set_enabled("E", false);

        // The next session/page reads the same file the manager wrote.
        let reloaded = FeaturesPage::load(&path);
        let rows = reloaded.rows();
        let d = rows.iter().find(|row| row.id == "D").unwrap();
        let e = rows.iter().find(|row| row.id == "E").unwrap();
        assert!(d.installed && d.enabled);
        assert_eq!(d.status, "Installed");
        assert!(e.installed && !e.enabled, "a disabled pack stays installed");
        assert_eq!(e.status, "Disabled");
        assert!(reloaded.usage_label().contains("2 packs installed"));
        assert!(reloaded.usage_label().contains("30 MB disk"));
        assert!(reloaded.message().is_empty(), "a reload has no stale message");

        // Removing one pack is persisted too.
        let mut reloaded = reloaded;
        reloaded.remove("D");
        let again = FeaturesPage::load(&path);
        assert!(!again.rows().iter().find(|row| row.id == "D").unwrap().installed);
        assert!(again.rows().iter().find(|row| row.id == "E").unwrap().installed);
    }

    #[test]
    fn repeated_actions_report_the_manager_errors() {
        let path = temp_path("errors");
        let mut page = FeaturesPage::load(&path);
        page.install("D");
        page.install("D");
        assert!(page.failed());
        assert!(page.message().contains("already installed"));

        page.set_enabled("A", false);
        assert!(page.failed());
        assert!(page.message().contains("not installed"));

        // The file still names exactly one pack: the refused calls wrote
        // nothing.
        let saved = std::fs::read_to_string(&path).unwrap();
        let state: aether_config::FeatureState = toml::from_str(&saved).unwrap();
        assert_eq!(state.installed.len(), 1, "state: {saved}");
        assert!(state.installed.contains("D"), "state: {saved}");
        assert!(state.enabled.contains("D"), "state: {saved}");
        assert!(!state.installed.contains("A"), "state: {saved}");

        page.set_enabled("D", true);
        assert!(!page.failed());
        assert!(page.message().contains("enabled"));
    }

    #[test]
    fn removing_the_last_pack_empties_the_persisted_state() {
        let path = temp_path("remove");
        let mut page = FeaturesPage::load(&path);
        page.install("G");
        assert!(std::fs::read_to_string(&path).unwrap().contains("G"));
        page.remove("G");
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("G"), "removal clears the pack state: {saved}");
        let reloaded = FeaturesPage::load(&path);
        assert!(reloaded
            .manager()
            .state
            .installed
            .is_empty());
        assert!(reloaded.usage_label().contains("0 packs installed"));
    }
}
