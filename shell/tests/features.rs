//! Headless test for Settings → Features (P7-T07, F15 OPT-AC-01..05).
//!
//! Drives the real controller/surface against a temporary config root, so the
//! tests prove the UI reflects the S4 feature manager and its persisted state.
//! One test function: the testing backend installs a process-wide platform.

use std::path::PathBuf;

use slint::{ComponentHandle, Model};

use aether_shell::config::{Category, SettingsStore};
use aether_shell::services::Services;
use aether_shell::{PowerMode, ShellController, ShellRoot};

#[derive(Default)]
struct NoopServices;

impl Services for NoopServices {
    fn set_wifi(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_bluetooth(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_brightness(&mut self, _brightness: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_power_mode(&mut self, _mode: PowerMode) -> Result<(), String> {
        Ok(())
    }
    fn battery(&mut self) -> Option<aether_shell::services::Battery> {
        None
    }
    fn lock_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-shell-features-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn settings_features_installs_and_persists_packs() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let root = temp_root("ui");
    let features_file = root.join("features").join("features.toml");
    let ui = ShellRoot::new().unwrap();
    ui.show().unwrap();
    let controller = ShellController::with_demo_content(
        ui,
        Box::new(NoopServices),
        SettingsStore::new(root.clone()),
    );

    // --- the Features category is in the sidebar and opens its page ------
    controller.open_settings_category(Category::Features);
    let ui = controller.ui();
    assert_eq!(
        controller.state().settings_page,
        Category::FEATURES_INDEX,
        "Features sits at a stable sidebar index"
    );
    assert_eq!(ui.get_settings_page(), Category::FEATURES_INDEX as i32);
    assert_eq!(ui.get_feature_rows().row_count(), 7);

    let rows = ui.get_feature_rows();
    let first = rows.row_data(0).unwrap();
    assert_eq!(first.id, "A");
    assert!(!first.installed);
    assert!(first.disk.contains("1500 MB"), "cost shown before install");
    assert!(ui.get_feature_budget().to_string().contains("Idle budget"));
    assert!(ui.get_feature_usage().to_string().starts_with("0 packs installed"));
    assert!(ui.get_feature_message().to_string().is_empty());

    // --- install pack D: row, message and file all update -----------------
    controller.install_feature("D");
    let rows = controller.ui().get_feature_rows();
    let d = (0..rows.row_count())
        .map(|index| rows.row_data(index).unwrap())
        .find(|row| row.id == "D")
        .unwrap();
    assert!(d.installed);
    assert!(d.enabled);
    assert_eq!(d.status, "Installed");
    assert!(controller
        .ui()
        .get_feature_message()
        .to_string()
        .contains("Advanced monitoring"));
    assert!(!controller.ui().get_feature_failed());
    assert!(
        features_file.exists(),
        "features.toml is written for aetherctl and the next session"
    );
    let saved = std::fs::read_to_string(&features_file).unwrap();
    assert!(saved.contains("D"), "persisted state names the pack: {saved}");
    assert!(controller
        .ui()
        .get_feature_usage()
        .to_string()
        .contains("0.5% of 1.0% idle CPU"));

    // --- disable keeps it installed; remove clears it ---------------------
    controller.set_feature_enabled("D", false);
    let rows = controller.ui().get_feature_rows();
    let d = (0..rows.row_count())
        .map(|index| rows.row_data(index).unwrap())
        .find(|row| row.id == "D")
        .unwrap();
    assert!(d.installed);
    assert!(!d.enabled);
    assert_eq!(d.status, "Disabled");

    controller.remove_feature("D");
    let rows = controller.ui().get_feature_rows();
    let d = (0..rows.row_count())
        .map(|index| rows.row_data(index).unwrap())
        .find(|row| row.id == "D")
        .unwrap();
    assert!(!d.installed);
    assert!(controller.ui().get_feature_message().to_string().contains("Removed"));

    // --- an unknown pack is reported, flagged as a failure ----------------
    controller.install_feature("Z");
    assert!(controller.ui().get_feature_failed());
    assert!(controller
        .ui()
        .get_feature_message()
        .to_string()
        .contains("does not exist"));

    // --- Settings → Features state round-trips through a fresh controller -
    let reloaded = aether_shell::FeaturesPage::load(&features_file);
    assert!(reloaded.rows().iter().all(|row| !row.installed));

    std::fs::remove_dir_all(&root).ok();
}
