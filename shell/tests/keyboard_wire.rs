//! Keyboard path end to end: `wl_keyboard` keycode → XKB keysym →
//! `ShellController::handle_key` → focus move and activation (F1 SHL-18).
//!
//! The compositor's DRM backend turns libinput events into `wl_keyboard.key`
//! events (`evdev + 8`) plus `wl_keyboard.modifiers`. This test feeds exactly
//! those values through [`aether_shell::keys::dispatch`], the function the
//! layer host calls from its `wl_keyboard` handler, and asserts the shell moves
//! focus between surfaces, activates through the same handlers the pointer
//! uses, and leaves keys it does not own to the focused app.
//!
//! No pointer event is used anywhere in this file: the only input is a keycode
//! and a modifier mask, so a regression that only the mouse can work around
//! fails here.

use std::path::PathBuf;

use slint::ComponentHandle;

use aether_shell::config::SettingsStore;
use aether_shell::kbdnav::Surface;
use aether_shell::keys::{self, KeyEvent, MOD_MOD4, MOD_SHIFT};
use aether_shell::model::Overlay;
use aether_shell::services::{Battery, Services};
use aether_shell::{PowerMode, ShellController, ShellRoot};

#[derive(Default)]
struct QuietServices;

impl Services for QuietServices {
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
        "aether-shell-keyboard-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Press one key the way the compositor delivers it: the evdev keycode on the
/// wire (no `+8`) plus the XKB modifier mask.
fn press(controller: &ShellController, evdev: u32, modifiers: u32) -> KeyEvent {
    keys::dispatch(
        controller,
        keys::KeyInput::from_wire(evdev, modifiers),
        None,
    )
}

#[test]
fn keys_reach_the_shell_through_the_wire_path() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_config_root();
        let ui = ShellRoot::new().unwrap();
        let controller = ShellController::with_demo_content(
            ui,
            Box::new(QuietServices),
            SettingsStore::new(root.clone()),
        );
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1440, 900));

        // Nothing typed yet: no focus ring, focus parked on the menu bar.
        assert!(!controller.focus().visible);
        assert_eq!(controller.focus().surface, Surface::Menubar);

        // --- Tab: KEY_TAB (evdev 15) moves focus to the dock ---------------
        let event = press(&controller, 15, 0);
        assert_eq!(event.keysym, Some(keys::XKB_TAB));
        assert_eq!(event.binding.as_deref(), Some("tab"));
        assert!(event.handled, "Tab is a shell key");
        assert_eq!(controller.focus().surface, Surface::Dock);
        assert!(
            controller.focus().visible,
            "the focus ring is drawn after the first key"
        );
        assert_eq!(
            event.ring,
            Surface::Dock,
            "the marker reports the new focus ring"
        );
        assert_eq!(
            event.surface, None,
            "no layer surface in this in-process path"
        );
        assert!(event.line().starts_with(
            "AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true focus=none ring=dock"
        ));

        // --- Right: KEY_RIGHT (evdev 106) moves inside the dock -------------
        let items = controller.keyboard_counts()[1].1;
        assert!(items >= 2, "the demo dock has more than one app");
        let index = controller.focus().index;
        let event = press(&controller, 106, 0);
        assert!(event.handled);
        assert_eq!(controller.focus().index, (index + 1) % items);

        // --- Enter: KEY_ENTER (evdev 28) activates the focused dock app -----
        let focused_app = {
            let state = controller.state();
            state.dock_apps()[controller.focus().index].clone()
        };
        let before = controller.state().active_app.clone();
        let event = press(&controller, 28, 0);
        assert_eq!(event.binding.as_deref(), Some("enter"));
        assert!(event.handled);
        let state = controller.state();
        assert_eq!(
            state.active_app, focused_app.name,
            "Enter launches the focused app"
        );
        assert_ne!(state.active_app, before);
        assert!(
            state
                .apps
                .iter()
                .any(|app| app.id == focused_app.id && app.running),
            "activation goes through the same handler the dock click uses"
        );
        drop(state);

        // --- Shift+Tab (evdev 15 + Shift) goes back to the menu bar ---------
        let event = press(&controller, 15, MOD_SHIFT);
        assert_eq!(event.keysym, Some(keys::XKB_ISO_LEFT_TAB));
        assert_eq!(event.binding.as_deref(), Some("shift+tab"));
        assert_eq!(controller.focus().surface, Surface::Menubar);

        // --- Super+Space (evdev 57 + Mod4) opens the launcher ---------------
        let event = press(&controller, 57, MOD_MOD4);
        assert_eq!(event.binding.as_deref(), Some("super+space"));
        assert!(event.handled);
        assert_eq!(controller.state().overlay, Overlay::Launcher);
        assert_eq!(
            controller.focus().surface,
            Surface::Launcher,
            "opening the launcher puts the focus ring on it"
        );
        assert_eq!(controller.focus().index, 0);

        // --- Down/Up inside the launcher drive its selection ----------------
        let event = press(&controller, 108, 0);
        assert!(event.handled);
        let selected = controller.state().search_selected;
        assert_eq!(controller.focus().surface, Surface::Launcher);
        assert_eq!(controller.focus().index, selected);
        let event = press(&controller, 103, 0);
        assert!(event.handled);
        assert_eq!(controller.focus().index, controller.state().search_selected);

        // --- Escape (evdev 1) closes the overlay ---------------------------
        let event = press(&controller, 1, 0);
        assert_eq!(event.keysym, Some(keys::XKB_ESCAPE));
        assert!(event.handled);
        assert_eq!(controller.state().overlay, Overlay::None);

        // --- A key the shell does not own is left to the app ----------------
        let focus_before = controller.focus();
        let event = press(&controller, 113, 0); // KEY_MUTE
        assert_eq!(event.keysym, None);
        assert_eq!(event.binding, None);
        assert!(
            !event.handled,
            "the compositor passes unowned keys to the focused window"
        );
        assert_eq!(controller.focus(), focus_before);
        assert!(event.line().contains("handled=false"));

        // --- Shift+A is a modifier, not a shell action ---------------------
        let event = press(&controller, 30, MOD_SHIFT);
        assert_eq!(
            event.keysym,
            Some(0x0041),
            "Shift+A produces the 'A' keysym"
        );
        assert_eq!(event.binding.as_deref(), Some("a"));
        assert!(!event.handled, "plain typing never reaches the shell");

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
