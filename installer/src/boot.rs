//! Boot menu entries (P8-T04, F5 INS-06).
//!
//! systemd-boot reads `loader/loader.conf` and one file per entry. Windows
//! keeps its own boot manager; AetherOS only adds an entry and never modifies
//! the Windows files.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    AetherOs,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootEntry {
    pub title: String,
    pub kind: EntryKind,
    /// Path inside the ESP, using backslashes for the Windows manager.
    pub efi_path: String,
    pub options: String,
}

impl BootEntry {
    pub fn aetheros(title: &str) -> Self {
        Self {
            title: title.to_string(),
            kind: EntryKind::AetherOs,
            efi_path: "\\EFI\\aether\\aetheros-linuxx64.efi".into(),
            options: "quiet loglevel=3".into(),
        }
    }

    pub fn windows(title: &str) -> Self {
        Self {
            title: title.to_string(),
            kind: EntryKind::Windows,
            efi_path: "\\EFI\\Microsoft\\Boot\\bootmgfw.efi".into(),
            options: String::new(),
        }
    }

    /// File name in `loader/entries/`.
    pub fn entry_id(&self) -> String {
        match self.kind {
            EntryKind::AetherOs => "aetheros".into(),
            EntryKind::Windows => "windows".into(),
        }
    }

    pub fn render(&self) -> String {
        let mut out = format!(
            "title {}\nlinux {}\n",
            self.title,
            self.efi_path
        );
        if self.kind == EntryKind::AetherOs {
            out.push_str(&format!("options {}\n", self.options));
        }
        out
    }
}

/// `loader.conf` with AetherOS as the default and a visible timeout.
pub fn render_loader_conf(default: &str, timeout_secs: u32) -> String {
    format!(
        "default {default}.conf\ntimeout {timeout_secs}\nconsole-mode max\neditor no\n"
    )
}

/// All entry files, keyed by file name.
pub fn render_entries(entries: &[BootEntry]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|entry| (format!("{}.conf", entry.entry_id()), entry.render()))
        .collect()
}

/// Full menu bundle: loader.conf plus each entry.
pub fn render_menu(entries: &[BootEntry], default: &str, timeout_secs: u32) -> Vec<(String, String)> {
    let mut files = vec![(
        "loader.conf".to_string(),
        render_loader_conf(default, timeout_secs),
    )];
    files.extend(render_entries(entries));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aetheros_and_windows_entries_are_rendered() {
        let entries = vec![
            BootEntry::aetheros("AetherOS"),
            BootEntry::windows("Windows Boot Manager"),
        ];
        let files = render_menu(&entries, "aetheros", 5);
        assert_eq!(files.len(), 3);
        let loader = &files[0].1;
        assert!(loader.contains("default aetheros.conf"));
        assert!(loader.contains("timeout 5"));
        assert!(loader.contains("editor no"));
        let aether = &files[1].1;
        assert!(aether.contains("title AetherOS"));
        assert!(aether.contains("\\EFI\\aether\\aetheros-linuxx64.efi"));
        let windows = &files[2].1;
        assert!(windows.contains("Windows Boot Manager"));
        assert!(windows.contains("\\EFI\\Microsoft\\Boot\\bootmgfw.efi"));
        assert!(!windows.contains("options"));
    }

    #[test]
    fn entry_ids_are_stable() {
        assert_eq!(BootEntry::aetheros("x").entry_id(), "aetheros");
        assert_eq!(BootEntry::windows("x").entry_id(), "windows");
    }

    #[test]
    fn windows_entry_is_not_modified() {
        // The options field is empty and the path is the standard Windows
        // manager path, so the installer never rewrites Windows files.
        let entry = BootEntry::windows("Windows Boot Manager");
        assert!(entry.options.is_empty());
        assert_eq!(entry.efi_path, "\\EFI\\Microsoft\\Boot\\bootmgfw.efi");
    }
}
