//! Launcher integration: the shell's app model and its spawn messages come from
//! `aether-launcher-registry` (P4 launch path).
//!
//! Three surfaces are covered:
//!
//! 1. **No drift.** The launcher model (`desktop::default_apps`) is the registry
//!    list (ids, names, icons, `Exec`) plus only the ids the shell itself owns
//!    (`Settings`), so an app cannot appear in one place and be missing in the
//!    other.
//! 2. **Spawn seam.** `ShellController` hands every start to a `Spawner`; the
//!    injected recording spawner proves the argv is the registry's
//!    `<exec> --launch <id>` and that it is started exactly once. The real
//!    `ProcessSpawner` is proven with `/bin/true --launch x`, so no AetherOS
//!    binary and no GUI are needed.
//! 3. **`--launch`.** The shipped binary hands off to the registry argv (a stub
//!    app on `PATH` records it), and an unknown id exits non-zero with a clear
//!    message.

use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;

use aether_shell::config::SettingsStore;
use aether_shell::desktop;
use aether_shell::services::{Battery, Services};
use aether_shell::spawn::{self, ProcessSpawner, Source, Spawner};
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

/// Shared recording spawner: the controller owns the `Box`, the test keeps the
/// calls.
#[derive(Default, Clone)]
struct RecordingSpawner(Rc<RefCell<Vec<Vec<String>>>>);

impl RecordingSpawner {
    fn calls(&self) -> Vec<Vec<String>> {
        self.0.borrow().clone()
    }
}

impl Spawner for RecordingSpawner {
    fn spawn(&self, argv: &[String]) -> Result<(), String> {
        self.0.borrow_mut().push(argv.to_vec());
        Ok(())
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-launcher-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn the_shell_app_model_matches_the_registry() {
    let apps = desktop::default_apps();
    let registry = aether_launcher_registry::entries();

    // (a) every id the launcher model exposes either resolves through the
    // registry (with the registry argv) or is explicitly shell-owned.
    for app in &apps {
        match aether_launcher_registry::resolve(&app.id) {
            Some(entry) => {
                assert_eq!(
                    app.exec, entry.exec,
                    "{} exec drifted from the registry",
                    app.id
                );
                assert_eq!(
                    app.name, entry.name,
                    "{} name drifted from the registry",
                    app.id
                );
                let (argv, source) = spawn::launch_argv(&app.id, &app.exec);
                assert_eq!(
                    source,
                    Source::Registry,
                    "{} must use the registry argv",
                    app.id
                );
                assert_eq!(
                    argv,
                    aether_launcher_registry::launch_argv(&app.id).unwrap(),
                    "{} argv",
                    app.id
                );
            }
            None => assert!(
                desktop::SHELL_OWNED_IDS.contains(&app.id.as_str()),
                "{} is in the launcher but neither registered nor shell-owned",
                app.id
            ),
        }
    }

    // ... and the reverse: no registry app is missing from the launcher.
    for entry in registry {
        assert!(
            apps.iter().any(|app| app.id == entry.id),
            "{} is registered but missing from the launcher model",
            entry.id
        );
    }

    // The dock is the same list, so its ids resolve too.
    let dock: Vec<&str> = apps
        .iter()
        .filter(|app| app.pinned)
        .map(|app| app.id.as_str())
        .collect();
    assert_eq!(dock, desktop::PINNED_IDS.to_vec());
    for id in &dock {
        assert!(
            aether_launcher_registry::resolve(id).is_some(),
            "pinned dock app {id} is not in the registry"
        );
    }
}

#[test]
fn a_launch_starts_the_registry_argv_exactly_once() {
    let spawner = RecordingSpawner::default();
    let (argv, source, outcome) = spawn::launch("aether-files", "aether-files", &spawner);
    assert!(outcome.is_ok());
    assert_eq!(source, Source::Registry);
    assert_eq!(
        argv,
        vec!["aether-files", "--launch", "aether-files"],
        "the registry owns the argv"
    );
    assert_eq!(spawner.calls(), vec![argv], "exactly one start per launch");

    // An id the registry does not own keeps its `.desktop` Exec line.
    let spawner = RecordingSpawner::default();
    let (argv, source, outcome) = spawn::launch("org.example.App", "example --new", &spawner);
    assert!(outcome.is_ok());
    assert_eq!(source, Source::Exec);
    assert_eq!(argv, vec!["sh", "-c", "exec example --new"]);
    assert_eq!(spawner.calls().len(), 1);

    // The real process seam, with a harmless argv: no GUI, no AetherOS app.
    // `/bin/true` on the image, `/usr/bin/true` on the macOS dev host.
    let true_bin = ["/bin/true", "/usr/bin/true"]
        .into_iter()
        .find(|path| Path::new(path).exists())
        .expect("a `true` binary for the harness");
    let spawner = RecordingSpawner::default();
    let argv = vec![
        true_bin.to_string(),
        "--launch".to_string(),
        "x".to_string(),
    ];
    ProcessSpawner.spawn(&argv).expect("starts true");
    spawner.spawn(&argv).expect("starts true");
    assert_eq!(spawner.calls(), vec![argv]);
}

#[test]
fn launching_from_the_shell_uses_the_registry_argv() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let root = temp_dir("controller");
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

        // The launcher starts calculator through the seam: one start, registry
        // argv, and the host log names both.
        let spawner = RecordingSpawner::default();
        controller.set_spawner(Box::new(spawner.clone()));
        controller.launch("aether-calculator");
        assert_eq!(
            spawner.calls(),
            vec![vec![
                "aether-calculator".to_string(),
                "--launch".to_string(),
                "aether-calculator".to_string()
            ]]
        );
        assert!(
            controller
                .log
                .borrow()
                .iter()
                .any(|line| line
                    == "spawned aether-calculator source=registry argv=aether-calculator --launch aether-calculator"),
            "the log records what was started: {:?}",
            controller.log.borrow()
        );
        assert_eq!(controller.state().active_app, "Calculator");
        controller.close_app("aether-calculator");

        // A shell-owned surface never leaves the process: Settings is a shell
        // overlay, so nothing is spawned for it.
        controller.launch("aether-settings");
        assert_eq!(spawner.calls().len(), 1, "Settings must not be spawned");

        // The same message comes from the keyboard path and from `launch()`.
        assert_eq!(
            controller.launch_argv("aether-terminal"),
            Some(vec![
                "aether-terminal".to_string(),
                "--launch".to_string(),
                "aether-terminal".to_string()
            ])
        );
        assert_eq!(controller.launch_argv("nope"), None);

        let _ = std::fs::remove_dir_all(root);
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}

/// Write a stub executable that records the argv it was started with.
fn stub_app(dir: &std::path::Path, name: &str) -> PathBuf {
    let log = dir.join(format!("{name}.argv"));
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    log
}

#[test]
fn the_launch_cli_hands_off_to_the_registry_argv() {
    let dir = temp_dir("cli");
    std::fs::create_dir_all(&dir).unwrap();
    let recorded = stub_app(&dir, "aether-files");
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(["--launch", "aether-files"])
        .env("PATH", &path)
        .output()
        .expect("run aether-shell --launch");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{stdout}");
    assert!(
        stdout.contains("AETHER_SHELL_LAUNCH: id=aether-files source=registry exec=aether-files"),
        "stdout:\n{stdout}"
    );

    // The app was started with the handshake the registry defines.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut argv = String::new();
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&recorded) {
            argv = text;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        argv.lines().collect::<Vec<_>>(),
        vec!["--launch", "aether-files"],
        "the app receives its own --launch id"
    );

    // Shell-owned ids are surfaces, not processes.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(["--launch", "aether-settings"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("AETHER_SHELL_LAUNCH: id=aether-settings handled=shell"));

    // An unknown id is a clear failure, not a silent no-op.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(["--launch", "aether-nope"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown app id 'aether-nope'"),
        "stderr:\n{stderr}"
    );

    // A missing id is reported too.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .arg("--launch")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--launch needs an app id"));

    let _ = std::fs::remove_dir_all(dir);
}
