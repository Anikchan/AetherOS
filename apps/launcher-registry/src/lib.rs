//! Desktop-entry registry for the AetherOS default apps (P4 launch path).
//!
//! The shell builds a launcher model from `.desktop` files, but nothing on the
//! apps side said which binary answers an id or how to start it. This crate is
//! that seam:
//!
//! * [`entries`] lists every default app as a desktop-entry-like record
//!   (`id`, `name`, `exec`, `icon`, `categories`),
//! * [`resolve`] maps a launcher id to its record,
//! * [`launch_argv`] returns the argv the shell must spawn:
//!   `<exec> --launch <id>`,
//! * [`desktop_file`] renders the record as a `.desktop` file for packaging,
//! * [`launch_decision`] is the pure rule: `--launch <own id>` continues into
//!   the app, `--launch-check <own id>` stops after the marker, and a wrong or
//!   unknown id exits 2,
//! * [`handle_launch_arg`] applies that rule at the top of `main`, prints the
//!   `AETHER_LAUNCH <id>` marker and removes the launch flags from the
//!   arguments so the app continues into its normal UI path.
//!
//! `--launch <id>` is a real launch: a dock click starts the binary, the marker
//! goes to the shell log, and the process keeps running into the same code path
//! it uses without flags (the Slint window once the P3-T02 host is attached,
//! `--render` for headless evidence). `--launch-check <id>` is the dry variant
//! used by tests and packaging checks.

/// One default app, shaped like a freedesktop `.desktop` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Launcher id, for example `aether-files`.
    pub id: &'static str,
    /// Display name, for example `Files`.
    pub name: &'static str,
    /// Executable name placed on `PATH` by the package/image.
    pub exec: &'static str,
    /// Icon name; the shell maps it with its `icon_for` heuristic.
    pub icon: &'static str,
    /// Directory under `apps/` that owns the crate.
    pub crate_dir: &'static str,
    /// Freedesktop categories, joined with `;` in [`desktop_file`].
    pub categories: &'static [&'static str],
    /// True when the app is a terminal emulator (`Terminal=true`).
    pub terminal: bool,
}

/// Every default app the launcher and dock offer on a fresh install.
pub const ENTRIES: &[Entry] = &[
    Entry {
        id: "aether-files",
        name: "Files",
        exec: "aether-files",
        icon: "folder",
        crate_dir: "files",
        categories: &["System", "FileManager", "Utility"],
        terminal: false,
    },
    Entry {
        id: "aether-terminal",
        name: "Terminal",
        exec: "aether-terminal",
        icon: "terminal",
        crate_dir: "terminal",
        categories: &["System", "TerminalEmulator", "Utility"],
        terminal: true,
    },
    Entry {
        id: "aether-browser",
        name: "Browser",
        exec: "aether-browser",
        icon: "web-browser",
        crate_dir: "browser",
        categories: &["Network", "WebBrowser"],
        terminal: false,
    },
    Entry {
        id: "aether-text",
        name: "AetherPad",
        exec: "aether-text",
        icon: "text-editor",
        crate_dir: "text",
        categories: &["Utility", "TextEditor"],
        terminal: false,
    },
    Entry {
        id: "aether-calculator",
        name: "Calculator",
        exec: "aether-calculator",
        icon: "calculator",
        crate_dir: "calculator",
        categories: &["Utility", "Calculator"],
        terminal: false,
    },
    Entry {
        id: "aether-images",
        name: "Images",
        exec: "aether-images",
        icon: "image-viewer",
        crate_dir: "images",
        categories: &["Graphics", "Viewer"],
        terminal: false,
    },
    Entry {
        id: "aether-documents",
        name: "Documents",
        exec: "aether-documents",
        icon: "document-viewer",
        crate_dir: "documents",
        categories: &["Office", "Viewer"],
        terminal: false,
    },
    Entry {
        id: "aether-player",
        name: "Player",
        exec: "aether-player",
        icon: "media-player",
        crate_dir: "player",
        categories: &["AudioVideo", "Player"],
        terminal: false,
    },
    Entry {
        id: "aether-calendar",
        name: "Calendar",
        exec: "aether-calendar",
        icon: "calendar",
        crate_dir: "calendar",
        categories: &["Utility", "Calendar"],
        terminal: false,
    },
    Entry {
        id: "aether-archive",
        name: "Archive",
        exec: "aether-archive",
        icon: "archive-manager",
        crate_dir: "archive",
        categories: &["Utility", "Archiving"],
        terminal: false,
    },
    Entry {
        id: "aether-taskmanager",
        name: "Activity Manager",
        exec: "aether-taskmanager",
        icon: "system-monitor",
        crate_dir: "taskmanager",
        categories: &["System", "Monitor"],
        terminal: false,
    },
];

/// The registry, in launcher order.
pub fn entries() -> &'static [Entry] {
    ENTRIES
}

/// Resolve a launcher id, for example `aether-files`.
pub fn resolve(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.id == id)
}

/// Resolve by executable name, for the apps' `--launch` handshake.
pub fn resolve_exec(exec: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.exec == exec)
}

/// Failure modes of a launch request, with messages the apps print verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// `--launch` was not given an id.
    MissingId,
    /// The id is not in the registry.
    UnknownId(String),
    /// The id belongs to a different executable.
    WrongExec {
        id: String,
        expected: String,
        got: String,
    },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::MissingId => write!(f, "--launch needs an app id"),
            LaunchError::UnknownId(id) => write!(f, "unknown app id '{id}'"),
            LaunchError::WrongExec { id, expected, got } => {
                write!(f, "'{got}' cannot start '{id}'; use '{expected}'")
            }
        }
    }
}

impl std::error::Error for LaunchError {}

/// The argv the shell must spawn for a launcher id: `<exec> --launch <id>`.
/// `None` when the id is not in the registry.
pub fn launch_argv(id: &str) -> Option<Vec<String>> {
    resolve(id).map(|entry| {
        vec![
            entry.exec.to_string(),
            "--launch".to_string(),
            entry.id.to_string(),
        ]
    })
}

/// The marker a launch prints (`AETHER_LAUNCH <id>`), used by the shell log
/// and the acceptance checks.
pub fn launch_marker(id: &str) -> String {
    format!("AETHER_LAUNCH {id}")
}

/// Validate a `--launch <id>` pair for the app whose executable is `exec`.
pub fn launch_outcome(id: &str, exec: &str) -> Result<String, LaunchError> {
    let entry = resolve(id).ok_or_else(|| LaunchError::UnknownId(id.to_string()))?;
    if entry.exec != exec {
        return Err(LaunchError::WrongExec {
            id: id.to_string(),
            expected: entry.exec.to_string(),
            got: exec.to_string(),
        });
    }
    Ok(launch_marker(entry.id))
}

/// What a launch flag means, decided without printing or exiting.
///
/// * `--launch <own id>` → [`LaunchDecision::Continue`] with the marker: the
///   process keeps running and enters the app's normal UI path.
/// * `--launch-check <own id>` → [`LaunchDecision::Exit`] with code 0: the dry
///   handshake used by tests and packaging checks.
/// * unknown, foreign or missing id → [`LaunchDecision::Exit`] with code 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchDecision {
    /// Keep running the app. `marker` is `Some` for a real `--launch <id>`,
    /// and the app prints it before continuing.
    Continue { marker: Option<String> },
    /// Stop with `code`. `marker` goes to stdout, `message` to stderr.
    Exit {
        code: i32,
        marker: Option<String>,
        message: Option<String>,
    },
}

impl LaunchDecision {
    /// The marker to print, if any.
    pub fn marker(&self) -> Option<&str> {
        match self {
            LaunchDecision::Continue { marker } | LaunchDecision::Exit { marker, .. } => {
                marker.as_deref()
            }
        }
    }

    /// The exit code, `None` when the app keeps running.
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            LaunchDecision::Continue { .. } => None,
            LaunchDecision::Exit { code, .. } => Some(*code),
        }
    }
}

/// The arguments the launch flags left for the app, plus the exit request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchStartup {
    /// `Some(code)` when the process must stop (dry check or rejection).
    pub exit: Option<i32>,
    /// The app's own arguments with `--launch`/`--launch-check` removed.
    pub rest: Vec<String>,
}

/// The error message for a rejected launch. It names the registry as the place
/// where launcher ids are defined, so a wrong `Exec` line is easy to fix.
fn reject_message(exec: &str, error: &LaunchError) -> String {
    format!("{exec}: {error} (launcher ids live in apps/launcher-registry/src/lib.rs)")
}

/// Decide what the first two arguments mean for `exec`. Pure: no printing, no
/// process exit, so every app can test its own id without a display.
pub fn launch_decision(exec: &str, args: &[String]) -> LaunchDecision {
    let Some(flag) = args.first().map(String::as_str) else {
        return LaunchDecision::Continue { marker: None };
    };
    if flag != "--launch" && flag != "--launch-check" {
        return LaunchDecision::Continue { marker: None };
    }
    let dry_run = flag == "--launch-check";
    let Some(id) = args.get(1) else {
        return LaunchDecision::Exit {
            code: 2,
            marker: None,
            message: Some(reject_message(exec, &LaunchError::MissingId)),
        };
    };
    match launch_outcome(id, exec) {
        Ok(marker) if dry_run => LaunchDecision::Exit {
            code: 0,
            marker: Some(marker),
            message: None,
        },
        Ok(marker) => LaunchDecision::Continue {
            marker: Some(marker),
        },
        Err(error) => LaunchDecision::Exit {
            code: 2,
            marker: None,
            message: Some(reject_message(exec, &error)),
        },
    }
}

/// Apply a launch flag before the app's own argument parsing.
///
/// Returns the app's remaining arguments and, only when the process must stop
/// (`--launch-check`, a wrong id), the exit code. A real `--launch <id>` prints
/// the marker and leaves `exit` as `None`, so the app continues into the same
/// path it uses without a launch flag:
///
/// ```ignore
/// fn main() {
///     let startup = aether_launcher_registry::handle_launch_arg(
///         "aether-files",
///         std::env::args().skip(1),
///     );
///     if let Some(code) = startup.exit {
///         std::process::exit(code);
///     }
///     let mut args = startup.rest.into_iter();
///     // ...
/// }
/// ```
pub fn handle_launch_arg(exec: &str, args: impl IntoIterator<Item = String>) -> LaunchStartup {
    let args: Vec<String> = args.into_iter().collect();
    let decision = launch_decision(exec, &args);
    let has_launch_flag = matches!(
        args.first().map(String::as_str),
        Some("--launch" | "--launch-check")
    );
    let rest = if has_launch_flag {
        args.into_iter().skip(2).collect()
    } else {
        args
    };
    match decision {
        LaunchDecision::Continue { marker } => {
            if let Some(marker) = marker {
                println!("{marker}");
            }
            LaunchStartup { exit: None, rest }
        }
        LaunchDecision::Exit {
            code,
            marker,
            message,
        } => {
            if let Some(marker) = marker {
                println!("{marker}");
            }
            if let Some(message) = message {
                eprintln!("{message}");
            }
            LaunchStartup {
                exit: Some(code),
                rest,
            }
        }
    }
}

/// Render an entry as a freedesktop `.desktop` file (packaging and tests).
pub fn desktop_file(entry: &Entry) -> String {
    let mut text = String::new();
    text.push_str("[Desktop Entry]\n");
    text.push_str("Type=Application\n");
    text.push_str(&format!("Name={}\n", entry.name));
    text.push_str(&format!("Exec={}\n", entry.exec));
    text.push_str(&format!("Icon={}\n", entry.icon));
    text.push_str(&format!(
        "Terminal={}\n",
        if entry.terminal { "true" } else { "false" }
    ));
    text.push_str(&format!("Categories={};\n", entry.categories.join(";")));
    text.push_str(&format!("X-Aether-Id={}\n", entry.id));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("apps/launcher-registry sits two levels below the workspace root")
            .to_path_buf()
    }

    #[test]
    fn registry_covers_every_default_app_once() {
        let mut ids: Vec<&str> = entries().iter().map(|entry| entry.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "ids must be unique");
        assert_eq!(count, 11, "every P4 default app is registered");
        for expected in [
            "aether-files",
            "aether-terminal",
            "aether-browser",
            "aether-text",
            "aether-calculator",
            "aether-images",
            "aether-documents",
            "aether-player",
            "aether-calendar",
            "aether-archive",
            "aether-taskmanager",
        ] {
            assert!(resolve(expected).is_some(), "{expected} is missing");
        }
        assert!(
            resolve("aether-settings").is_none(),
            "Settings is shell-owned"
        );
    }

    #[test]
    fn every_entry_exposes_a_complete_desktop_record() {
        for entry in entries() {
            assert_eq!(entry.exec, entry.id, "{} exec mismatch", entry.id);
            assert!(!entry.name.is_empty(), "{} has no name", entry.id);
            assert!(!entry.icon.is_empty(), "{} has no icon", entry.id);
            assert!(
                !entry.categories.is_empty(),
                "{} has no categories",
                entry.id
            );
            let desktop = desktop_file(entry);
            assert!(desktop.contains("Type=Application"));
            assert!(desktop.contains(&format!("Name={}\n", entry.name)));
            assert!(desktop.contains(&format!("Exec={}\n", entry.exec)));
            assert!(desktop.contains(&format!("X-Aether-Id={}\n", entry.id)));
        }
    }

    #[test]
    fn launch_argv_is_exec_then_launch_then_id() {
        for entry in entries() {
            assert_eq!(
                launch_argv(entry.id),
                Some(vec![
                    entry.exec.to_string(),
                    "--launch".to_string(),
                    entry.id.to_string()
                ])
            );
        }
        assert_eq!(launch_argv("nope"), None);
    }

    #[test]
    fn launch_outcome_accepts_only_the_owning_executable() {
        assert_eq!(
            launch_outcome("aether-files", "aether-files"),
            Ok("AETHER_LAUNCH aether-files".to_string())
        );
        assert_eq!(
            launch_outcome("aether-files", "aether-terminal"),
            Err(LaunchError::WrongExec {
                id: "aether-files".to_string(),
                expected: "aether-files".to_string(),
                got: "aether-terminal".to_string(),
            })
        );
        assert_eq!(
            launch_outcome("aether-nope", "aether-nope"),
            Err(LaunchError::UnknownId("aether-nope".to_string()))
        );
        assert!(LaunchError::MissingId.to_string().contains("--launch"));
    }

    #[test]
    fn launch_decision_continues_for_a_real_launch_and_stops_for_a_check() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            launch_decision("aether-files", &args(&[])),
            LaunchDecision::Continue { marker: None }
        );
        assert_eq!(
            launch_decision("aether-files", &args(&["--render", "out"])),
            LaunchDecision::Continue { marker: None }
        );
        // A real launch keeps running so the window can appear.
        let launch = launch_decision("aether-files", &args(&["--launch", "aether-files"]));
        assert_eq!(
            launch,
            LaunchDecision::Continue {
                marker: Some("AETHER_LAUNCH aether-files".to_string())
            }
        );
        assert_eq!(launch.exit_code(), None);
        assert_eq!(launch.marker(), Some("AETHER_LAUNCH aether-files"));
        // The dry check prints the marker and exits 0.
        let check = launch_decision("aether-files", &args(&["--launch-check", "aether-files"]));
        assert_eq!(
            check,
            LaunchDecision::Exit {
                code: 0,
                marker: Some("AETHER_LAUNCH aether-files".to_string()),
                message: None
            }
        );
        assert_eq!(check.exit_code(), Some(0));
        // Foreign, unknown and missing ids stop with 2 and name the registry.
        for bad in [
            args(&["--launch", "aether-text"]),
            args(&["--launch", "aether-nope"]),
            args(&["--launch"]),
            args(&["--launch-check"]),
        ] {
            match launch_decision("aether-files", &bad) {
                LaunchDecision::Exit {
                    code: 2,
                    message: Some(message),
                    ..
                } => assert!(
                    message.contains("apps/launcher-registry"),
                    "message names the registry: {message}"
                ),
                other => panic!("{bad:?} decided {other:?}"),
            }
        }
    }

    #[test]
    fn handle_launch_arg_strips_the_flags_and_keeps_a_real_launch_running() {
        // `--launch` does not stop the process; the app's own flags survive.
        let startup = handle_launch_arg(
            "aether-files",
            [
                "--launch".to_string(),
                "aether-files".to_string(),
                "--render".to_string(),
                "/tmp/x".to_string(),
            ],
        );
        assert_eq!(startup.exit, None);
        assert_eq!(startup.rest, vec!["--render", "/tmp/x"]);
        // `--launch-check` is the dry handshake.
        let check = handle_launch_arg(
            "aether-files",
            ["--launch-check".to_string(), "aether-files".to_string()],
        );
        assert_eq!(check.exit, Some(0));
        assert!(check.rest.is_empty());
        // Rejections stop with 2 and leave no launch flags behind.
        let missing = handle_launch_arg("aether-files", ["--launch".to_string()]);
        assert_eq!(missing.exit, Some(2));
        assert!(missing.rest.is_empty());
        // No launch flag: the arguments pass through untouched.
        let plain = handle_launch_arg("aether-files", ["--render".to_string()]);
        assert_eq!(plain.exit, None);
        assert_eq!(plain.rest, vec!["--render"]);
    }

    #[test]
    fn every_app_handles_the_launch_flag_with_its_own_exec() {
        // Drift guard: adding an app to the registry without the `--launch`
        // handshake (or renaming its binary) fails here, not in the field.
        let root = workspace_root();
        for entry in entries() {
            let app = root.join("apps").join(entry.crate_dir);
            let manifest = std::fs::read_to_string(app.join("Cargo.toml"))
                .unwrap_or_else(|error| panic!("{}: {error}", app.display()));
            assert!(
                manifest.contains(&format!("name = \"{}\"", entry.id)),
                "{} does not declare package {}",
                app.display(),
                entry.id
            );
            let main = std::fs::read_to_string(app.join("src/main.rs"))
                .unwrap_or_else(|error| panic!("{}: {error}", app.display()));
            // Whitespace-insensitive so rustfmt line breaks do not matter.
            let compact: String = main.split_whitespace().collect();
            assert!(
                compact.contains(&format!("handle_launch_arg(\"{}\"", entry.id)),
                "{} does not call handle_launch_arg(\"{}\")",
                app.display(),
                entry.id
            );
        }
    }
}
