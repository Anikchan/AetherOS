//! Keybinding file and conflict detection (P7-T06, F4 CUS-05).
//!
//! Format (`input/keybindings.toml`):
//!
//! ```toml
//! [bindings]
//! "Super+Escape" = "taskmanager"
//! "Super+Space" = "launcher"
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BindingError {
    #[error("invalid shortcut {0:?}")]
    InvalidKey(String),
    #[error("shortcut {key} is bound to both {first} and {second}")]
    Conflict {
        key: String,
        first: String,
        second: String,
    },
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml error: {0}")]
    Toml(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// Normalized shortcut, for example `super+escape`.
    pub key: String,
    pub action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingSet {
    pub bindings: Vec<Binding>,
    pub conflicts: Vec<String>,
}

impl BindingSet {
    pub fn defaults() -> Self {
        let defaults = [
            ("Super+Escape", "taskmanager"),
            ("Super+Space", "launcher"),
            ("Super+Tab", "overview"),
            ("Super+L", "lockscreen"),
            ("Super+E", "files"),
            ("Ctrl+Alt+T", "terminal"),
            ("Print", "screenshot"),
        ];
        let mut bindings: Vec<Binding> = defaults
            .iter()
            .map(|(key, action)| Binding {
                key: normalize_key(key).unwrap_or_default(),
                action: (*action).to_string(),
            })
            .collect();
        bindings.sort_by(|a, b| a.key.cmp(&b.key));
        Self {
            bindings,
            conflicts: Vec::new(),
        }
    }

    pub fn from_toml(text: &str) -> Result<Self, BindingError> {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            bindings: BTreeMap<String, String>,
        }
        let file: File = toml::from_str(text).map_err(|e| BindingError::Toml(e.to_string()))?;
        Self::from_pairs(file.bindings.into_iter().collect())
    }

    pub fn from_pairs(pairs: Vec<(String, String)>) -> Result<Self, BindingError> {
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        let mut conflicts = Vec::new();
        for (key, action) in pairs {
            let normalized = normalize_key(&key).ok_or_else(|| BindingError::InvalidKey(key.clone()))?;
            if let Some(existing) = map.get(&normalized) {
                if existing != &action {
                    conflicts.push(format!("{key}: {existing} vs {action}"));
                    continue;
                }
            }
            map.insert(normalized, action);
        }
        Ok(Self {
            bindings: map
                .into_iter()
                .map(|(key, action)| Binding { key, action })
                .collect(),
            conflicts,
        })
    }

    pub fn load(path: &Path) -> Result<Self, BindingError> {
        if !path.exists() {
            return Ok(Self::defaults());
        }
        Self::from_toml(&fs::read_to_string(path)?)
    }

    /// Merge the defaults with a user file: user bindings win, unbound
    /// defaults remain available.
    pub fn merge_with_defaults(mut self) -> Self {
        let defaults = Self::defaults();
        let mut map: BTreeMap<String, String> = defaults
            .bindings
            .into_iter()
            .map(|binding| (binding.key, binding.action))
            .collect();
        for binding in self.bindings {
            map.insert(binding.key, binding.action);
        }
        let mut bindings: Vec<Binding> = map
            .into_iter()
            .map(|(key, action)| Binding { key, action })
            .collect();
        bindings.sort_by(|a, b| a.key.cmp(&b.key));
        self.bindings = bindings;
        self
    }

    pub fn action_for(&self, key: &str) -> Option<&str> {
        let normalized = normalize_key(key)?;
        self.bindings
            .iter()
            .find(|binding| binding.key == normalized)
            .map(|binding| binding.action.as_str())
    }

    pub fn keys_for(&self, action: &str) -> Vec<&str> {
        self.bindings
            .iter()
            .filter(|binding| binding.action == action)
            .map(|binding| binding.key.as_str())
            .collect()
    }
}

const MODIFIERS: &[&str] = &[
    "super", "ctrl", "control", "alt", "shift", "meta", "hyper",
];

/// Normalize to a canonical lowercase form with sorted modifiers.
pub fn normalize_key(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let mut modifiers: Vec<String> = Vec::new();
    let mut key = String::new();
    let parts: Vec<&str> = input.split('+').map(|part| part.trim()).collect();
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return None;
        }
        let lower = part.to_ascii_lowercase();
        let canonical = match lower.as_str() {
            "control" => "ctrl".to_string(),
            "meta" | "hyper" => "super".to_string(),
            other => other.to_string(),
        };
        if index + 1 < parts.len() {
            if !MODIFIERS.contains(&lower.as_str()) {
                return None;
            }
            modifiers.push(canonical);
        } else {
            if MODIFIERS.contains(&lower.as_str()) {
                return None;
            }
            key = canonical;
        }
    }
    if key.is_empty() {
        return None;
    }
    modifiers.sort();
    modifiers.dedup();
    if modifiers.is_empty() {
        Some(key)
    } else {
        Some(format!("{}+{}", modifiers.join("+"), key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_the_windows_shortcut() {
        let set = BindingSet::defaults();
        assert_eq!(set.action_for("Super+Escape"), Some("taskmanager"));
        assert_eq!(set.action_for("ctrl+alt+t"), Some("terminal"));
        assert_eq!(set.action_for("Ctrl+Alt+T"), Some("terminal"));
        assert_eq!(set.keys_for("launcher"), vec!["super+space"]);
    }

    #[test]
    fn parsing_is_normalized_and_sorted() {
        let set = BindingSet::from_toml(
            "[bindings]\n\"Shift+Super+X\" = \"custom\"\n\"Super+L\" = \"lock\"\n",
        )
        .unwrap();
        assert_eq!(set.action_for("super+shift+x"), Some("custom"));
        assert_eq!(set.bindings[0].key, "shift+super+x");
    }

    #[test]
    fn duplicate_keys_are_reported() {
        let set = BindingSet::from_toml(
            "[bindings]\n\"Super+E\" = \"files\"\n\"super+e\" = \"editor\"\n",
        )
        .unwrap();
        assert_eq!(set.conflicts.len(), 1);
        assert_eq!(set.action_for("Super+E"), Some("files"));
    }

    #[test]
    fn invalid_shortcuts_are_rejected() {
        assert!(BindingSet::from_toml("[bindings]\n\"Super+Nope+Q\" = \"x\"\n").is_err());
        assert!(BindingSet::from_toml("[bindings]\n\"+\" = \"x\"\n").is_err());
        assert!(normalize_key("").is_none());
        assert!(normalize_key("Super+").is_none());
    }

    #[test]
    fn user_bindings_merge_over_defaults() {
        let user = BindingSet::from_toml("[bindings]\n\"Super+E\" = \"my-files\"\n").unwrap();
        let merged = user.merge_with_defaults();
        assert_eq!(merged.action_for("Super+E"), Some("my-files"));
        assert_eq!(merged.action_for("Super+Escape"), Some("taskmanager"));
    }
}
