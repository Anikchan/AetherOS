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
//! * [`handle_launch_arg`] is the one-line entry point each app calls at the
//!   top of `main` so `--launch <id>` validates the id and prints the
//!   `AETHER_LAUNCH <id>` handshake marker without opening a window.
//!
//! Headless builds stop after the marker. The interactive window host (P3-T02)
//! keeps the same argv and continues into the Slint event loop instead of
//! exiting, so the shell's spawn line does not change.

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

/// The marker a headless `--launch` prints, used by the acceptance checks.
pub fn launch_marker(id: &str) -> String {
    format!("AETHER_LAUNCH {id}")
}

/// Validate a `--launch <id>` pair for the app whose executable is `exec`.
///
/// Returns the handshake marker on success; the marker is what the app prints
/// before it continues (interactive host) or exits (headless build).
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

/// Handle `--launch <id>` before the app's own argument parsing.
///
/// Returns `Some(exit code)` when the arguments start with `--launch`, and
/// `None` when they do not, so the caller keeps its normal CLI:
///
/// ```ignore
/// fn main() {
///     if let Some(code) =
///         aether_launcher_registry::handle_launch_arg("aether-files", std::env::args().skip(1))
///     {
///         std::process::exit(code);
///     }
///     // ...
/// }
/// ```
pub fn handle_launch_arg(exec: &str, args: impl IntoIterator<Item = String>) -> Option<i32> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("--launch") {
        return None;
    }
    let id = args.next();
    Some(match id {
        Some(id) => match launch_outcome(&id, exec) {
            Ok(marker) => {
                println!("{marker}");
                0
            }
            Err(error) => {
                eprintln!("{exec}: {error}");
                2
            }
        },
        None => {
            eprintln!("{exec}: {}", LaunchError::MissingId);
            2
        }
    })
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
    fn handle_launch_arg_is_silent_without_the_flag() {
        assert_eq!(handle_launch_arg("aether-files", ["--render".into()]), None);
        assert_eq!(
            handle_launch_arg("aether-files", Vec::<String>::new()),
            None
        );
        // Success and failure both return an exit code instead of panicking.
        assert_eq!(
            handle_launch_arg("aether-files", ["--launch".into(), "aether-files".into()]),
            Some(0)
        );
        assert_eq!(
            handle_launch_arg("aether-files", ["--launch".into()]),
            Some(2)
        );
        assert_eq!(
            handle_launch_arg("aether-files", ["--launch".into(), "aether-text".into()]),
            Some(2)
        );
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
