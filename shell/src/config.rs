//! Settings schema and layered configuration files (P4-T07, F4).
//!
//! The settings app is a visual editor for TOML keys: every row shows the file
//! and key it writes (`key` + `file`), which is the P4-T07 acceptance condition
//! "every setting maps to a file key". Files are layered per F4 section 1:
//! system defaults, then `/etc/aether`, then `~/.config/aether` — user overrides
//! win. Writes are atomic and a file that fails to parse is never overwritten.

use std::fs;
use std::path::{Path, PathBuf};

use crate::IconKind;
use serde::Serialize;

/// Settings categories shown in the sidebar (F1 SHL-11 list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Display,
    Sound,
    Network,
    Power,
    Keyboard,
    Mouse,
    Apps,
    Notifications,
    Privacy,
    Features,
    About,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Appearance,
        Category::Display,
        Category::Sound,
        Category::Network,
        Category::Power,
        Category::Keyboard,
        Category::Mouse,
        Category::Apps,
        Category::Notifications,
        Category::Privacy,
        Category::Features,
        Category::About,
    ];

    /// Sidebar index of Settings → Features (P7-T07); the surface shows the
    /// pack page instead of schema rows at this index.
    pub const FEATURES_INDEX: usize = 10;

    pub fn name(self) -> &'static str {
        match self {
            Category::Appearance => "Appearance",
            Category::Display => "Display",
            Category::Sound => "Sound",
            Category::Network => "Network",
            Category::Power => "Power",
            Category::Keyboard => "Keyboard",
            Category::Mouse => "Mouse & touchpad",
            Category::Apps => "Apps",
            Category::Notifications => "Notifications",
            Category::Privacy => "Privacy",
            Category::Features => "Features",
            Category::About => "About",
        }
    }

    pub fn icon(self) -> IconKind {
        match self {
            Category::Appearance => IconKind::Sun,
            Category::Display => IconKind::Display,
            Category::Sound => IconKind::Volume,
            Category::Network => IconKind::Wifi,
            Category::Power => IconKind::Battery,
            Category::Keyboard => IconKind::Keyboard,
            Category::Mouse => IconKind::Mouse,
            Category::Apps => IconKind::Apps,
            Category::Notifications => IconKind::Bell,
            Category::Privacy => IconKind::Shield,
            Category::Features => IconKind::Download,
            Category::About => IconKind::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    Toggle,
    Select(&'static [&'static str]),
    Value,
}

impl SettingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SettingKind::Toggle => "toggle",
            SettingKind::Select(_) => "select",
            SettingKind::Value => "value",
        }
    }
}

/// One configurable key.
#[derive(Debug, Clone, Copy)]
pub struct SettingDef {
    pub key: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub kind: SettingKind,
    pub default: &'static str,
    pub category: Category,
}

impl SettingDef {
    pub fn category_icon(&self) -> IconKind {
        self.category.icon()
    }
}

/// The schema. Adding a setting here is the only change needed to make it
/// appear in the settings app and in launcher search.
pub const SETTINGS: &[SettingDef] = &[
    // Appearance — theme tokens (D1 section 5).
    SettingDef { key: "theme.appearance", label: "Appearance", file: "theme/tokens.toml", kind: SettingKind::Select(&["dark", "light", "auto"]), default: "dark", category: Category::Appearance },
    SettingDef { key: "theme.accent", label: "Accent colour", file: "theme/tokens.toml", kind: SettingKind::Value, default: "#4C8DFF", category: Category::Appearance },
    SettingDef { key: "theme.effects-tier", label: "Effects tier", file: "theme/tokens.toml", kind: SettingKind::Select(&["full", "reduced", "minimal"]), default: "full", category: Category::Appearance },
    SettingDef { key: "theme.reduce-transparency", label: "Reduce transparency", file: "theme/tokens.toml", kind: SettingKind::Toggle, default: "false", category: Category::Appearance },
    SettingDef { key: "theme.reduce-motion", label: "Reduce motion", file: "theme/tokens.toml", kind: SettingKind::Toggle, default: "false", category: Category::Appearance },
    SettingDef { key: "theme.text-scale", label: "Text scale", file: "theme/tokens.toml", kind: SettingKind::Select(&["100%", "125%", "150%", "175%", "200%"]), default: "100%", category: Category::Appearance },
    SettingDef { key: "theme.wallpaper", label: "Wallpaper", file: "theme/tokens.toml", kind: SettingKind::Value, default: "gradient-default", category: Category::Appearance },

    // Display.
    SettingDef { key: "display.scale", label: "Scale", file: "display.toml", kind: SettingKind::Value, default: "1.0", category: Category::Display },
    SettingDef { key: "display.night-light", label: "Night light", file: "display.toml", kind: SettingKind::Toggle, default: "false", category: Category::Display },
    SettingDef { key: "display.night-light-temperature", label: "Night light warmth", file: "display.toml", kind: SettingKind::Value, default: "3500", category: Category::Display },
    SettingDef { key: "display.refresh-rate", label: "Refresh rate", file: "display.toml", kind: SettingKind::Value, default: "60", category: Category::Display },

    // Sound.
    SettingDef { key: "sound.output-volume", label: "Output volume", file: "sound.toml", kind: SettingKind::Value, default: "0.62", category: Category::Sound },
    SettingDef { key: "sound.input-volume", label: "Input volume", file: "sound.toml", kind: SettingKind::Value, default: "0.40", category: Category::Sound },
    SettingDef { key: "sound.output-device", label: "Output device", file: "sound.toml", kind: SettingKind::Value, default: "auto", category: Category::Sound },
    SettingDef { key: "sound.alert-sound", label: "Alert sounds", file: "sound.toml", kind: SettingKind::Toggle, default: "true", category: Category::Sound },

    // Network.
    SettingDef { key: "network.wifi", label: "Wi-Fi", file: "network.toml", kind: SettingKind::Toggle, default: "true", category: Category::Network },
    SettingDef { key: "network.bluetooth", label: "Bluetooth", file: "network.toml", kind: SettingKind::Toggle, default: "true", category: Category::Network },
    SettingDef { key: "network.airplane-mode", label: "Airplane mode", file: "network.toml", kind: SettingKind::Toggle, default: "false", category: Category::Network },
    SettingDef { key: "network.hostname", label: "Hostname", file: "network.toml", kind: SettingKind::Value, default: "aether", category: Category::Network },

    // Power.
    SettingDef { key: "power.power-mode", label: "Power mode", file: "power/power.toml", kind: SettingKind::Select(&["saver", "balanced", "performance"]), default: "balanced", category: Category::Power },
    SettingDef { key: "power.charge-limit", label: "Charge limit", file: "power/power.toml", kind: SettingKind::Value, default: "80", category: Category::Power },
    SettingDef { key: "power.low-battery-threshold", label: "Low battery warning", file: "power/power.toml", kind: SettingKind::Value, default: "15", category: Category::Power },
    SettingDef { key: "power.suspend-on-lid", label: "Suspend on lid close", file: "power/power.toml", kind: SettingKind::Toggle, default: "true", category: Category::Power },

    // Keyboard.
    SettingDef { key: "input.layout", label: "Keyboard layout", file: "input/keybindings.toml", kind: SettingKind::Value, default: "us", category: Category::Keyboard },
    SettingDef { key: "input.repeat-delay", label: "Repeat delay (ms)", file: "input/keybindings.toml", kind: SettingKind::Value, default: "250", category: Category::Keyboard },
    SettingDef { key: "input.repeat-rate", label: "Repeat rate (per second)", file: "input/keybindings.toml", kind: SettingKind::Value, default: "30", category: Category::Keyboard },
    SettingDef { key: "input.launcher-shortcut", label: "Launcher shortcut", file: "input/keybindings.toml", kind: SettingKind::Value, default: "Super+Space", category: Category::Keyboard },

    // Mouse and touchpad.
    SettingDef { key: "input.pointer.natural-scroll", label: "Natural scrolling", file: "input/pointer.toml", kind: SettingKind::Toggle, default: "true", category: Category::Mouse },
    SettingDef { key: "input.pointer.tap-to-click", label: "Tap to click", file: "input/pointer.toml", kind: SettingKind::Toggle, default: "true", category: Category::Mouse },
    SettingDef { key: "input.pointer.speed", label: "Pointer speed", file: "input/pointer.toml", kind: SettingKind::Value, default: "0.50", category: Category::Mouse },
    SettingDef { key: "input.pointer.touchpad-gestures", label: "Touchpad gestures", file: "input/pointer.toml", kind: SettingKind::Toggle, default: "true", category: Category::Mouse },

    // Default apps.
    SettingDef { key: "apps.default-browser", label: "Default browser", file: "apps/defaults.toml", kind: SettingKind::Value, default: "aether-browser", category: Category::Apps },
    SettingDef { key: "apps.default-terminal", label: "Default terminal", file: "apps/defaults.toml", kind: SettingKind::Value, default: "aether-terminal", category: Category::Apps },
    SettingDef { key: "apps.default-text-editor", label: "Default text editor", file: "apps/defaults.toml", kind: SettingKind::Value, default: "aether-text", category: Category::Apps },
    SettingDef { key: "apps.default-player", label: "Default player", file: "apps/defaults.toml", kind: SettingKind::Value, default: "aether-player", category: Category::Apps },

    // Notifications and Focus (F1 SHL-05). A schedule is a daily window; the
    // shell suppresses notification cards while it is active.
    SettingDef { key: "notifications.focus-enabled", label: "Focus schedule", file: "notifications.toml", kind: SettingKind::Toggle, default: "false", category: Category::Notifications },
    SettingDef { key: "notifications.focus-start", label: "Focus starts", file: "notifications.toml", kind: SettingKind::Value, default: "09:00", category: Category::Notifications },
    SettingDef { key: "notifications.focus-end", label: "Focus ends", file: "notifications.toml", kind: SettingKind::Value, default: "17:00", category: Category::Notifications },
    SettingDef { key: "notifications.show-previews", label: "Show message previews", file: "notifications.toml", kind: SettingKind::Toggle, default: "true", category: Category::Notifications },
    SettingDef { key: "notifications.dnd-until", label: "Do not disturb until", file: "notifications.toml", kind: SettingKind::Value, default: "18:00", category: Category::Notifications },

    // Privacy and security.
    SettingDef { key: "privacy.screen-lock-timeout", label: "Lock after (seconds)", file: "privacy.toml", kind: SettingKind::Value, default: "300", category: Category::Privacy },
    SettingDef { key: "privacy.lock-on-suspend", label: "Lock on suspend", file: "privacy.toml", kind: SettingKind::Toggle, default: "true", category: Category::Privacy },
    SettingDef { key: "privacy.telemetry", label: "Anonymous usage data", file: "privacy.toml", kind: SettingKind::Toggle, default: "false", category: Category::Privacy },
    SettingDef { key: "privacy.camera-indicator", label: "Camera indicator", file: "privacy.toml", kind: SettingKind::Toggle, default: "true", category: Category::Privacy },

    // About (read-only values).
    SettingDef { key: "about.version", label: "AetherOS version", file: "about.toml", kind: SettingKind::Value, default: "0.1.0-p4", category: Category::About },
    SettingDef { key: "about.compositor", label: "Compositor", file: "about.toml", kind: SettingKind::Value, default: "aether-compositor (Smithay)", category: Category::About },
    SettingDef { key: "about.kernel", label: "Kernel", file: "about.toml", kind: SettingKind::Value, default: "6.x aether", category: Category::About },
];

pub fn settings_for(category: Category) -> Vec<SettingDef> {
    SETTINGS
        .iter()
        .filter(|setting| setting.category == category)
        .copied()
        .collect()
}

pub fn find_setting(key: &str) -> Option<&'static SettingDef> {
    SETTINGS.iter().find(|setting| setting.key == key)
}

/// A row as rendered by the settings app.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettingRow {
    pub key: String,
    pub label: String,
    pub file: String,
    pub kind: String,
    pub value: String,
    pub on: bool,
}

impl SettingRow {
    pub fn from_def(def: &SettingDef, raw: Option<&toml::Value>) -> SettingRow {
        let value = raw
            .map(render_value)
            .unwrap_or_else(|| def.default.to_string());
        let on = raw
            .map(|value| value.as_bool().unwrap_or(value.as_str() == Some("true")))
            .unwrap_or(def.default == "true");
        SettingRow {
            key: def.key.to_string(),
            label: def.label.to_string(),
            file: def.file.to_string(),
            kind: def.kind.as_str().to_string(),
            value,
            on,
        }
    }
}

pub fn render_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        toml::Value::Boolean(flag) => flag.to_string(),
        toml::Value::Integer(number) => number.to_string(),
        toml::Value::Float(number) => {
            let text = format!("{number:.2}");
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        other => other.to_string(),
    }
}

/// Layered settings store rooted at the user config directory.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    /// User layer: the only layer that is ever written.
    root: PathBuf,
    /// Read-only layers in precedence order: system defaults, then `/etc`.
    layers: Vec<PathBuf>,
}

impl SettingsStore {
    pub fn new(root: impl Into<PathBuf>) -> SettingsStore {
        SettingsStore {
            root: root.into(),
            layers: Vec::new(),
        }
    }

    /// User layer plus read-only layers (highest precedence first). Saves always
    /// go to the user layer, so a system default can never be modified.
    pub fn with_layers(root: impl Into<PathBuf>, layers: Vec<PathBuf>) -> SettingsStore {
        SettingsStore {
            root: root.into(),
            layers,
        }
    }

    /// F4 layering: `~/.config/aether` (user), `/etc/aether` (system overrides),
    /// `/usr/share/aether/defaults` (shipped defaults). Later layers lose.
    pub fn at_default_location() -> SettingsStore {
        let user = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                Path::new(&xdg).join("aether")
            } else {
                Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".config")
                    .join("aether")
            }
        } else {
            Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".config")
                .join("aether")
        };
        SettingsStore::with_layers(
            user,
            vec![
                PathBuf::from("/etc/aether"),
                PathBuf::from("/usr/share/aether/defaults"),
            ],
        )
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read-only layers in precedence order.
    pub fn layers(&self) -> &[PathBuf] {
        &self.layers
    }

    fn file_path(&self, def: &SettingDef) -> PathBuf {
        self.root.join(def.file)
    }

    /// Read one key. A missing file or key yields `None` (the default applies).
    pub fn load(&self, key: &str) -> Result<Option<toml::Value>, String> {
        let def = find_setting(key).ok_or_else(|| format!("unknown setting {key}"))?;
        self.load_def(def)
    }

    fn load_def(&self, def: &SettingDef) -> Result<Option<toml::Value>, String> {
        // Highest precedence first: user layer, then each read-only layer.
        for base in std::iter::once(&self.root).chain(self.layers.iter()) {
            if let Some(value) = read_key_in(&base.join(def.file), def)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    /// Write one key atomically. Nested keys are created as tables.
    pub fn save(&self, key: &str, value: toml::Value) -> Result<(), String> {
        let def = find_setting(key).ok_or_else(|| format!("unknown setting {key}"))?;
        let path = self.file_path(def);
        let mut table = match fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<toml::Table>(&text).map_err(|error| {
                format!(
                    "{} is not valid TOML ({error}); refusing to overwrite it",
                    path.display()
                )
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };

        set_nested(&mut table, &def.key.split('.').collect::<Vec<_>>(), value);
        let serialised = toml::to_string_pretty(&table)
            .map_err(|error| format!("cannot serialise {}: {error}", path.display()))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let temp = path.with_extension("toml.tmp");
        fs::write(&temp, serialised).map_err(|error| format!("cannot write {}: {error}", temp.display()))?;
        fs::rename(&temp, &path).map_err(|error| format!("cannot replace {}: {error}", path.display()))?;
        Ok(())
    }

    /// Every row of one category, with stored values applied.
    pub fn rows_for(&self, category: Category) -> Vec<SettingRow> {
        settings_for(category)
            .iter()
            .map(|def| SettingRow::from_def(def, self.load_def(def).ok().flatten().as_ref()))
            .collect()
    }

    /// Toggle a boolean key and return the stored value.
    pub fn toggle(&self, key: &str) -> Result<bool, String> {
        let current = self
            .load(key)?
            .and_then(|value| value.as_bool())
            .unwrap_or_else(|| {
                find_setting(key)
                    .map(|def| def.default == "true")
                    .unwrap_or(false)
            });
        let next = !current;
        self.save(key, toml::Value::Boolean(next))?;
        Ok(next)
    }
}

/// Read `def.key` from one TOML file. A missing file or key yields `None`; an
/// invalid file is an error so the caller never silently loses a layer.
fn read_key_in(path: &Path, def: &SettingDef) -> Result<Option<toml::Value>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let table: toml::Table = toml::from_str(&text).map_err(|error| {
        format!(
            "{} is not valid TOML ({error}); keeping the last valid configuration",
            path.display()
        )
    })?;
    let mut current = toml::Value::Table(table);
    for segment in def.key.split('.') {
        match current.get(segment) {
            Some(value) => current = value.clone(),
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

fn set_nested(table: &mut toml::Table, path: &[&str], value: toml::Value) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        table.insert(path[0].to_string(), value);
        return;
    }
    let entry = table
        .entry(path[0].to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !entry.is_table() {
        *entry = toml::Value::Table(toml::Table::new());
    }
    if let Some(inner) = entry.as_table_mut() {
        set_nested(inner, &path[1..], value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "aether-settings-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        base
    }

    #[test]
    fn every_setting_maps_to_a_file_and_key() {
        for def in SETTINGS {
            assert!(!def.key.is_empty(), "key is required");
            assert!(
                def.file.ends_with(".toml"),
                "{} must point at a TOML file",
                def.key
            );
            assert!(def.key.contains('.'), "{} must be a dotted key", def.key);
            assert!(
                def.file.contains('/') || def.file.ends_with(".toml"),
                "{} file layout",
                def.key
            );
        }
        // Keys must be unique.
        let mut keys: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        let count = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), count);
    }

    #[test]
    fn every_schema_category_has_at_least_one_setting() {
        for category in Category::ALL {
            // Features is the P7-T07 pack page, backed by features.toml
            // instead of the F4 schema, so it has no SettingDef rows.
            if category == Category::Features {
                continue;
            }
            assert!(
                !settings_for(category).is_empty(),
                "{} must expose settings",
                category.name()
            );
        }
        assert!(settings_for(Category::Features).is_empty());
        assert_eq!(Category::ALL[Category::FEATURES_INDEX], Category::Features);
    }

    #[test]
    fn save_then_load_round_trips_nested_keys() {
        let root = temp_root("roundtrip");
        let store = SettingsStore::new(&root);
        store
            .save("theme.reduce-transparency", toml::Value::Boolean(true))
            .unwrap();
        assert_eq!(
            store.load("theme.reduce-transparency").unwrap(),
            Some(toml::Value::Boolean(true))
        );
        // A second key in the same file must not clobber the first.
        store
            .save("theme.appearance", toml::Value::String("light".into()))
            .unwrap();
        assert_eq!(
            store.load("theme.appearance").unwrap(),
            Some(toml::Value::String("light".into()))
        );
        assert_eq!(
            store.load("theme.reduce-transparency").unwrap(),
            Some(toml::Value::Boolean(true))
        );
        let text = fs::read_to_string(root.join("theme/tokens.toml")).unwrap();
        assert!(text.contains("[theme]"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn layers_apply_in_precedence_and_saves_go_to_the_user_layer() {
        let user = temp_root("layer-user");
        let system = temp_root("layer-system");
        let shipped = temp_root("layer-shipped");
        fs::create_dir_all(system.join("theme")).unwrap();
        fs::create_dir_all(shipped.join("theme")).unwrap();
        // Shipped default, system override, then a user override.
        fs::write(
            shipped.join("theme/tokens.toml"),
            "[theme]\nappearance = \"light\"\neffects-tier = \"minimal\"\n",
        )
        .unwrap();
        fs::write(
            system.join("theme/tokens.toml"),
            "[theme]\nappearance = \"dark\"\n",
        )
        .unwrap();

        let store = SettingsStore::with_layers(&user, vec![system.clone(), shipped.clone()]);
        assert_eq!(
            store.load("theme.appearance").unwrap(),
            Some(toml::Value::String("dark".into())),
            "the system layer beats the shipped default"
        );
        assert_eq!(
            store.load("theme.effects-tier").unwrap(),
            Some(toml::Value::String("minimal".into())),
            "a key missing from the system layer falls back to the shipped one"
        );

        // Saving writes the user layer only and then wins over both.
        store
            .save("theme.appearance", toml::Value::String("auto".into()))
            .unwrap();
        assert!(user.join("theme/tokens.toml").exists());
        assert_eq!(
            fs::read_to_string(system.join("theme/tokens.toml")).unwrap(),
            "[theme]\nappearance = \"dark\"\n",
            "system files are never modified"
        );
        assert_eq!(
            store.load("theme.appearance").unwrap(),
            Some(toml::Value::String("auto".into()))
        );
        let _ = fs::remove_dir_all(user);
        let _ = fs::remove_dir_all(system);
        let _ = fs::remove_dir_all(shipped);
    }

    #[test]
    fn invalid_file_is_never_overwritten() {
        let root = temp_root("invalid");
        let path = root.join("theme/tokens.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "this is not toml = = =").unwrap();
        let store = SettingsStore::new(&root);
        assert!(store
            .save("theme.appearance", toml::Value::String("light".into()))
            .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "this is not toml = = =");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_key_falls_back_to_the_schema_default() {
        let root = temp_root("defaults");
        let store = SettingsStore::new(&root);
        let rows = store.rows_for(Category::Appearance);
        let appearance = rows.iter().find(|row| row.key == "theme.appearance").unwrap();
        assert_eq!(appearance.value, "dark");
        let reduce = rows
            .iter()
            .find(|row| row.key == "theme.reduce-transparency")
            .unwrap();
        assert!(!reduce.on);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn toggle_flips_and_persists() {
        let root = temp_root("toggle");
        let store = SettingsStore::new(&root);
        assert!(!store.toggle("power.suspend-on-lid").unwrap()); // default true -> false
        assert_eq!(
            store.load("power.suspend-on-lid").unwrap(),
            Some(toml::Value::Boolean(false))
        );
        assert!(store.toggle("power.suspend-on-lid").unwrap()); // false -> true
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rows_expose_the_file_key_mapping() {
        for category in Category::ALL {
            let root = temp_root("rows");
            let store = SettingsStore::new(&root);
            for row in store.rows_for(category) {
                let def = find_setting(&row.key).unwrap();
                assert_eq!(row.file, def.file);
                assert_eq!(row.kind, def.kind.as_str());
            }
            let _ = fs::remove_dir_all(root);
        }
    }
}
