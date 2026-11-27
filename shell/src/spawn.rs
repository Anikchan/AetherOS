//! The process seam for the launcher (P4 launch path).
//!
//! `aether-launcher-registry` owns the apps-side contract: which id maps to which
//! executable and the argv the shell must start ([`registry::launch_argv`]).
//! [`crate::app::ShellController`] decides *when* to start an app; this module
//! decides *how*, behind the [`Spawner`] seam, so a test or a host can observe
//! or replace the process start without a GUI and without the app binaries
//! being installed.
//!
//! Registry ids always get the registry argv (`<exec> --launch <id>`), which is
//! what every app's `handle_launch_arg` handshake expects. Anything the registry
//! does not own (an installed `.desktop` entry, a shell-owned surface) keeps its
//! `Exec` line, run through `sh -c "exec …"` so quoted arguments still work.

use aether_launcher_registry as registry;

/// Where a launcher argv came from, for the host log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `aether_launcher_registry::launch_argv`: `<exec> --launch <id>`.
    Registry,
    /// The app's own `Exec` line, for ids the registry does not own.
    Exec,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Registry => "registry",
            Source::Exec => "exec",
        }
    }
}

/// The argv the shell starts for `id`, whose launcher record has `exec`.
pub fn launch_argv(id: &str, exec: &str) -> (Vec<String>, Source) {
    match registry::launch_argv(id) {
        Some(argv) => (argv, Source::Registry),
        None => (
            vec!["sh".to_string(), "-c".to_string(), format!("exec {exec}")],
            Source::Exec,
        ),
    }
}

/// Starts one launcher argv. The default [`ProcessSpawner`] really spawns;
/// tests install a recording spawner through
/// `ShellController::set_spawner`.
pub trait Spawner {
    /// Start the argv. Returns once the child exists (`spawn`, never `wait`);
    /// the message is what the shell log shows when the start failed.
    fn spawn(&self, argv: &[String]) -> Result<(), String>;
}

/// Runs the argv as a child process of the shell.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessSpawner;

impl Spawner for ProcessSpawner {
    fn spawn(&self, argv: &[String]) -> Result<(), String> {
        let (program, rest) = argv.split_first().ok_or("empty launch argv")?;
        std::process::Command::new(program)
            .args(rest)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("cannot start {program}: {error}"))
    }
}

/// Spawn one launcher id through `spawner`; returns the argv used, where it came
/// from, and the spawn result.
pub fn launch(
    id: &str,
    exec: &str,
    spawner: &dyn Spawner,
) -> (Vec<String>, Source, Result<(), String>) {
    let (argv, source) = launch_argv(id, exec);
    let outcome = spawner.spawn(&argv);
    (argv, source, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_use_the_launch_handshake() {
        let (argv, source) = launch_argv("aether-files", "aether-files");
        assert_eq!(
            argv,
            vec![
                "aether-files".to_string(),
                "--launch".to_string(),
                "aether-files".to_string()
            ]
        );
        assert_eq!(source, Source::Registry);
        assert_eq!(
            launch_argv("aether-taskmanager", "aether-taskmanager").1,
            Source::Registry
        );
    }

    #[test]
    fn unowned_ids_keep_their_exec_line() {
        let (argv, source) = launch_argv("org.gnome.Nautilus", "nautilus --new-window");
        assert_eq!(argv, vec!["sh", "-c", "exec nautilus --new-window"]);
        assert_eq!(source, Source::Exec);
        assert_eq!(source.as_str(), "exec");
        assert_eq!(Source::Registry.as_str(), "registry");
    }

    #[test]
    fn a_recording_spawner_sees_the_argv_once() {
        use std::cell::RefCell;

        #[derive(Default)]
        struct Recording(RefCell<Vec<Vec<String>>>);
        impl Spawner for Recording {
            fn spawn(&self, argv: &[String]) -> Result<(), String> {
                self.0.borrow_mut().push(argv.to_vec());
                Ok(())
            }
        }

        let spawner = Recording::default();
        let (argv, source, outcome) = launch("aether-text", "aether-text", &spawner);
        assert!(outcome.is_ok());
        assert_eq!(source, Source::Registry);
        assert_eq!(
            argv,
            vec!["aether-text", "--launch", "aether-text"],
            "the argv handed back is the one that was started"
        );
        let calls = spawner.0.borrow();
        assert_eq!(calls.len(), 1, "exactly one start per launch");
        assert_eq!(calls[0], argv);
    }

    #[test]
    fn the_process_spawner_runs_a_harmless_argv() {
        // No GUI and no AetherOS app binary: `true` proves the real process
        // seam is wired and that arguments are passed verbatim.
        let true_bin = ["/bin/true", "/usr/bin/true"]
            .into_iter()
            .find(|path| std::path::Path::new(path).exists())
            .expect("a `true` binary for the harness");
        let spawner = ProcessSpawner;
        spawner
            .spawn(&[true_bin.into(), "--launch".into(), "x".into()])
            .expect("the process seam starts true");
        // A missing program is an error (logged by the shell), not a panic.
        let error = spawner
            .spawn(&["aether-shell-does-not-exist".into()])
            .expect_err("missing programs report an error");
        assert!(error.contains("cannot start aether-shell-does-not-exist"));
        let empty = spawner.spawn(&[]).expect_err("an empty argv cannot start");
        assert_eq!(empty, "empty launch argv");
    }
}
