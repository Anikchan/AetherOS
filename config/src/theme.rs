//! Theme token engine with live reload (P7-T02, F4 CUS-03/04, D1 section 5).
//!
//! Tokens live in `theme/tokens.toml`. Invalid changes are rejected and the
//! last valid token set stays active (CUS-AC-02); a valid change is reported
//! so the shell can re-render without a restart (CUS-AC-07).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::schema::{is_hex_color, spec_for};
use crate::store::ConfigStore;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeTokens {
    pub accent: String,
    pub background: String,
    pub foreground: String,
    pub radius: i64,
    pub blur: i64,
    pub tint: f64,
    pub font: String,
    pub spacing: i64,
}

impl ThemeTokens {
    pub fn from_store(store: &ConfigStore) -> Result<Self, ConfigError> {
        Ok(Self {
            accent: store.get_string("theme.accent")?,
            background: store.get_string("theme.background")?,
            foreground: store.get_string("theme.foreground")?,
            radius: store.get_i64("theme.radius")?,
            blur: store.get_i64("theme.blur")?,
            tint: store.get_f64("theme.tint")?,
            font: store.get_string("theme.font")?,
            spacing: store.get_i64("theme.spacing")?,
        })
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        for (key, value) in [
            ("theme.accent", &self.accent),
            ("theme.background", &self.background),
            ("theme.foreground", &self.foreground),
        ] {
            if !is_hex_color(value) {
                return Err(ConfigError::InvalidValue {
                    key: key.into(),
                    expected: "a color such as \"#3b82f6\"".into(),
                    actual: value.clone(),
                });
            }
        }
        if !(0..=40).contains(&self.radius) {
            return Err(ConfigError::InvalidValue {
                key: "theme.radius".into(),
                expected: "an integer between 0 and 40".into(),
                actual: self.radius.to_string(),
            });
        }
        if !(0.0..=1.0).contains(&self.tint) {
            return Err(ConfigError::InvalidValue {
                key: "theme.tint".into(),
                expected: "a number between 0 and 1".into(),
                actual: self.tint.to_string(),
            });
        }
        Ok(())
    }

    /// Flat key/value map for the shell's renderer.
    pub fn as_map(&self) -> Vec<(&'static str, String)> {
        vec![
            ("accent", self.accent.clone()),
            ("background", self.background.clone()),
            ("foreground", self.foreground.clone()),
            ("radius", self.radius.to_string()),
            ("blur", self.blur.to_string()),
            ("tint", self.tint.to_string()),
            ("font", self.font.clone()),
            ("spacing", self.spacing.to_string()),
        ]
    }
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self {
            accent: "#3b82f6".into(),
            background: "#0f1115".into(),
            foreground: "#e8eaed".into(),
            radius: 12,
            blur: 16,
            tint: 0.35,
            font: "Inter".into(),
            spacing: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeReload {
    pub changed: bool,
    pub tokens: Option<ThemeTokens>,
    pub error: Option<String>,
}

/// Watches one tokens file and keeps the last valid token set.
#[derive(Debug)]
pub struct ThemeEngine {
    path: PathBuf,
    last_modified: Option<SystemTime>,
    tokens: ThemeTokens,
}

impl ThemeEngine {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            last_modified: None,
            tokens: ThemeTokens::default(),
        }
    }

    pub fn tokens(&self) -> &ThemeTokens {
        &self.tokens
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mtime(path: &Path) -> Option<SystemTime> {
        fs::metadata(path).ok().and_then(|meta| meta.modified().ok())
    }

    /// Read tokens now; invalid input keeps the current tokens.
    pub fn load(&mut self) -> ThemeReload {
        let modified = Self::mtime(&self.path);
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) => {
                // A missing file is not an error: defaults stay active.
                if error.kind() == std::io::ErrorKind::NotFound {
                    self.last_modified = modified;
                    return ThemeReload {
                        changed: false,
                        tokens: Some(self.tokens.clone()),
                        error: None,
                    };
                }
                return ThemeReload {
                    changed: false,
                    tokens: Some(self.tokens.clone()),
                    error: Some(error.to_string()),
                };
            }
        };
        match tokens_from_toml(&text) {
            Ok(tokens) => {
                let changed = tokens != self.tokens;
                self.tokens = tokens.clone();
                self.last_modified = modified;
                ThemeReload {
                    changed,
                    tokens: Some(tokens),
                    error: None,
                }
            }
            Err(error) => ThemeReload {
                changed: false,
                tokens: Some(self.tokens.clone()),
                error: Some(error.to_string()),
            },
        }
    }

    /// Reload only when the file timestamp changed (CUS-AC-01/CUS-AC-07).
    pub fn reload_if_changed(&mut self) -> ThemeReload {
        let modified = Self::mtime(&self.path);
        if modified == self.last_modified {
            return ThemeReload {
                changed: false,
                tokens: Some(self.tokens.clone()),
                error: None,
            };
        }
        self.load()
    }
}

/// Parse tokens from TOML text using schema defaults for missing values.
pub fn tokens_from_toml(text: &str) -> Result<ThemeTokens, ConfigError> {
    let table: toml::Table = toml::from_str(text).map_err(|error| ConfigError::Schema(error.to_string()))?;
    let mut tokens = ThemeTokens::default();
    let take_string = |key: &str, target: &mut String| -> Result<(), ConfigError> {
        if let Some(value) = table.get(key) {
            let Some(text) = value.as_str() else {
                return Err(ConfigError::InvalidValue {
                    key: format!("theme.{key}"),
                    expected: "text".into(),
                    actual: value.to_string(),
                });
            };
            *target = text.to_string();
        }
        Ok(())
    };
    take_string("accent", &mut tokens.accent)?;
    take_string("background", &mut tokens.background)?;
    take_string("foreground", &mut tokens.foreground)?;
    take_string("font", &mut tokens.font)?;
    if let Some(value) = table.get("radius").and_then(|v| v.as_integer()) {
        tokens.radius = value;
    }
    if let Some(value) = table.get("blur").and_then(|v| v.as_integer()) {
        tokens.blur = value;
    }
    if let Some(value) = table.get("spacing").and_then(|v| v.as_integer()) {
        tokens.spacing = value;
    }
    if let Some(value) = table
        .get("tint")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
    {
        tokens.tint = value;
    }
    tokens.validate()?;
    // Cross-check with the schema so the two definitions cannot drift.
    for (key, _) in tokens.as_map() {
        let dotted = format!("theme.{key}");
        debug_assert!(spec_for(&dotted).is_some(), "missing schema key {dotted}");
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Roots;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_file(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-theme-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("tokens.toml")
    }

    #[test]
    fn tokens_come_from_the_config_store() {
        let roots = Roots::at(std::env::temp_dir().join(format!(
            "aether-theme-store-{}",
            std::process::id()
        )));
        let store = ConfigStore::load(roots);
        let tokens = ThemeTokens::from_store(&store).unwrap();
        assert_eq!(tokens.accent, "#3b82f6");
        assert_eq!(tokens.radius, 12);
        tokens.validate().unwrap();
    }

    #[test]
    fn reload_detects_a_valid_change() {
        let path = temp_file("reload");
        fs::write(&path, "accent = \"#ff0000\"\n").unwrap();
        let mut engine = ThemeEngine::new(&path);
        let first = engine.load();
        assert!(first.changed);
        assert_eq!(engine.tokens().accent, "#ff0000");

        let unchanged = engine.reload_if_changed();
        assert!(!unchanged.changed);

        // Ensure a distinct mtime on coarse filesystems.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "accent = \"#00ff00\"\nradius = 20\n").unwrap();
        let changed = engine.reload_if_changed();
        assert!(changed.changed);
        assert_eq!(engine.tokens().accent, "#00ff00");
        assert_eq!(engine.tokens().radius, 20);
    }

    #[test]
    fn invalid_change_keeps_the_last_valid_tokens() {
        let path = temp_file("invalid");
        fs::write(&path, "accent = \"#ff0000\"\n").unwrap();
        let mut engine = ThemeEngine::new(&path);
        engine.load();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "accent = \"blue\"\n").unwrap();
        let reload = engine.reload_if_changed();
        assert!(reload.error.is_some());
        assert_eq!(engine.tokens().accent, "#ff0000");
    }

    #[test]
    fn map_is_complete() {
        let tokens = ThemeTokens::default();
        let map = tokens.as_map();
        assert_eq!(map.len(), 8);
        assert!(map.iter().any(|(key, _)| *key == "blur"));
    }
}
