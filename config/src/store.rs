//! Layered configuration store (P7-T01, F4 sections 1–4, 6 and 11).
//!
//! Layers: `/usr/share/aether/defaults`, `/etc/aether`, `~/.config/aether`.
//! Later layers win. An invalid layer is reported and skipped, so the last
//! valid configuration stays active (CUS-AC-02).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::error::ConfigError;
use crate::schema::{schema_files, spec_for, KeySpec, SCHEMA};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Default,
    System,
    User,
}

impl Layer {
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::System => "system",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Roots {
    pub defaults: PathBuf,
    pub system: PathBuf,
    pub user: PathBuf,
}

impl Roots {
    pub fn for_user() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/home/user"));
        Self {
            defaults: PathBuf::from("/usr/share/aether/defaults"),
            system: PathBuf::from("/etc/aether"),
            user: std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("aether"),
        }
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            defaults: root.join("defaults"),
            system: root.join("system"),
            user: root.join("user"),
        }
    }

    pub fn path(&self, layer: Layer) -> &Path {
        match layer {
            Layer::Default => &self.defaults,
            Layer::System => &self.system,
            Layer::User => &self.user,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MergedValue {
    pub value: toml::Value,
    pub layer: Layer,
    pub source: PathBuf,
}

#[derive(Debug)]
pub struct ConfigStore {
    pub roots: Roots,
    values: BTreeMap<String, MergedValue>,
    pub errors: Vec<ConfigError>,
    /// Raw merged tables per file, for the shell to read lists/maps.
    pub tables: BTreeMap<String, toml::Table>,
}

impl ConfigStore {
    pub fn load(roots: Roots) -> Self {
        let mut store = Self {
            roots,
            values: BTreeMap::new(),
            errors: Vec::new(),
            tables: BTreeMap::new(),
        };
        store.reload();
        store
    }

    pub fn reload(&mut self) {
        self.values.clear();
        self.errors.clear();
        self.tables.clear();

        for file in schema_files() {
            let mut merged = toml::Table::new();
            for layer in [Layer::Default, Layer::System, Layer::User] {
                let path = self.roots.path(layer).join(file);
                if !path.exists() {
                    continue;
                }
                match fs::read_to_string(&path) {
                    Ok(text) => match toml::from_str::<toml::Table>(&text) {
                        Ok(table) => deep_merge(&mut merged, table),
                        Err(error) => {
                            // Keep the previous layers active (CUS-AC-02).
                            self.errors.push(parse_error(&path, &error));
                        }
                    },
                    Err(source) => self.errors.push(ConfigError::Io { path, source }),
                }
            }
            self.tables.insert(file.to_string(), merged);
        }

        for spec in SCHEMA {
            match self.lookup(spec) {
                Ok(Some(found)) => {
                    self.values.insert(spec.key.to_string(), found);
                }
                Ok(None) => {
                    // Default from the schema, attributed to the defaults layer.
                    if let Ok(value) = spec.default_value() {
                        self.values.insert(
                            spec.key.to_string(),
                            MergedValue {
                                value,
                                layer: Layer::Default,
                                source: self.roots.defaults.join(spec.file),
                            },
                        );
                    }
                }
                Err(error) => self.errors.push(error),
            }
        }
    }

    fn lookup(&self, spec: &KeySpec) -> Result<Option<MergedValue>, ConfigError> {
        let Some(table) = self.tables.get(spec.file) else {
            return Ok(None);
        };
        let Some(value) = table.get(spec.toml_key) else {
            return Ok(None);
        };
        // Determine which layer provided the effective value (the highest
        // layer that contains the key).
        let mut layer = Layer::Default;
        for candidate in [Layer::Default, Layer::System, Layer::User] {
            let path = self.roots.path(candidate).join(spec.file);
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(user_table) = toml::from_str::<toml::Table>(&text) {
                    if user_table.get(spec.toml_key).is_some() {
                        layer = candidate;
                    }
                }
            }
        }
        let value = value.clone();
        spec.validate(&value)?;
        Ok(Some(MergedValue {
            value,
            layer,
            source: self.roots.path(layer).join(spec.file),
        }))
    }

    pub fn get(&self, key: &str) -> Result<&toml::Value, ConfigError> {
        spec_for(key).ok_or_else(|| ConfigError::UnknownKey(key.to_string()))?;
        self.values
            .get(key)
            .map(|value| &value.value)
            .ok_or_else(|| ConfigError::UnknownKey(key.to_string()))
    }

    pub fn get_string(&self, key: &str) -> Result<String, ConfigError> {
        self.get(key).map(|value| match value {
            toml::Value::String(text) => text.clone(),
            other => other.to_string(),
        })
    }

    pub fn get_i64(&self, key: &str) -> Result<i64, ConfigError> {
        self.get(key)?
            .as_integer()
            .ok_or_else(|| ConfigError::InvalidValue {
                key: key.into(),
                expected: "an integer".into(),
                actual: self.get(key).map(|v| v.to_string()).unwrap_or_default(),
            })
    }

    pub fn get_f64(&self, key: &str) -> Result<f64, ConfigError> {
        let value = self.get(key)?;
        value
            .as_float()
            .or_else(|| value.as_integer().map(|v| v as f64))
            .ok_or_else(|| ConfigError::InvalidValue {
                key: key.into(),
                expected: "a number".into(),
                actual: value.to_string(),
            })
    }

    pub fn get_bool(&self, key: &str) -> Result<bool, ConfigError> {
        self.get(key)?
            .as_bool()
            .ok_or_else(|| ConfigError::InvalidValue {
                key: key.into(),
                expected: "true or false".into(),
                actual: self.get(key).map(|v| v.to_string()).unwrap_or_default(),
            })
    }

    pub fn source(&self, key: &str) -> Option<(&Layer, &Path)> {
        self.values.get(key).map(|value| (&value.layer, value.source.as_path()))
    }

    pub fn effective(&self) -> &BTreeMap<String, MergedValue> {
        &self.values
    }

    /// Set a value in the user layer, then reload. Invalid values are rejected
    /// before anything is written.
    pub fn set(&mut self, key: &str, raw: &str) -> Result<(), ConfigError> {
        let spec = spec_for(key).ok_or_else(|| ConfigError::UnknownKey(key.to_string()))?;
        let value = parse_typed(spec, raw)?;
        spec.validate(&value)?;

        let path = self.roots.user.join(spec.file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|source| ConfigError::Io { path: parent.to_path_buf(), source })?;
        }
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut doc = existing
            .parse::<DocumentMut>()
            .map_err(|error| ConfigError::Parse {
                path: path.clone(),
                message: error.to_string(),
                line: None,
                column: None,
            })?;
        let item = toml_value_to_item(&value);
        doc[spec.toml_key] = item;
        fs::write(&path, doc.to_string()).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        self.reload();
        if let Some(error) = self.errors.first() {
            return Err(ConfigError::Schema(error.to_string()));
        }
        Ok(())
    }

    /// Remove one key from the user layer (falls back to system/default).
    pub fn reset_key(&mut self, key: &str) -> Result<bool, ConfigError> {
        let spec = spec_for(key).ok_or_else(|| ConfigError::UnknownKey(key.to_string()))?;
        let path = self.roots.user.join(spec.file);
        if !path.exists() {
            return Ok(false);
        }
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        let mut doc = text.parse::<DocumentMut>().map_err(|error| ConfigError::Parse {
            path: path.clone(),
            message: error.to_string(),
            line: None,
            column: None,
        })?;
        let removed = doc.remove(spec.toml_key).is_some();
        if removed {
            fs::write(&path, doc.to_string()).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            })?;
        }
        self.reload();
        Ok(removed)
    }

    /// Reset a whole file by deleting the user override.
    pub fn reset_file(&mut self, file: &str) -> Result<bool, ConfigError> {
        if !schema_files().contains(&file) {
            return Err(ConfigError::Schema(format!("unknown file {file}")));
        }
        let path = self.roots.user.join(file);
        let removed = if path.exists() {
            fs::remove_file(&path).map_err(|source| ConfigError::Io {
                path: path.clone(),
                source,
            })?;
            true
        } else {
            false
        };
        self.reload();
        Ok(removed)
    }

    pub fn reset_all(&mut self) -> Result<usize, ConfigError> {
        let mut removed = 0;
        for file in schema_files() {
            if self.reset_file(file)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Validate every file in every layer and every schema value.
    pub fn check(&self) -> Vec<ConfigError> {
        let mut errors = Vec::new();
        for file in schema_files() {
            for layer in [Layer::Default, Layer::System, Layer::User] {
                let path = self.roots.path(layer).join(file);
                if !path.exists() {
                    continue;
                }
                match fs::read_to_string(&path) {
                    Ok(text) => {
                        if let Err(error) = toml::from_str::<toml::Table>(&text) {
                            errors.push(parse_error(&path, &error));
                        }
                    }
                    Err(source) => errors.push(ConfigError::Io { path, source }),
                }
            }
        }
        errors.extend(self.errors.iter().map(clone_error));
        for (key, merged) in &self.values {
            if let Some(spec) = spec_for(key) {
                if let Err(error) = spec.validate(&merged.value) {
                    errors.push(error);
                }
            }
        }
        errors
    }
}

fn clone_error(error: &ConfigError) -> ConfigError {
    match error {
        ConfigError::Io { path, source } => ConfigError::Io {
            path: path.clone(),
            source: std::io::Error::new(source.kind(), source.to_string()),
        },
        ConfigError::Parse {
            path,
            message,
            line,
            column,
        } => ConfigError::Parse {
            path: path.clone(),
            message: message.clone(),
            line: *line,
            column: *column,
        },
        ConfigError::UnknownKey(key) => ConfigError::UnknownKey(key.clone()),
        ConfigError::InvalidValue {
            key,
            expected,
            actual,
        } => ConfigError::InvalidValue {
            key: key.clone(),
            expected: expected.clone(),
            actual: actual.clone(),
        },
        ConfigError::Schema(message) => ConfigError::Schema(message.clone()),
        ConfigError::Hook(error) => ConfigError::Hook(error.clone()),
        ConfigError::Features(message) => ConfigError::Features(message.clone()),
    }
}

fn parse_error(path: &Path, error: &toml::de::Error) -> ConfigError {
    let message = error.to_string();
    // toml errors include "at line X column Y" in the message.
    let mut line = None;
    let mut column = None;
    if let Some(index) = message.find("line ") {
        let rest = &message[index + 5..];
        let mut parts = rest.split_whitespace();
        line = parts.next().and_then(|value| value.parse().ok());
        if let Some(next) = parts.next() {
            if next == "column" {
                column = parts.next().and_then(|value| value.parse().ok());
            }
        }
    }
    ConfigError::Parse {
        path: path.to_path_buf(),
        message,
        line,
        column,
    }
}

fn deep_merge(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(new)) => {
                deep_merge(existing, new);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Parse a CLI value according to its schema kind.
pub fn parse_typed(spec: &KeySpec, raw: &str) -> Result<toml::Value, ConfigError> {
    use crate::schema::ValueKind;
    let invalid = || ConfigError::InvalidValue {
        key: spec.key.to_string(),
        expected: spec.kind.describe(),
        actual: raw.to_string(),
    };
    match spec.kind {
        ValueKind::Bool => match raw {
            "true" => Ok(toml::Value::Boolean(true)),
            "false" => Ok(toml::Value::Boolean(false)),
            _ => Err(invalid()),
        },
        ValueKind::Integer => raw.parse::<i64>().map(toml::Value::Integer).map_err(|_| invalid()),
        ValueKind::Float => raw
            .parse::<f64>()
            .map(toml::Value::Float)
            .map_err(|_| invalid()),
        ValueKind::String | ValueKind::Color | ValueKind::Enum(_) => {
            Ok(toml::Value::String(raw.trim_matches('"').to_string()))
        }
        ValueKind::StringList => {
            let text = if raw.trim_start().starts_with('[') {
                raw.to_string()
            } else {
                format!(
                    "[{}]",
                    raw.split(',')
                        .map(|item| format!("\"{}\"", item.trim()))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let wrapped = format!("v = {text}");
            let value = toml::from_str::<toml::Value>(&wrapped)
                .ok()
                .and_then(|value| value.get("v").cloned())
                .ok_or_else(invalid)?;
            value.as_array().ok_or_else(invalid)?;
            Ok(value)
        }
    }
}

fn toml_value_to_item(value: &toml::Value) -> Item {
    match value {
        toml::Value::String(text) => Item::Value(Value::from(text.clone())),
        toml::Value::Integer(number) => Item::Value(Value::from(*number)),
        toml::Value::Float(number) => Item::Value(Value::from(*number)),
        toml::Value::Boolean(flag) => Item::Value(Value::from(*flag)),
        toml::Value::Array(array) => {
            let mut new_array = toml_edit::Array::new();
            for item in array {
                if let toml::Value::String(text) = item {
                    new_array.push(text.clone());
                }
            }
            Item::Value(Value::Array(new_array))
        }
        other => {
            // Tables are not part of the MVP settings schema.
            let mut table = Table::new();
            let _ = &mut table;
            let _ = other;
            Item::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_roots(tag: &str) -> Roots {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-config-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Roots::at(dir)
    }

    fn write(base: &Path, file: &str, text: &str) {
        let path = base.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn defaults_come_from_the_schema() {
        let roots = temp_roots("defaults");
        let store = ConfigStore::load(roots);
        assert_eq!(store.get_string("dock.position").unwrap(), "bottom");
        assert_eq!(store.get_i64("dock.icon_size").unwrap(), 56);
        assert!(store.get_bool("dock.auto_hide").unwrap());
        assert!(store.errors.is_empty());
    }

    #[test]
    fn layers_override_and_last_layer_wins() {
        let roots = temp_roots("layers");
        write(&roots.system, "shell/dock.toml", "position = \"left\"\nicon_size = 64\n");
        write(&roots.user, "shell/dock.toml", "position = \"right\"\n");
        let store = ConfigStore::load(roots);
        assert_eq!(store.get_string("dock.position").unwrap(), "right");
        assert_eq!(store.get_i64("dock.icon_size").unwrap(), 64);
        assert_eq!(store.source("dock.position").unwrap().0, &Layer::User);
        assert_eq!(store.source("dock.icon_size").unwrap().0, &Layer::System);
    }

    #[test]
    fn set_get_roundtrip_preserves_other_keys_and_comments() {
        let roots = temp_roots("set");
        write(
            &roots.user,
            "shell/dock.toml",
            "# my dock\nposition = \"bottom\"\n",
        );
        let mut store = ConfigStore::load(roots.clone());
        store.set("dock.position", "left").unwrap();
        let text = fs::read_to_string(roots.user.join("shell/dock.toml")).unwrap();
        assert!(text.contains("# my dock"));
        assert!(text.contains("position = \"left\""));
        assert_eq!(store.get_string("dock.position").unwrap(), "left");
        assert!(store.check().is_empty());
    }

    #[test]
    fn invalid_value_is_rejected_without_writing() {
        let roots = temp_roots("invalid");
        let mut store = ConfigStore::load(roots.clone());
        let path = roots.user.join("shell/dock.toml");
        assert!(store.set("dock.icon_size", "9999").is_err());
        assert!(!path.exists(), "invalid value must not create a file");
        assert_eq!(store.get_i64("dock.icon_size").unwrap(), 56);
        assert!(store.set("dock.position", "middle").is_err());
    }

    #[test]
    fn invalid_file_is_reported_and_last_valid_value_stays() {
        let roots = temp_roots("broken");
        write(&roots.system, "shell/dock.toml", "position = \"left\"\n");
        write(&roots.user, "shell/dock.toml", "position = ???\n");
        let store = ConfigStore::load(roots);
        assert_eq!(store.get_string("dock.position").unwrap(), "left");
        assert!(!store.errors.is_empty());
        let error = format!("{}", store.errors[0]);
        assert!(error.contains("dock.toml"));
    }

    #[test]
    fn reset_key_and_file_fall_back_to_defaults() {
        let roots = temp_roots("reset");
        write(&roots.user, "shell/dock.toml", "position = \"left\"\nicon_size = 80\n");
        let mut store = ConfigStore::load(roots.clone());
        assert!(store.reset_key("dock.icon_size").unwrap());
        assert_eq!(store.get_i64("dock.icon_size").unwrap(), 56);
        assert_eq!(store.get_string("dock.position").unwrap(), "left");
        assert!(store.reset_file("shell/dock.toml").unwrap());
        assert_eq!(store.get_string("dock.position").unwrap(), "bottom");
        assert!(!roots.user.join("shell/dock.toml").exists());
    }

    #[test]
    fn check_reports_every_layer() {
        let roots = temp_roots("check");
        write(&roots.system, "shell/dock.toml", "position = \"top\"\n");
        write(&roots.user, "theme/tokens.toml", "accent = \"blue\"\n");
        let store = ConfigStore::load(roots);
        let errors = store.check();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|error| {
            let text = format!("{error}");
            text.contains("theme/tokens.toml") || text.contains("theme.accent")
        }));
    }

    #[test]
    fn typed_parsing_handles_lists() {
        let spec = spec_for("dock.pinned").unwrap();
        let value = parse_typed(spec, "org.aether.text, org.aether.files").unwrap();
        assert_eq!(value.as_array().unwrap().len(), 2);
        let value = parse_typed(spec, "[\"a\", \"b\", \"c\"]").unwrap();
        assert_eq!(value.as_array().unwrap().len(), 3);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let roots = temp_roots("unknown");
        let mut store = ConfigStore::load(roots);
        assert!(matches!(
            store.get("dock.nope"),
            Err(ConfigError::UnknownKey(_))
        ));
        assert!(matches!(
            store.set("nope", "1"),
            Err(ConfigError::UnknownKey(_))
        ));
    }
}
