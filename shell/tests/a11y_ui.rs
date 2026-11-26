//! Accessibility interaction tests (F1 SHL-18, design D1 sections 8 and 9).
//!
//! One test function per file because the Slint testing backend installs a
//! process-wide platform. The test drives the real widget tree: the Appearance
//! rows persist reduce motion / text scale, the tokens reach every widget, and
//! keyboard navigation reaches all five shell surfaces with a visible 2 px
//! accent focus ring.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model};

use aether_shell::config::{Category, SettingsStore};
use aether_shell::kbdnav::Surface;
use aether_shell::model::Overlay;
use aether_shell::services::{Battery, Services};
use aether_shell::{PowerMode, ShellController, ShellRoot, Tokens};

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
    fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("volume {volume:.2}"));
        Ok(())
    }
    fn set_brightness(&mut self, brightness: f32) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("brightness {brightness:.2}"));
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
        "aether-shell-a11y-{}-{}",
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

fn stored_bool(controller: &ShellController, key: &str) -> Option<bool> {
    controller
        .store()
        .load(key)
        .unwrap()
        .and_then(|value| value.as_bool())
}

fn stored_string(controller: &ShellController, key: &str) -> Option<String> {
    controller
        .store()
        .load(key)
        .unwrap()
        .and_then(|value| value.as_str().map(str::to_string))
}

#[test]
fn accessibility_settings_and_keyboard_navigation_reach_every_surface() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_config_root();
        let ui = ShellRoot::new().unwrap();
        let controller = ShellController::with_demo_content(
            ui,
            Box::new(QuietServices::default()),
            SettingsStore::new(root.clone()),
        );
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1440, 900));

        // --- defaults: motion on, 100 %, Minimal only via transparency -----
        let ui = controller.ui();
        assert!(!controller.a11y().reduce_motion);
        assert_eq!(controller.a11y().text_scale_percent(), 100);
        assert_eq!(ui.get_text_scale(), 1.0);
        assert!(!ui.get_reduce_motion());
        assert!(!ui.get_focus_visible(), "no ring before keyboard input");
        assert_eq!(ui.get_focus_ring_width(), 2.0, "2 px focus outline (D1 §9)");
        assert_eq!(
            ui.get_focus_ring_color(),
            slint::Color::from_rgb_u8(0x4C, 0x8D, 0xFF),
            "the ring uses the accent colour"
        );

        // --- Appearance → Text scale cycles 100 → 125 % --------------------
        controller.settings_category(Category::Appearance);
        let labels: Vec<String> = ui
            .get_settings_rows()
            .iter()
            .map(|row| row.label.to_string())
            .collect();
        assert!(labels.iter().any(|label| label == "Text scale"), "{labels:?}");
        assert!(labels.iter().any(|label| label == "Reduce motion"), "{labels:?}");
        click(ui, "Text scale").await;
        assert_eq!(
            stored_string(&controller, "theme.text-scale").as_deref(),
            Some("125%")
        );
        assert_eq!(controller.a11y().text_scale_percent(), 125);
        assert_eq!(ui.get_text_scale(), 1.25);
        // The tokens every widget reads scale with it.
        let tokens = ui.global::<Tokens>();
        assert_eq!(tokens.get_text_scale(), 1.25);
        assert_eq!(tokens.get_font_body(), 16.25);
        assert_eq!(tokens.get_menubar_height(), 35.0);

        // The next step is 150 %, and the API clamps out-of-range requests.
        click(ui, "Text scale").await;
        assert_eq!(stored_string(&controller, "theme.text-scale").as_deref(), Some("150%"));
        controller.set_text_scale(300);
        assert_eq!(controller.a11y().text_scale_percent(), 200);
        assert_eq!(ui.get_text_scale(), 2.0);
        assert_eq!(stored_string(&controller, "theme.text-scale").as_deref(), Some("200%"));
        controller.set_text_scale(10);
        assert_eq!(controller.a11y().text_scale_percent(), 100, "10 % clamps to 100 %");
        controller.set_text_scale(150);

        // --- Reduce motion collapses slides to a 120 ms fade ----------------
        click(ui, "Reduce motion").await;
        assert_eq!(stored_bool(&controller, "theme.reduce-motion"), Some(true));
        assert!(controller.a11y().reduce_motion);
        assert!(ui.get_reduce_motion());
        assert!(tokens.get_reduce_motion());
        assert_eq!(controller.a11y().slide_ms(), 0, "movement is removed");
        assert_eq!(controller.a11y().motion_ms(), 120);
        assert_eq!(controller.a11y().fade_ms(), 120);
        assert!(
            controller
                .log
                .borrow()
                .iter()
                .any(|line| line.contains("AETHER_SHELL_A11Y: reduce-motion=1")),
            "the change is logged for the harness: {:?}",
            controller.log.borrow()
        );

        // --- Reduce transparency forces the Minimal tier (D1 §9) ------------
        click(ui, "Reduce transparency").await;
        assert!(controller.a11y().reduce_transparency);
        assert_eq!(controller.effect_tier().as_str(), "minimal");
        assert_eq!(ui.get_tier(), aether_shell::GlassTier::Minimal);
        click(ui, "Reduce transparency").await;
        assert_eq!(controller.effect_tier().as_str(), "full");

        // --- keyboard: menu bar → dock, with a visible focus ring -----------
        controller.open(Overlay::None);
        assert_eq!(controller.focus().surface, Surface::Menubar);
        assert!(controller.handle_key("tab"));
        let focus = controller.focus();
        assert_eq!(focus.surface, Surface::Dock);
        assert_eq!(focus.index, 0);
        assert!(focus.visible);
        assert_eq!(ui.get_focus_surface(), Surface::Dock.code());
        assert!(ui.get_focus_visible());

        let dock_app = controller.state().dock_apps()[0].clone();
        let focused = ElementHandle::find_by_accessible_label(ui, &dock_app.name)
            .find(|handle| {
                handle
                    .accessible_description()
                    .map(|description| description.to_string().starts_with("Focused"))
                    .unwrap_or(false)
            })
            .expect("the focused dock app carries the focus indicator");
        assert!(
            focused
                .accessible_description()
                .unwrap()
                .contains("Focused"),
            "focus indicator is exposed to assistive tech"
        );

        // Enter launches the focused dock app through the click handler.
        assert!(controller.handle_key("enter"));
        assert_eq!(controller.state().active_app, dock_app.name);
        assert!(controller
            .state()
            .apps
            .iter()
            .any(|app| app.id == dock_app.id && app.running));

        // --- keyboard: launcher arrows and Enter ----------------------------
        controller.open(Overlay::Launcher);
        controller.search("cal");
        assert_eq!(controller.focus().surface, Surface::Launcher);
        let first = controller.focus().index;
        assert!(controller.handle_key("arrow-down"));
        assert_eq!(controller.focus().index, first + 1);
        let expected = controller.state().search_results[first + 1].title.clone();
        assert!(controller.handle_key("enter"));
        assert_eq!(controller.state().overlay, Overlay::None);
        assert_eq!(controller.state().active_app, expected);

        // --- keyboard: control centre tiles, tiers and actions --------------
        controller.open(Overlay::ControlCenter);
        assert_eq!(controller.focus().surface, Surface::ControlCenter);
        assert_eq!(controller.focus().index, 0);
        let wifi_before = controller.state().wifi;
        assert!(controller.handle_key("enter"));
        assert_eq!(controller.state().wifi, !wifi_before, "Enter toggles the Wi-Fi tile");
        assert!(controller.handle_key("arrow-right"));
        assert_eq!(controller.focus().index, 1);
        let bluetooth_before = controller.state().bluetooth;
        assert!(controller.handle_key("space"));
        assert_eq!(controller.state().bluetooth, !bluetooth_before);
        for _ in 1..7 {
            assert!(controller.handle_key("arrow-right"));
        }
        assert_eq!(controller.focus().index, 7, "the effects segment");
        let tier_before = controller.effect_settings().requested;
        assert!(controller.handle_key("enter"));
        assert_ne!(controller.effect_settings().requested, tier_before);
        assert_eq!(controller.focus().index, 7, "focus stays on the segment");
        assert!(controller.handle_key("arrow-right"));
        assert!(controller.handle_key("enter"));
        assert_eq!(controller.state().overlay, Overlay::Settings, "Settings from the keyboard");

        // --- keyboard: notifications mark read, Escape closes ---------------
        controller.open(Overlay::Notifications);
        assert_eq!(controller.focus().surface, Surface::Notifications);
        assert_eq!(controller.focus().index, 0);
        let unread_before = controller.state().unread_count();
        assert!(unread_before > 0);
        assert!(controller.handle_key("arrow-right"), "first notification card");
        assert!(controller.handle_key("enter"));
        assert_eq!(
            controller.state().unread_count(),
            unread_before - 1,
            "Enter marks the card read without deleting it"
        );
        assert!(
            controller.state().notifications.len() >= unread_before,
            "the card is still listed"
        );
        assert!(controller.handle_key("escape"));
        assert_eq!(controller.state().overlay, Overlay::None);
        assert_eq!(controller.focus().surface, Surface::Menubar);

        // Tab/Shift+Tab cover all five surfaces in order.
        let mut visited = vec![controller.focus().surface];
        for _ in 0..4 {
            assert!(controller.handle_key("tab"));
            visited.push(controller.focus().surface);
        }
        assert_eq!(
            visited,
            vec![
                Surface::Menubar,
                Surface::Dock,
                Surface::Launcher,
                Surface::ControlCenter,
                Surface::Notifications
            ],
            "every shell surface is reachable by Tab"
        );
        assert!(controller.handle_key("shift+tab"));
        assert_eq!(controller.focus().surface, Surface::ControlCenter);

        // --- the harness summary names the whole contract -------------------
        let summary = controller.a11y_summary();
        assert!(summary.contains("A11Y_REDUCE_MOTION: on"), "{summary}");
        assert!(summary.contains("A11Y_TEXT_SCALE: 150%"), "{summary}");
        assert!(summary.contains("A11Y_KEYBOARD: surface=notifications"), "{summary}");
        assert!(summary.contains("AETHER_SHELL_A11Y_OK"), "{summary}");

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
