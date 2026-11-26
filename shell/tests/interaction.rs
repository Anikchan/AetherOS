//! Headless interaction tests for the shell surfaces (P4, S3).
//!
//! Uses the Slint testing backend: every check drives the real widget tree
//! (clicks, text input) through `ShellController`, then asserts both the model
//! and the persisted configuration. One test function per file because the
//! testing backend installs a process-wide platform.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::{ComponentHandle, Model};
use slint::platform::PointerEventButton;

use aether_shell::auth::LocalCredentials;
use aether_shell::config::{Category, SettingsStore};
use aether_shell::model::Overlay;
use aether_shell::services::{Battery, Services};
use aether_shell::{PowerMode, ShellController, ShellRoot};

/// Mock services that can be inspected after the controller consumed the box.
#[derive(Default)]
struct SharedServices {
    calls: Rc<RefCell<Vec<String>>>,
}

impl Services for SharedServices {
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
        self.calls.borrow_mut().push("lock".into());
        Ok(())
    }
}

fn temp_config_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-shell-test-{tag}-{}-{}",
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

#[test]
fn shell_surfaces_are_interactive() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_config_root("interaction");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let ui = ShellRoot::new().unwrap();
        let services = SharedServices {
            calls: calls.clone(),
        };
        let controller = ShellController::with_demo_content(
            ui,
            Box::new(services),
            SettingsStore::new(root.clone()),
        );
        // Pin the lock-screen backend so the test never touches real PAM.
        controller.set_authenticator(Box::new(LocalCredentials::default()));
        controller.ui().show().unwrap();
        controller.ui().window().set_size(slint::PhysicalSize::new(1440, 900));

        // --- menu bar -----------------------------------------------------
        let ui = controller.ui();
        assert_eq!(ui.get_active_app().to_string(), "AetherOS");
        assert!(ui.get_clock().to_string().contains(':'));
        assert_eq!(ui.get_wifi(), true);
        click(ui, "Wi-Fi on").await;
        assert_eq!(controller.state().wifi, false, "tray Wi-Fi click toggles state");
        assert!(calls.borrow().contains(&"wifi false".to_string()));
        assert_eq!(
            controller
                .store()
                .load("network.wifi")
                .unwrap()
                .and_then(|value| value.as_bool()),
            Some(false),
            "the toggle is persisted to network.toml"
        );

        // --- dock ---------------------------------------------------------
        click(ui, "AetherPad").await;
        {
            let state = controller.state();
            assert_eq!(state.active_app, "AetherPad");
            assert!(
                state.apps.iter().any(|app| app.id == "aether-text" && app.running),
                "launching from the dock marks the app running"
            );
        }
        assert_eq!(ui.get_active_app().to_string(), "AetherPad");

        // --- launcher -----------------------------------------------------
        click(ui, "Notifications").await; // opens the notification centre
        assert_eq!(controller.state().overlay, Overlay::Notifications);
        controller.open(Overlay::Notifications); // same trigger closes it
        assert_eq!(controller.state().overlay, Overlay::None);

        controller.open(Overlay::Launcher);
        assert_eq!(controller.state().overlay, Overlay::Launcher);
        ui.window().dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::SharedString::from("c"),
        });
        controller.search("cal");
        let titles = controller.result_titles();
        assert!(
            titles.iter().any(|title| title == "Calculator"),
            "partial name finds apps: {titles:?}"
        );
        click(ui, "Calculator").await;
        {
            let state = controller.state();
            assert_eq!(state.active_app, "Calculator");
            assert_eq!(state.overlay, Overlay::None, "launching closes the launcher");
        }

        // --- control centre ----------------------------------------------
        controller.open(Overlay::ControlCenter);
        assert_eq!(ui.get_overlay(), Overlay::ControlCenter.code());
        click(ui, "Bluetooth").await;
        assert_eq!(
            controller.state().bluetooth, false,
            "quick tile toggles Bluetooth"
        );
        click(ui, "Performance").await;
        assert_eq!(controller.state().power_mode, PowerMode::Performance);
        assert!(calls.borrow().contains(&"power performance".to_string()));

        // --- settings (F4 file mapping) -----------------------------------
        controller.settings_category(Category::Sound);
        assert_eq!(controller.state().overlay, Overlay::Settings);
        let rows = ui.get_settings_rows();
        assert!(rows.row_count() > 0);
        assert!(rows
            .iter()
            .any(|row| row.file.to_string() == "sound.toml" && row.key.to_string() == "sound.alert-sound"));
        click(ui, "Alert sounds").await;
        assert_eq!(
            controller
                .store()
                .load("sound.alert-sound")
                .unwrap()
                .and_then(|value| value.as_bool()),
            Some(false),
            "toggling a row writes the TOML key"
        );
        // Sidebar categories switch the page and its rows (the click used to be
        // lost because the parent owns the `page` binding).
        click(ui, "Display").await;
        assert_eq!(ui.get_settings_page(), 1);
        assert!(ui
            .get_settings_rows()
            .iter()
            .any(|row| row.file.to_string() == "display.toml"));
        click(ui, "Appearance").await;
        assert_eq!(ui.get_settings_page(), 0);

        // Select rows cycle their values and persist them.
        controller.settings_category(Category::Appearance);
        click(ui, "Effects tier").await;
        assert_eq!(
            controller.store().load("theme.effects-tier").unwrap(),
            Some(toml::Value::String("reduced".into())),
            "select rows cycle dark -> reduced through the schema"
        );

        // --- notifications + Focus ---------------------------------------
        controller.open(Overlay::Notifications);
        let before = controller.state().notifications.len();
        assert!(before >= 1);
        let first_title = controller.state().notifications[0].title.clone();
        click(ui, &format!("Dismiss {first_title}")).await;
        assert_eq!(controller.state().notifications.len(), before - 1);
        controller.open(Overlay::Notifications); // close
        controller.set_toggle("focus", true);
        assert_eq!(
            controller
                .store()
                .load("notifications.focus-enabled")
                .unwrap()
                .and_then(|value| value.as_bool()),
            Some(true),
            "Focus is persisted to notifications.toml"
        );
        assert_eq!(ui.get_unread().to_string(), "");
        assert_eq!(ui.get_notifications().row_count(), 0);
        assert!(controller.state().notifications.len() >= 1, "Focus hides, never deletes");
        controller.set_toggle("focus", false);
        assert_eq!(
            ui.get_notifications().row_count(),
            controller.state().notifications.len()
        );

        // --- lock screen --------------------------------------------------
        controller.open(Overlay::Lock);
        assert_eq!(controller.state().overlay, Overlay::Lock);
        assert!(calls.borrow().contains(&"lock".to_string()), "locking asks logind");
        let password = ElementHandle::find_by_accessible_label(ui, "Password")
            .next()
            .expect("password field is in the tree");
        // The lock screen is modal: scrim/Escape/shortcuts must not dismiss it.
        controller.open(Overlay::None);
        assert_eq!(controller.state().overlay, Overlay::Lock, "scrim cannot dismiss the lock");
        controller.handle_shortcut("escape");
        assert_eq!(controller.state().overlay, Overlay::Lock, "Escape cannot dismiss the lock");
        controller.handle_shortcut("super+space");
        assert_eq!(controller.state().overlay, Overlay::Lock, "launcher cannot open over the lock");

        password.set_accessible_value("wrong");
        click(ui, "Unlock").await;
        assert!(ui.get_lock_error().to_string().contains("Incorrect"));
        assert_eq!(controller.state().overlay, Overlay::Lock);
        password.set_accessible_value("aether");
        click(ui, "Unlock").await;
        assert_eq!(controller.state().overlay, Overlay::None, "correct password unlocks");
        assert!(ui.get_lock_error().to_string().is_empty());

        // --- overview -----------------------------------------------------
        controller.open(Overlay::Overview);
        assert_eq!(controller.state().overlay, Overlay::Overview);
        let window_title = controller.state().windows[0].title.clone();
        click(ui, &window_title).await;
        assert_eq!(controller.state().overlay, Overlay::None);
        assert_eq!(controller.state().active_app, "AetherPad");

        // --- keyboard shortcuts -------------------------------------------
        controller.handle_shortcut("escape");
        assert_eq!(controller.state().overlay, Overlay::None);
        assert!(controller.handle_shortcut("super+space"), "launcher shortcut handled");
        assert_eq!(controller.state().overlay, Overlay::Launcher);
        controller.search("ca");
        controller.handle_shortcut("arrow-down");
        assert_eq!(controller.state().search_selected, 1);
        controller.handle_shortcut("arrow-up");
        assert_eq!(controller.state().search_selected, 0);
        controller.handle_shortcut("enter");
        assert_eq!(controller.state().overlay, Overlay::None, "Enter opens the selection");
        assert!(controller.handle_shortcut("super+f"));
        assert!(controller.state().focus_active(), "Super+F toggles Focus");
        assert!(controller.handle_shortcut("super+f"));
        assert!(!controller.state().focus_active());
        assert!(!controller.handle_shortcut("ctrl+alt+delete"));
        assert!(controller.handle_shortcut("super+l"));
        assert_eq!(controller.state().overlay, Overlay::Lock);
        controller.handle_shortcut("escape");

        // Closing the app clears the dock's running indicator (P4-T02 close path).
        assert!(
            controller
                .state()
                .apps
                .iter()
                .any(|app| app.id == "aether-text" && app.running),
            "AetherPad is running before the close"
        );
        controller.close_app("aether-text");
        assert!(
            controller
                .state()
                .apps
                .iter()
                .any(|app| app.id == "aether-text" && !app.running),
            "closing clears app.running"
        );
        assert!(
            !controller
                .state()
                .windows
                .iter()
                .any(|window| window.app == "aether-text"),
            "closing removes its windows"
        );
        assert_ne!(ui.get_active_app().to_string(), "AetherPad");
        assert_eq!(
            ui.get_apps()
                .iter()
                .filter(|app| app.name == "AetherPad")
                .count(),
            1,
            "the dock still lists the app, just not as running"
        );

        // The settings schema is complete: every category renders rows.
        for category in Category::ALL {
            let rows = SettingsStore::new(root.clone()).rows_for(category);
            assert!(
                !rows.is_empty(),
                "{} must expose settings",
                category.name()
            );
        }

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
