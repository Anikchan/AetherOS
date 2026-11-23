//! Configuration schema (P7-T01, F4 sections 1–4 and CUS-12).
//!
//! Every setting the settings app shows is a key in this table. The table is
//! the single source of truth for validation, defaults and file placement, so
//! the settings-to-file check is a table test rather than a manual audit.

use std::fmt;

use crate::error::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Integer,
    Float,
    String,
    StringList,
    Color,
    Enum(&'static [&'static str]),
}

impl ValueKind {
    pub fn describe(self) -> String {
        match self {
            Self::Bool => "true or false".into(),
            Self::Integer => "an integer".into(),
            Self::Float => "a decimal number".into(),
            Self::String => "text".into(),
            Self::StringList => "a list of strings".into(),
            Self::Color => "a color such as \"#3b82f6\"".into(),
            Self::Enum(values) => format!("one of: {}", values.join(", ")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeySpec {
    /// Dotted key shown to users, for example `dock.position`.
    pub key: &'static str,
    /// File relative to a configuration root, for example `shell/dock.toml`.
    pub file: &'static str,
    /// Key inside that file (same as the part after the first dot).
    pub toml_key: &'static str,
    pub kind: ValueKind,
    /// TOML literal for the default value.
    pub default: &'static str,
    pub description: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl KeySpec {
    pub fn default_value(&self) -> Result<toml::Value, ConfigError> {
        let wrapped = format!("v = {}", self.default);
        toml::from_str::<toml::Value>(&wrapped)
            .ok()
            .and_then(|value| value.get("v").cloned())
            .ok_or_else(|| ConfigError::Schema(format!("bad default for {}", self.key)))
    }

    pub fn validate(&self, value: &toml::Value) -> Result<(), ConfigError> {
        let fail = |expected: String| {
            Err(ConfigError::InvalidValue {
                key: self.key.to_string(),
                expected,
                actual: value.to_string(),
            })
        };
        match self.kind {
            ValueKind::Bool => {
                if value.as_bool().is_none() {
                    return fail(self.kind.describe());
                }
            }
            ValueKind::Integer => {
                let Some(number) = value.as_integer() else {
                    return fail(self.kind.describe());
                };
                if !self.in_range(number as f64) {
                    return fail(format!("an integer between {} and {}", self.min.unwrap_or(f64::MIN), self.max.unwrap_or(f64::MAX)));
                }
            }
            ValueKind::Float => {
                let Some(number) = value.as_float().or_else(|| value.as_integer().map(|v| v as f64)) else {
                    return fail(self.kind.describe());
                };
                if !self.in_range(number) {
                    return fail(format!("a number between {} and {}", self.min.unwrap_or(f64::MIN), self.max.unwrap_or(f64::MAX)));
                }
            }
            ValueKind::String => {
                if value.as_str().is_none() {
                    return fail(self.kind.describe());
                }
            }
            ValueKind::StringList => {
                let Some(list) = value.as_array() else {
                    return fail(self.kind.describe());
                };
                if list.iter().any(|item| item.as_str().is_none()) {
                    return fail(self.kind.describe());
                }
            }
            ValueKind::Color => {
                let Some(text) = value.as_str() else {
                    return fail(self.kind.describe());
                };
                if !is_hex_color(text) {
                    return fail(self.kind.describe());
                }
            }
            ValueKind::Enum(values) => {
                let Some(text) = value.as_str() else {
                    return fail(self.kind.describe());
                };
                if !values.contains(&text) {
                    return fail(self.kind.describe());
                }
            }
        }
        Ok(())
    }

    fn in_range(&self, number: f64) -> bool {
        self.min.map(|min| number >= min).unwrap_or(true)
            && self.max.map(|max| number <= max).unwrap_or(true)
    }
}

impl fmt::Display for KeySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, default {}) — {}",
            self.key,
            self.kind.describe(),
            self.default,
            self.description
        )
    }
}

pub fn is_hex_color(text: &str) -> bool {
    let text = text.trim();
    text.len() == 7
        && text.starts_with('#')
        && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

const POSITIONS: &[&str] = &["bottom", "left", "right"];
const TIERS: &[&str] = &["auto", "full", "balanced", "minimal"];
const MENUBAR_POSITIONS: &[&str] = &["top", "bottom"];
const FOCUS_MODES: &[&str] = &["off", "work", "personal", "sleep"];

/// The complete MVP schema (F4). Adding a settings screen requires adding its
/// keys here first (CUS-12).
pub const SCHEMA: &[KeySpec] = &[
    // Global
    KeySpec { key: "locale", file: "aether.toml", toml_key: "locale", kind: ValueKind::String, default: "\"en-US\"", description: "Language and regional format", min: None, max: None },
    KeySpec { key: "timezone", file: "aether.toml", toml_key: "timezone", kind: ValueKind::String, default: "\"Asia/Kolkata\"", description: "System time zone", min: None, max: None },
    KeySpec { key: "effects.tier", file: "aether.toml", toml_key: "tier", kind: ValueKind::Enum(TIERS), default: "\"auto\"", description: "Effects quality tier (F6)", min: None, max: None },
    // Dock
    KeySpec { key: "dock.position", file: "shell/dock.toml", toml_key: "position", kind: ValueKind::Enum(POSITIONS), default: "\"bottom\"", description: "Dock edge", min: None, max: None },
    KeySpec { key: "dock.icon_size", file: "shell/dock.toml", toml_key: "icon_size", kind: ValueKind::Integer, default: "56", description: "Base icon size in pixels", min: Some(24.0), max: Some(128.0) },
    KeySpec { key: "dock.magnification", file: "shell/dock.toml", toml_key: "magnification", kind: ValueKind::Float, default: "1.4", description: "Magnification factor (1.0 disables)", min: Some(1.0), max: Some(2.5) },
    KeySpec { key: "dock.auto_hide", file: "shell/dock.toml", toml_key: "auto_hide", kind: ValueKind::Bool, default: "true", description: "Hide the dock when a window overlaps", min: None, max: None },
    KeySpec { key: "dock.pinned", file: "shell/dock.toml", toml_key: "pinned", kind: ValueKind::StringList, default: "[\"org.aether.terminal\", \"org.aether.text\"]", description: "Pinned app IDs", min: None, max: None },
    // Menu bar
    KeySpec { key: "menubar.position", file: "shell/menubar.toml", toml_key: "position", kind: ValueKind::Enum(MENUBAR_POSITIONS), default: "\"top\"", description: "Menu bar edge", min: None, max: None },
    KeySpec { key: "menubar.show_clock", file: "shell/menubar.toml", toml_key: "show_clock", kind: ValueKind::Bool, default: "true", description: "Show the clock", min: None, max: None },
    KeySpec { key: "menubar.tray", file: "shell/menubar.toml", toml_key: "tray", kind: ValueKind::Bool, default: "true", description: "Show the system tray", min: None, max: None },
    // Launcher
    KeySpec { key: "launcher.sources", file: "shell/launcher.toml", toml_key: "sources", kind: ValueKind::StringList, default: "[\"apps\", \"files\"]", description: "Search sources", min: None, max: None },
    // Notifications / Focus
    KeySpec { key: "notifications.focus", file: "shell/notifications.toml", toml_key: "focus", kind: ValueKind::Enum(FOCUS_MODES), default: "\"off\"", description: "Focus mode", min: None, max: None },
    KeySpec { key: "notifications.enabled", file: "shell/notifications.toml", toml_key: "enabled", kind: ValueKind::Bool, default: "true", description: "Show notifications", min: None, max: None },
    // Theme tokens (D1 section 5)
    KeySpec { key: "theme.accent", file: "theme/tokens.toml", toml_key: "accent", kind: ValueKind::Color, default: "\"#3b82f6\"", description: "Accent color", min: None, max: None },
    KeySpec { key: "theme.background", file: "theme/tokens.toml", toml_key: "background", kind: ValueKind::Color, default: "\"#0f1115\"", description: "Window background", min: None, max: None },
    KeySpec { key: "theme.foreground", file: "theme/tokens.toml", toml_key: "foreground", kind: ValueKind::Color, default: "\"#e8eaed\"", description: "Primary text color", min: None, max: None },
    KeySpec { key: "theme.radius", file: "theme/tokens.toml", toml_key: "radius", kind: ValueKind::Integer, default: "12", description: "Corner radius in pixels", min: Some(0.0), max: Some(40.0) },
    KeySpec { key: "theme.blur", file: "theme/tokens.toml", toml_key: "blur", kind: ValueKind::Integer, default: "16", description: "Glass blur strength", min: Some(0.0), max: Some(64.0) },
    KeySpec { key: "theme.tint", file: "theme/tokens.toml", toml_key: "tint", kind: ValueKind::Float, default: "0.35", description: "Glass tint opacity", min: Some(0.0), max: Some(1.0) },
    KeySpec { key: "theme.font", file: "theme/tokens.toml", toml_key: "font", kind: ValueKind::String, default: "\"Inter\"", description: "Interface font family", min: None, max: None },
    KeySpec { key: "theme.spacing", file: "theme/tokens.toml", toml_key: "spacing", kind: ValueKind::Integer, default: "8", description: "Base spacing unit", min: Some(2.0), max: Some(32.0) },
    // Process lifecycle (F2/D3)
    KeySpec { key: "process.grace_ac_seconds", file: "process/lifecycle.toml", toml_key: "grace_ac_seconds", kind: ValueKind::Integer, default: "30", description: "Background grace period on AC", min: Some(1.0), max: Some(600.0) },
    KeySpec { key: "process.grace_battery_seconds", file: "process/lifecycle.toml", toml_key: "grace_battery_seconds", kind: ValueKind::Integer, default: "10", description: "Background grace period on battery", min: Some(1.0), max: Some(600.0) },
    KeySpec { key: "process.hibernate_after_minutes", file: "process/lifecycle.toml", toml_key: "hibernate_after_minutes", kind: ValueKind::Integer, default: "10", description: "Hibernation delay after suspension", min: Some(1.0), max: Some(240.0) },
];

/// Schema lookup by user-facing key.
pub fn spec_for(key: &str) -> Option<&'static KeySpec> {
    SCHEMA.iter().find(|spec| spec.key == key)
}

/// All files referenced by the schema (unique, sorted).
pub fn schema_files() -> Vec<&'static str> {
    let mut files: Vec<&'static str> = SCHEMA.iter().map(|spec| spec.file).collect();
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_a_valid_default() {
        for spec in SCHEMA {
            let value = spec
                .default_value()
                .unwrap_or_else(|error| panic!("{}: {error}", spec.key));
            spec.validate(&value)
                .unwrap_or_else(|error| panic!("{}: {error}", spec.key));
        }
    }

    #[test]
    fn every_key_maps_to_a_file_and_toml_key() {
        for spec in SCHEMA {
            assert!(!spec.file.is_empty(), "{} has no file", spec.key);
            assert!(!spec.toml_key.is_empty(), "{} has no toml key", spec.key);
            let parts: Vec<&str> = spec.key.split('.').collect();
            assert!(!parts.is_empty());
            assert_eq!(
                *parts.last().unwrap(),
                spec.toml_key,
                "{} key/toml mismatch",
                spec.key
            );
            if spec.file != "aether.toml" {
                assert_eq!(parts.len(), 2, "{} must be section.key", spec.key);
            }
        }
    }

    #[test]
    fn keys_are_unique_and_files_cover_the_layout() {
        let mut keys: Vec<&str> = SCHEMA.iter().map(|s| s.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), SCHEMA.len(), "duplicate key in schema");
        let files = schema_files();
        for expected in [
            "aether.toml",
            "shell/dock.toml",
            "shell/menubar.toml",
            "shell/launcher.toml",
            "shell/notifications.toml",
            "theme/tokens.toml",
            "process/lifecycle.toml",
        ] {
            assert!(files.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn validation_rejects_out_of_range_and_bad_enums() {
        let icon = spec_for("dock.icon_size").unwrap();
        assert!(icon.validate(&toml::Value::Integer(8)).is_err());
        assert!(icon.validate(&toml::Value::Integer(200)).is_err());
        assert!(icon.validate(&toml::Value::Integer(56)).is_ok());

        let position = spec_for("dock.position").unwrap();
        assert!(position
            .validate(&toml::Value::String("middle".into()))
            .is_err());
        assert!(position
            .validate(&toml::Value::String("left".into()))
            .is_ok());

        let accent = spec_for("theme.accent").unwrap();
        assert!(accent.validate(&toml::Value::String("blue".into())).is_err());
        assert!(accent.validate(&toml::Value::String("#aabbcc".into())).is_ok());
    }

    #[test]
    fn every_settings_screen_has_a_key() {
        // CUS-AC-05 automated coverage check: each screen named in F4 §2 has
        // at least one key in the schema.
        let screens = [
            "shell/menubar.toml",
            "shell/dock.toml",
            "shell/launcher.toml",
            "shell/notifications.toml",
            "theme/tokens.toml",
            "process/lifecycle.toml",
        ];
        for screen in screens {
            assert!(
                SCHEMA.iter().any(|spec| spec.file == screen),
                "settings screen {screen} has no keys"
            );
        }
    }
}
