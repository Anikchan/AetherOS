//! Session ledger through the interactive host (G-NH2, S3).
//!
//! The host owns the `--start-begin` / `--start-confirm` handshake: every launch
//! counts before the surfaces exist, the first presented frame confirms it, and
//! three unconfirmed launches (or three compositor shell-watchdog timeouts) run
//! the reduced session with the safe-mode banner.
//!
//! One test function per file because the Slint testing backend installs a
//! process-wide platform.

use std::path::PathBuf;
use std::time::Instant;

use slint::ComponentHandle;

use aether_shell::config::SettingsStore;
use aether_shell::nohang::StartLedger;
use aether_shell::runtime::{HostConfig, MemoryJournal, ShellHost};
use aether_shell::services::{Battery, Services};
use aether_shell::{ShellController, ShellRoot};

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
        "aether-shell-ledger-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn the_host_counts_starts_confirms_the_session_and_arms_safe_mode() {
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
        let now = Instant::now();

        // Launch 1 and 2 die before the surfaces are presented.
        let mut first = ShellHost::new(
            controller.clone(),
            Box::new(MemoryJournal::new()),
            HostConfig::default(),
        );
        let outcome = first.start(now);
        assert_eq!((outcome.attempt, outcome.safe_mode), (1, false));
        let lines = first.drain_published();
        assert!(
            lines
                .iter()
                .any(|line| line == "AETHER_SHELL_START: attempt=1 safe_mode=false previous=clean"),
            "{lines:?}"
        );

        let mut second = ShellHost::new(
            controller.clone(),
            Box::new(MemoryJournal::new()),
            HostConfig::default(),
        );
        let outcome = second.start(now);
        assert_eq!((outcome.attempt, outcome.previous_failed), (2, true));

        // Launch 3 is the third unconfirmed start: safe mode, minimal glass.
        let mut third = ShellHost::new(
            controller.clone(),
            Box::new(MemoryJournal::new()),
            HostConfig::default(),
        );
        let outcome = third.start(now);
        assert_eq!((outcome.attempt, outcome.safe_mode), (3, true));
        let lines = third.drain_published();
        assert!(
            lines.iter().any(|line| line
                == "AETHER_SHELL_START: attempt=3 safe_mode=true previous=unconfirmed"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line
                == "AETHER_SHELL_SAFE_MODE: attempts=3 timeouts=0 reason=3 starts failed in a row effects=minimal"),
            "{lines:?}"
        );
        assert!(controller.safe_mode());
        assert!(controller.ui().get_safe_mode());
        assert_eq!(controller.effect_tier().as_str(), "minimal");

        // The ledger on disk is the shared state the service manager reads.
        let path = StartLedger::path(controller.store());
        let on_disk = StartLedger::load(&path);
        assert_eq!(on_disk.attempts, 3);
        assert!(!on_disk.confirmed);
        assert!(on_disk.safe_mode);

        // Presenting the surfaces confirms the start; the banner stays until an
        // explicit reset.
        third.present(None).unwrap();
        let lines = third.drain_published();
        assert!(
            lines
                .iter()
                .any(|line| line == "AETHER_SHELL_START_CONFIRMED: attempts=0"),
            "{lines:?}"
        );
        assert_eq!(controller.startup().attempts, 0);
        assert!(controller.startup().confirmed);
        assert!(controller.safe_mode(), "a good start keeps safe mode on");

        // The banner's reset leaves safe mode and restores the requested tier.
        controller.reset_safe_mode();
        let lines = third.pump_once(now).unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line == "AETHER_SHELL_SAFE_MODE_RESET: attempts=0"),
            "{lines:?}"
        );
        assert!(!controller.ui().get_safe_mode());
        assert_eq!(controller.effect_tier().as_str(), "full");

        // A previous compositor run that stalled three times arms safe mode at
        // the next launch, before the start is counted.
        let journal = MemoryJournal::new();
        journal.push(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6000 timeout_ms=5000 timeouts=3\n",
        );
        let mut fourth = ShellHost::new(
            controller.clone(),
            Box::new(journal.clone()),
            HostConfig::default(),
        );
        let watchdog_lines = fourth.pump_once(now).unwrap();
        assert!(
            watchdog_lines.iter().any(|line| line
                == "AETHER_SHELL_HOST_WATCHDOG: timeouts=3 restart_required=true armed_safe_mode=true"),
            "{watchdog_lines:?}"
        );
        let outcome = fourth.start(now);
        assert!(outcome.safe_mode);
        let start_lines = fourth.drain_published();
        assert!(
            start_lines.iter().any(|line| line
                == "AETHER_SHELL_START: attempt=1 safe_mode=true previous=clean"),
            "{start_lines:?}"
        );
        assert!(
            start_lines.iter().any(|line| line
                == "AETHER_SHELL_SAFE_MODE: attempts=1 timeouts=3 reason=the shell stalled 3 times effects=minimal"),
            "{start_lines:?}"
        );
        assert_eq!(controller.startup().watchdog_timeouts, 3);

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
