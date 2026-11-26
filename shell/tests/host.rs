//! The interactive shell host: compositor journal → dialog → action →
//! terminate marker (G-NH2 ↔ P4, S3).
//!
//! One test function per file because the Slint testing backend installs a
//! process-wide platform. The journal is injected ([`MemoryJournal`]), so the
//! loop that runs on the image is exercised here without a display server: the
//! same `pump_once` / `present` path the acceptance harness drives with
//! `aether-shell --interactive --journal <fixture> --frames <n>`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use slint::ComponentHandle;

use aether_shell::config::SettingsStore;
use aether_shell::runtime::{HostConfig, MemoryJournal, ShellHost};
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
        "aether-shell-host-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn the_host_runs_the_dialog_from_the_journal_and_publishes_the_terminate_marker() {
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

        // The harness's writing end of the journal; the host clones the same
        // shared buffer.
        let journal = MemoryJournal::new();
        let now = Instant::now();
        let mut host = ShellHost::new(controller, Box::new(journal.clone()), HostConfig::default());

        // 1. The start is counted before any surface exists.
        let outcome = host.start(now);
        assert_eq!(outcome.attempt, 1);
        assert!(!outcome.safe_mode);
        let started = host.drain_published();
        assert!(
            started
                .iter()
                .any(|line| line.starts_with("AETHER_SHELL_START: attempt=1 safe_mode=false")),
            "{started:?}"
        );

        // 2. A compositor health marker raises the dialog on the surfaces.
        journal.push("AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100\n");
        let lines = host.pump_once(now + Duration::from_millis(10)).unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line
                    == "AETHER_SHELL_NOHANG_DIALOG: client=7 app=client-7 after_ms=5100"),
            "{lines:?}"
        );
        assert!(host.controller().ui().get_nohang_open());

        // 3. The first presented frame confirms the start and the session is
        //    ready (the ledger says attempts=0).
        host.present(None).unwrap();
        let presented = host.drain_published();
        assert!(
            presented
                .iter()
                .any(|line| line == "AETHER_SHELL_START_CONFIRMED: attempts=0"),
            "{presented:?}"
        );
        assert!(
            presented
                .iter()
                .any(|line| line.starts_with("AETHER_SHELL_HOST_READY: frames=1")),
            "{presented:?}"
        );
        assert_eq!(host.controller().startup().attempts, 0);
        assert!(host.controller().startup().confirmed);

        // 4. The user presses Wait: the dialog closes, the action is published
        //    and the app is re-offered once the 15 s grace elapses.
        assert_eq!(
            host.controller()
                .dialog_action(DialogAction::Wait, now + Duration::from_millis(10)),
            Some(7)
        );
        let lines = host.pump_once(now + Duration::from_secs(16)).unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("action=wait grace_ms=15000")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("AETHER_SHELL_NOHANG_DIALOG: client=7")),
            "the re-offer is published: {lines:?}"
        );
        assert!(host.controller().ui().get_nohang_open());

        // 5. Force Quit: the host publishes the action and the terminate
        //    intent, and hands the client to the supervisor.
        let acted = now + Duration::from_secs(17);
        assert_eq!(
            host.controller()
                .dialog_action(DialogAction::ForceQuit, acted),
            Some(7)
        );
        let lines = host.pump_once(acted).unwrap();
        assert!(
            lines.iter().any(|line| line.contains("action=force_quit")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "AETHER_SHELL_NOHANG_TERMINATE: client=7"),
            "{lines:?}"
        );
        assert_eq!(host.take_terminations(), vec![7]);
        assert!(!host.controller().ui().get_nohang_open());

        // 6. A late pong closes the dialog without any user action.
        journal.push("AETHER_COMPOSITOR_APP_STATE: client=9 state=not_responding after_ms=5000\n");
        host.pump_once(acted + Duration::from_millis(1)).unwrap();
        assert!(host.controller().ui().get_nohang_open());
        journal.push("AETHER_COMPOSITOR_APP_STATE: client=9 state=responsive recovered=true\n");
        let lines = host.pump_once(acted + Duration::from_millis(2)).unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("AETHER_SHELL_NOHANG_RECOVERED: client=9")),
            "{lines:?}"
        );
        assert!(!host.controller().ui().get_nohang_open());

        // 7. The harness's scripted button press takes the same path as a
        //    click: one force-quit when the next dialog is on screen.
        journal.push("AETHER_COMPOSITOR_APP_STATE: client=11 state=not_responding after_ms=5000\n");
        host.set_auto_action(Some(DialogAction::ForceQuit));
        let lines = host.pump_once(acted + Duration::from_millis(3)).unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("client=11") && line.contains("action=force_quit")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "AETHER_SHELL_NOHANG_TERMINATE: client=11"),
            "{lines:?}"
        );
        assert_eq!(host.take_terminations(), vec![11]);

        // 8. The bounded loop (the harness form) presents frames and reports a
        //    readable summary.
        let run = host.run_for(2, None, |_lines| {}).expect("host loop runs");
        assert_eq!(run.frames, 3, "one present call plus two loop frames");
        assert!(run.ready);
        assert_eq!(run.dialogs, 4, "three raises plus one Wait re-offer");
        assert_eq!(run.actions, 3, "one wait and two force quits");
        assert!(run
            .summary_line()
            .starts_with("AETHER_SHELL_HOST_DONE: frames=3"));
        assert!(run.summary_line().ends_with("ready=true"));

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
