//! G-NH2 dialog and safe-mode banner through the real widget tree (P3-T08 ↔ P4).
//!
//! One test function per file because the Slint testing backend installs a
//! process-wide platform. The compositor's markers drive the dialog (Wait /
//! Force Quit), and three failed starts drive the safe-mode banner and the
//! Minimal tier.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::ComponentHandle;

use aether_shell::config::SettingsStore;
use aether_shell::nohang::HangEvent;
use aether_shell::services::{Battery, Services};
use aether_shell::{DialogAction, ShellController, ShellRoot};

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
    fn battery(&mut self) -> Option<Battery> {
        Some(Battery {
            percent: 84,
            charging: false,
        })
    }
    fn set_power_mode(&mut self, _mode: aether_shell::PowerMode) -> Result<(), String> {
        Ok(())
    }
    fn lock_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn temp_config_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-shell-nohang-{}-{}",
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
fn the_dialog_and_safe_mode_follow_the_compositor_markers() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_config_root();
        let ui = ShellRoot::new().unwrap();
        let controller = ShellController::new(
            ui,
            Box::new(QuietServices),
            SettingsStore::new(root.clone()),
        );
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1440, 900));
        let ui = controller.ui();

        assert!(!controller.ui().get_nohang_open());
        assert!(!controller.ui().get_safe_mode());
        assert_eq!(controller.effect_tier().as_str(), "full");

        // The compositor marks client 7 not responding: the dialog names the
        // app and offers Wait / Force Quit.
        let now = Instant::now();
        controller.register_client(7, "aether-text", "Aether Text");
        controller.observe_health(
            HangEvent::NotResponding {
                client: 7,
                after_ms: 5100,
            },
            now,
        );
        assert!(ui.get_nohang_open());
        assert_eq!(ui.get_nohang_app().to_string(), "Aether Text");
        assert!(ui
            .get_nohang_detail()
            .to_string()
            .contains("stopped answering"));

        // Wait closes the dialog, logs the action and re-offers after the grace.
        click(ui, "Wait").await;
        assert!(!ui.get_nohang_open());
        assert!(
            controller
                .log
                .borrow()
                .iter()
                .any(|line| line.contains("action=wait")),
            "{:?}",
            controller.log.borrow()
        );
        assert_eq!(
            controller.tick_dialog(Instant::now() + Duration::from_secs(16)),
            Some(7),
            "the app is still hung: the dialog comes back"
        );
        assert!(ui.get_nohang_open());

        // Force Quit closes the dialog and returns the client to terminate.
        assert_eq!(
            controller.dialog_action(DialogAction::ForceQuit, Instant::now()),
            Some(7)
        );
        assert!(!ui.get_nohang_open());
        let log = controller.log.borrow().clone();
        assert!(
            log.iter().any(|line| line.contains("action=force_quit")),
            "{log:?}"
        );
        assert!(
            log.iter()
                .any(|line| line.contains("AETHER_SHELL_NOHANG_TERMINATE: client=7")),
            "{log:?}"
        );

        // A late pong closes the dialog on its own (recovery, no user action).
        controller.observe_health(
            HangEvent::NotResponding {
                client: 9,
                after_ms: 5000,
            },
            Instant::now(),
        );
        assert!(ui.get_nohang_open());
        controller.observe_health(HangEvent::Recovered { client: 9 }, Instant::now());
        assert!(!ui.get_nohang_open());

        // Three unconfirmed starts: safe mode, the Minimal tier and the banner.
        for _ in 0..3 {
            controller.begin_session_start();
        }
        assert!(controller.safe_mode());
        assert!(ui.get_safe_mode());
        assert!(
            ui.get_safe_mode_note()
                .to_string()
                .contains("3 starts failed"),
            "{}",
            ui.get_safe_mode_note()
        );
        assert_eq!(controller.effect_tier().as_str(), "minimal");
        assert_eq!(
            controller.effect_env_line(),
            "AETHER_COMPOSITOR_EFFECT_TIER=minimal"
        );

        // The banner's Reset leaves safe mode and restores the requested tier.
        click(ui, "Reset").await;
        assert!(!controller.safe_mode());
        assert!(!ui.get_safe_mode());
        assert_eq!(controller.effect_tier().as_str(), "full");

        // A confirmed start clears the attempt count on disk.
        controller.begin_session_start();
        assert_eq!(controller.startup().attempts, 1);
        controller.session_ready();
        assert_eq!(controller.startup().attempts, 0);

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
