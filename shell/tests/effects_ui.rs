//! Control-centre effect-tier interaction through the real widget tree (P4-T04).
//!
//! One test function per file because the Slint testing backend installs a
//! process-wide platform. The tier switch must persist through aether-config,
//! drive every panel's glass tier, and publish the compositor env.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::ComponentHandle;

use aether_shell::config::{Category, SettingsStore};
use aether_shell::model::Overlay;
use aether_shell::services::{Battery, Services};
use aether_shell::{GlassTier, PowerMode, ShellController, ShellRoot};

#[derive(Default)]
struct QuietServices {
    calls: Rc<RefCell<Vec<String>>>,
}

impl Services for QuietServices {
    fn set_wifi(&mut self, on: bool) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("wifi {on}"));
        Ok(())
    }
    fn set_bluetooth(&mut self, on: bool) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("bluetooth {on}"));
        Ok(())
    }
    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_brightness(&mut self, _brightness: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_power_mode(&mut self, mode: PowerMode) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("power {}", mode.as_str()));
        Ok(())
    }
    fn battery(&mut self) -> Option<Battery> {
        Some(Battery {
            percent: 84,
            charging: false,
        })
    }
    fn lock_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn temp_config_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-shell-effects-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Click the visible instance with this accessible label.
async fn click(ui: &ShellRoot, label: &str) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |h: &ElementHandle| h.size().width * h.size().height;
        area(b).partial_cmp(&area(a)).unwrap()
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

fn stored_tier(controller: &ShellController) -> Option<String> {
    controller
        .store()
        .load("theme.effects-tier")
        .unwrap()
        .and_then(|value| value.as_str().map(str::to_string))
}

#[test]
fn the_control_centre_drives_the_glass_tier() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_config_root();
        let ui = ShellRoot::new().unwrap();
        let controller = ShellController::new(
            ui,
            Box::new(QuietServices::default()),
            SettingsStore::new(root.clone()),
        );
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1440, 900));

        // Fresh profile: Full is the documented default (D1 section 4).
        assert_eq!(controller.effect_tier().as_str(), "full");
        assert_eq!(controller.ui().get_tier(), GlassTier::Full);

        let ui = controller.ui();
        controller.open(Overlay::ControlCenter);
        assert_eq!(ui.get_overlay(), Overlay::ControlCenter.code());
        assert_eq!(ui.get_effects_tier().to_string(), "full");
        assert!(
            ui.get_effects_note().to_string().contains("Blur"),
            "the panel names the material: {}",
            ui.get_effects_note()
        );

        // Reduced: persisted to theme/tokens.toml, live on every panel, and
        // published as the compositor env.
        click(ui, "Reduced").await;
        assert_eq!(stored_tier(&controller).as_deref(), Some("reduced"));
        assert_eq!(controller.effect_tier().as_str(), "reduced");
        assert_eq!(ui.get_effects_tier().to_string(), "reduced");
        assert_eq!(ui.get_tier(), GlassTier::Reduced);
        assert_eq!(
            controller.effect_env_line(),
            "AETHER_COMPOSITOR_EFFECT_TIER=reduced"
        );
        assert!(
            ui.get_effects_note().to_string().contains("Half-resolution"),
            "{}",
            ui.get_effects_note()
        );
        assert!(
            controller
                .log
                .borrow()
                .iter()
                .any(|line| line.contains("effects tier=reduced")),
            "the change is logged for the acceptance harness: {:?}",
            controller.log.borrow()
        );

        // Minimal, then back to Full (the switch is a radio, not a cycle).
        click(ui, "Minimal").await;
        assert_eq!(stored_tier(&controller).as_deref(), Some("minimal"));
        assert_eq!(ui.get_tier(), GlassTier::Minimal);
        assert!(ui
            .get_effects_note()
            .to_string()
            .contains("Solid panels"));
        click(ui, "Full").await;
        assert_eq!(stored_tier(&controller).as_deref(), Some("full"));
        assert_eq!(ui.get_tier(), GlassTier::Full);

        // The settings app writes the same key; both paths must agree.
        controller.settings_category(Category::Appearance);
        click(ui, "Effects tier").await;
        assert_eq!(
            stored_tier(&controller).as_deref(),
            Some("reduced"),
            "a select row cycles full -> reduced"
        );
        assert_eq!(ui.get_effects_tier().to_string(), "reduced");
        assert_eq!(ui.get_tier(), GlassTier::Reduced);

        // Reduce transparency is a hard accessibility override: the requested
        // tier stays reduced, the effective tier becomes minimal.
        click(ui, "Reduce transparency").await;
        assert_eq!(
            controller
                .store()
                .load("theme.reduce-transparency")
                .unwrap()
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(controller.effect_settings().requested.as_str(), "reduced");
        assert_eq!(controller.effect_tier().as_str(), "minimal");
        assert_eq!(ui.get_tier(), GlassTier::Minimal);
        assert_eq!(
            controller.effect_env_line(),
            "AETHER_COMPOSITOR_EFFECT_TIER=minimal"
        );
        // Turning it back off restores the requested tier.
        click(ui, "Reduce transparency").await;
        assert_eq!(controller.effect_tier().as_str(), "reduced");
        assert_eq!(ui.get_tier(), GlassTier::Reduced);

        // A fresh controller sees the persisted choice (no restart data loss).
        let reopened = ShellController::new(
            ShellRoot::new().unwrap(),
            Box::new(QuietServices::default()),
            SettingsStore::new(root.clone()),
        );
        assert_eq!(reopened.effect_tier().as_str(), "reduced");

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
