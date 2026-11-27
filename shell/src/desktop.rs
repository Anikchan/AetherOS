//! `.desktop` entry discovery for the launcher and the dock (P4, S3).
//!
//! The shell reads the standard XDG application directories, so apps installed
//! by the P6 package platform show up without a shell rebuild. When the system
//! has no entries (the macOS dev host, a fresh image), a built-in list of
//! AetherOS default apps keeps the dock and launcher usable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::IconKind;

use crate::model::App;

/// One application menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    pub exec: String,
    pub icon_name: String,
    pub terminal: bool,
    pub no_display: bool,
    pub categories: Vec<String>,
}

impl DesktopEntry {
    pub fn icon(&self) -> IconKind {
        icon_for(&self.icon_name, &self.name, &self.categories)
    }

    pub fn to_app(&self, pinned: bool) -> App {
        App {
            id: self.id.clone(),
            name: self.name.clone(),
            icon: self.icon(),
            exec: self.exec.clone(),
            running: false,
            pinned,
            badge: 0,
        }
    }
}

/// Parse a `.desktop` file. Only `[Desktop Entry]` keys are read; localized
/// `Name[xx]` variants are skipped in favour of the unlocalized value.
pub fn parse_entry(id: &str, content: &str) -> Option<DesktopEntry> {
    let mut in_group = false;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut icon_name = String::new();
    let mut terminal = false;
    let mut no_display = false;
    let mut hidden = false;
    let mut entry_type = String::new();
    let mut categories = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Type" => entry_type = value.to_string(),
            "Name" => name = Some(value.to_string()),
            "Exec" => exec = Some(strip_field_codes(value)),
            "Icon" => icon_name = value.to_string(),
            "Terminal" => terminal = value.eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "Categories" => {
                categories = value
                    .split(';')
                    .filter(|part| !part.is_empty())
                    .map(ToString::to_string)
                    .collect()
            }
            _ => {}
        }
    }

    if hidden || entry_type != "Application" {
        return None;
    }
    let name = name?;
    let exec = exec?;
    if exec.is_empty() {
        return None;
    }
    Some(DesktopEntry {
        id: id.to_string(),
        name,
        exec,
        icon_name,
        terminal,
        no_display,
        categories,
    })
}

/// Remove the `%U`, `%f`, `%i` and friends field codes from an `Exec` line.
pub fn strip_field_codes(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|token| !token.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read every `*.desktop` file in one directory (non-recursive).
pub fn scan_dir(dir: &Path) -> Vec<DesktopEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(entry) = parse_entry(id, &content) {
            found.push(entry);
        }
    }
    found
}

/// XDG application directories in precedence order.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        if !data_home.is_empty() {
            dirs.push(Path::new(&data_home).join("applications"));
        }
    } else if let Ok(home) = std::env::var("HOME") {
        dirs.push(Path::new(&home).join(".local/share/applications"));
    }
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
            dirs.push(Path::new(dir).join("applications"));
        }
    }
    if dirs.len() <= 2 {
        dirs.push(PathBuf::from("/usr/local/share/applications"));
        dirs.push(PathBuf::from("/usr/share/applications"));
    }
    dirs
}

/// All visible entries, de-duplicated by id (first directory wins).
pub fn scan_default() -> Vec<DesktopEntry> {
    let mut by_id: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for dir in search_dirs() {
        for entry in scan_dir(&dir) {
            if entry.no_display {
                continue;
            }
            by_id.entry(entry.id.clone()).or_insert(entry);
        }
    }
    by_id.into_values().collect()
}

/// Icon heuristic over the freedesktop icon name and categories.
pub fn icon_for(icon_name: &str, app_name: &str, categories: &[String]) -> IconKind {
    let haystack = format!(
        "{} {} {}",
        icon_name.to_lowercase(),
        app_name.to_lowercase(),
        categories.join(" ").to_lowercase()
    );
    let probe = |needles: &[&str]| needles.iter().any(|needle| haystack.contains(needle));

    if probe(&["terminal", "console", "shell", "konsole", "alacritty"]) {
        IconKind::Terminal
    } else if probe(&["folder", "filemanager", "file-manager", "files", "nautilus", "dolphin"]) {
        IconKind::Folder
    } else if probe(&["image", "photo", "picture", "gwenview", "eog"]) {
        IconKind::Image
    } else if probe(&["web", "browser", "firefox", "chrom", "epiphany", "webkit"]) {
        IconKind::Globe
    } else if probe(&["video", "music", "player", "vlc", "mpv", "rhythmbox"]) {
        IconKind::Play
    } else if probe(&["calc"]) {
        IconKind::Calculator
    } else if probe(&["calendar", "clock", "date"]) {
        IconKind::Calendar
    } else if probe(&["mail", "message", "thunderbird", "chat"]) {
        IconKind::Bell
    } else if probe(&["archive", "zip", "ark", "compressed"]) {
        IconKind::Archive
    } else if probe(&["settings", "preferences", "control"]) {
        IconKind::Gear
    } else if probe(&["document", "writer", "word", "pdf", "text", "editor"]) {
        IconKind::Document
    } else if probe(&["monitor", "activity", "system"]) {
        IconKind::Activity
    } else {
        IconKind::Apps
    }
}

/// Launcher ids the shell itself owns: they are shell surfaces, not processes,
/// so they have no registry entry and nothing to spawn.
pub const SHELL_OWNED_IDS: &[&str] = &["aether-settings"];

/// The pinned dock layout on a fresh install (ids, in dock order).
pub const PINNED_IDS: &[&str] = &[
    "aether-files",
    "aether-terminal",
    "aether-browser",
    "aether-text",
];

/// Apps the dock shows as already running on a fresh session (the demo set the
/// dock has always presented, pinned in [`PINNED_IDS`] order).
pub const RUNNING_IDS: &[&str] = &["aether-files", "aether-terminal", "aether-browser"];

/// The launcher model of every app in `aether-launcher-registry`, in registry
/// order. Names, icons and `Exec` come from the registry, so the shell cannot
/// drift from the apps; only the pinning and running demo set is shell policy.
pub fn registry_apps() -> Vec<App> {
    aether_launcher_registry::entries()
        .iter()
        .map(|entry| {
            let categories: Vec<String> = entry
                .categories
                .iter()
                .map(|category| category.to_string())
                .collect();
            App {
                id: entry.id.to_string(),
                name: entry.name.to_string(),
                icon: icon_for(entry.icon, entry.name, &categories),
                exec: entry.exec.to_string(),
                running: RUNNING_IDS.contains(&entry.id),
                pinned: PINNED_IDS.contains(&entry.id),
                badge: 0,
            }
        })
        .collect()
}

/// Built-in default apps: every registry app, then the shell-owned surfaces
/// (Settings). Used when no `.desktop` files exist and as the pinned dock
/// layout on a fresh install.
pub fn default_apps() -> Vec<App> {
    let mut apps = registry_apps();
    apps.push(App {
        id: "aether-settings".to_string(),
        name: "Settings".to_string(),
        icon: IconKind::Gear,
        exec: "aether-settings".to_string(),
        running: false,
        pinned: false,
        badge: 0,
    });
    apps
}

/// Merge discovered entries with the built-in list: discovered ids win, built-in
/// apps stay available so the shell is never empty.
pub fn merged_apps(discovered: Vec<DesktopEntry>) -> Vec<App> {
    let mut apps = default_apps();
    for entry in discovered {
        if let Some(existing) = apps.iter_mut().find(|app| app.id == entry.id) {
            existing.name = entry.name.clone();
            existing.icon = entry.icon();
            existing.exec = entry.exec.clone();
            continue;
        }
        let pinned = matches!(
            entry.id.as_str(),
            "org.gnome.Nautilus" | "org.gnome.Console" | "firefox" | "org.gnome.TextEditor"
        );
        apps.push(entry.to_app(pinned));
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[Desktop Entry]
Type=Application
Name=AetherPad
Name[de]=AetherPad DE
Exec=aether-text %U
Icon=accessories-text-editor
Terminal=false
Categories=Utility;TextEditor;
"#;

    #[test]
    fn parses_a_desktop_entry() {
        let entry = parse_entry("aether-text", SAMPLE).unwrap();
        assert_eq!(entry.name, "AetherPad");
        assert_eq!(entry.exec, "aether-text");
        assert_eq!(entry.icon(), IconKind::Document);
        assert!(!entry.terminal);
    }

    #[test]
    fn rejects_non_applications_and_hidden_entries() {
        let link = "[Desktop Entry]\nType=Link\nName=Docs\nExec=true\n";
        assert!(parse_entry("x", link).is_none());
        let hidden = "[Desktop Entry]\nType=Application\nName=X\nExec=x\nHidden=true\n";
        assert!(parse_entry("x", hidden).is_none());
        let no_exec = "[Desktop Entry]\nType=Application\nName=X\n";
        assert!(parse_entry("x", no_exec).is_none());
    }

    #[test]
    fn strips_exec_field_codes() {
        assert_eq!(strip_field_codes("firefox %u"), "firefox");
        assert_eq!(
            strip_field_codes("aether-text --new %F"),
            "aether-text --new"
        );
    }

    #[test]
    fn icon_heuristics_cover_common_apps() {
        assert_eq!(icon_for("utilities-terminal", "Terminal", &[]), IconKind::Terminal);
        assert_eq!(icon_for("firefox", "Firefox", &[]), IconKind::Globe);
        assert_eq!(icon_for("", "VLC media player", &[]), IconKind::Play);
        assert_eq!(icon_for("", "Something", &["Graphics".into()]), IconKind::Apps);
        assert_eq!(
            icon_for("", "Nautilus", &["FileManager".into()]),
            IconKind::Folder
        );
    }

    #[test]
    fn default_apps_include_every_p4_app() {
        let apps = default_apps();
        assert!(apps.iter().any(|app| app.id == "aether-text"));
        assert!(apps.iter().any(|app| app.id == "aether-browser"));
        assert_eq!(
            apps.iter().filter(|app| app.pinned).count(),
            4,
            "dock ships with Files, Terminal, Browser and AetherPad pinned"
        );
        assert!(apps.iter().any(|app| app.running));
    }

    #[test]
    fn the_shell_app_list_equals_the_registry_plus_shell_owned_surfaces() {
        let apps = default_apps();
        let registry = aether_launcher_registry::entries();
        assert_eq!(
            apps.len(),
            registry.len() + SHELL_OWNED_IDS.len(),
            "the model is the registry plus the shell's own surfaces"
        );
        for (app, entry) in apps.iter().zip(registry.iter()) {
            assert_eq!(app.id, entry.id, "registry order is launcher order");
            assert_eq!(app.name, entry.name, "{} name", entry.id);
            assert_eq!(app.exec, entry.exec, "{} exec", entry.id);
            assert_eq!(
                app.icon,
                icon_for(entry.icon, entry.name, &[]),
                "{} icon comes from the registry icon name",
                entry.id
            );
        }
        for id in SHELL_OWNED_IDS {
            assert!(
                apps.iter().any(|app| app.id == *id),
                "{id} is a shell surface and must still be listed"
            );
            assert!(
                aether_launcher_registry::resolve(id).is_none(),
                "{id} is shell-owned, so it has no registry entry"
            );
        }
    }

    #[test]
    fn merged_apps_prefer_discovered_metadata() {
        let entry = parse_entry("aether-text", SAMPLE).unwrap();
        let apps = merged_apps(vec![entry]);
        let text = apps.iter().find(|app| app.id == "aether-text").unwrap();
        assert_eq!(text.name, "AetherPad");
        assert!(text.pinned, "built-in pinning is preserved");
        assert_eq!(apps.len(), default_apps().len(), "no duplicate ids");
    }

    #[test]
    fn scan_dir_reads_fixture_files() {
        let dir = std::env::temp_dir().join(format!(
            "aether-desktop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("aether-text.desktop"), SAMPLE).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let entries = scan_dir(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "aether-text");
        let _ = std::fs::remove_dir_all(dir);
    }
}
