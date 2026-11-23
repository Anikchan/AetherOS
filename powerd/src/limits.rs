//! Screen Time limits and downtime (Pack E, P7-PK-T02, F14 ST-05/ST-06).
//!
//! Per-app daily limits with a one-shot extension, plus scheduled downtime
//! windows (which may cross midnight). Decisions are pure and keyed by day, so
//! usage resets at midnight without a timer.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MINUTES_PER_DAY: u32 = 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitRule {
    pub app: String,
    /// Daily allowance in minutes.
    pub daily_minutes: u32,
    /// Extra minutes granted by the user for today (one extension).
    pub extension_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DowntimeWindow {
    /// Minutes from midnight (0..1440).
    pub start_minute: u32,
    pub end_minute: u32,
    /// Empty means every app.
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default = "default_label")]
    pub label: String,
}

fn default_label() -> String {
    "Downtime".into()
}

impl DowntimeWindow {
    /// Handles windows that cross midnight (for example 22:00–07:00).
    pub fn contains(&self, minute_of_day: u32) -> bool {
        let minute = minute_of_day % MINUTES_PER_DAY;
        if self.start_minute == self.end_minute {
            false
        } else if self.start_minute < self.end_minute {
            minute >= self.start_minute && minute < self.end_minute
        } else {
            minute >= self.start_minute || minute < self.end_minute
        }
    }

    pub fn covers(&self, app: &str) -> bool {
        self.apps.is_empty() || self.apps.iter().any(|candidate| candidate == app)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsConfig {
    #[serde(default)]
    pub rules: Vec<LimitRule>,
    #[serde(default)]
    pub downtime: Vec<DowntimeWindow>,
    /// Warn when this share of the allowance is used (default 90%).
    #[serde(default = "default_warn")]
    pub warn_percent: u8,
}

fn default_warn() -> u8 {
    90
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsState {
    /// day (epoch days) -> app -> used seconds
    #[serde(default)]
    pub used_seconds: BTreeMap<i64, BTreeMap<String, u64>>,
    /// day -> app -> extension already granted today (0/1)
    #[serde(default)]
    pub extensions_granted: BTreeMap<i64, BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockDecision {
    Allowed,
    Warn { used_seconds: u64, limit_seconds: u64 },
    BlockedLimit { used_seconds: u64, limit_seconds: u64 },
    BlockedDowntime { label: String },
}

impl BlockDecision {
    pub fn blocked(&self) -> bool {
        matches!(
            self,
            Self::BlockedLimit { .. } | Self::BlockedDowntime { .. }
        )
    }
}

impl LimitsConfig {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, text)
    }

    pub fn rule(&self, app: &str) -> Option<&LimitRule> {
        self.rules.iter().find(|rule| rule.app == app)
    }
}

impl LimitsState {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, text)
    }

    pub fn used(&self, day: i64, app: &str) -> u64 {
        self.used_seconds
            .get(&day)
            .and_then(|apps| apps.get(app))
            .copied()
            .unwrap_or(0)
    }

    pub fn record(&mut self, day: i64, app: &str, seconds: u64) {
        *self
            .used_seconds
            .entry(day)
            .or_default()
            .entry(app.to_string())
            .or_insert(0) += seconds;
    }

    pub fn grant_extension(&mut self, day: i64, app: &str) {
        self.extensions_granted
            .entry(day)
            .or_default()
            .insert(app.to_string(), true);
    }

    pub fn extension_granted(&self, day: i64, app: &str) -> bool {
        self.extensions_granted
            .get(&day)
            .and_then(|apps| apps.get(app))
            .copied()
            .unwrap_or(false)
    }
}

/// Decide whether an app may be focused at `minute_of_day` on `day`.
pub fn decide(
    config: &LimitsConfig,
    state: &LimitsState,
    app: &str,
    day: i64,
    minute_of_day: u32,
) -> BlockDecision {
    for window in &config.downtime {
        if window.covers(app) && window.contains(minute_of_day) {
            return BlockDecision::BlockedDowntime {
                label: window.label.clone(),
            };
        }
    }
    let Some(rule) = config.rule(app) else {
        return BlockDecision::Allowed;
    };
    let extension = if state.extension_granted(day, app) {
        u64::from(rule.extension_minutes) * 60
    } else {
        0
    };
    let limit_seconds = u64::from(rule.daily_minutes) * 60 + extension;
    let used_seconds = state.used(day, app);
    if used_seconds >= limit_seconds {
        return BlockDecision::BlockedLimit {
            used_seconds,
            limit_seconds,
        };
    }
    let warn_at = limit_seconds * u64::from(config.warn_percent.min(100)) / 100;
    if used_seconds >= warn_at {
        return BlockDecision::Warn {
            used_seconds,
            limit_seconds,
        };
    }
    BlockDecision::Allowed
}

/// Remaining allowance in seconds (0 when blocked or unlimited).
pub fn remaining_seconds(
    config: &LimitsConfig,
    state: &LimitsState,
    app: &str,
    day: i64,
) -> Option<u64> {
    let rule = config.rule(app)?;
    let extension = if state.extension_granted(day, app) {
        u64::from(rule.extension_minutes) * 60
    } else {
        0
    };
    let limit = u64::from(rule.daily_minutes) * 60 + extension;
    Some(limit.saturating_sub(state.used(day, app)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LimitsConfig {
        LimitsConfig {
            rules: vec![LimitRule {
                app: "org.example.Chat".into(),
                daily_minutes: 30,
                extension_minutes: 15,
            }],
            downtime: vec![DowntimeWindow {
                start_minute: 22 * 60,
                end_minute: 7 * 60,
                apps: Vec::new(),
                label: "Night".into(),
            }],
            warn_percent: 90,
        }
    }

    #[test]
    fn daily_limit_blocks_and_extension_unblocks() {
        let config = config();
        let mut state = LimitsState::default();
        state.record(100, "org.example.Chat", 29 * 60);
        assert_eq!(
            decide(&config, &state, "org.example.Chat", 100, 12 * 60),
            BlockDecision::Warn {
                used_seconds: 29 * 60,
                limit_seconds: 30 * 60
            }
        );
        state.record(100, "org.example.Chat", 60);
        assert!(decide(&config, &state, "org.example.Chat", 100, 12 * 60).blocked());

        state.grant_extension(100, "org.example.Chat");
        assert!(!decide(&config, &state, "org.example.Chat", 100, 12 * 60).blocked());
        assert_eq!(
            remaining_seconds(&config, &state, "org.example.Chat", 100),
            Some(15 * 60)
        );
    }

    #[test]
    fn usage_resets_on_a_new_day() {
        let config = config();
        let mut state = LimitsState::default();
        state.record(100, "org.example.Chat", 30 * 60);
        assert!(decide(&config, &state, "org.example.Chat", 100, 12 * 60).blocked());
        assert_eq!(
            decide(&config, &state, "org.example.Chat", 101, 12 * 60),
            BlockDecision::Allowed
        );
    }

    #[test]
    fn downtime_crossing_midnight_blocks_every_app() {
        let config = config();
        let state = LimitsState::default();
        assert!(matches!(
            decide(&config, &state, "org.example.Other", 100, 23 * 60),
            BlockDecision::BlockedDowntime { .. }
        ));
        assert!(matches!(
            decide(&config, &state, "org.example.Other", 100, 2 * 60),
            BlockDecision::BlockedDowntime { .. }
        ));
        assert_eq!(
            decide(&config, &state, "org.example.Other", 100, 12 * 60),
            BlockDecision::Allowed
        );
    }

    #[test]
    fn downtime_can_target_specific_apps() {
        let mut config = config();
        config.downtime[0].apps = vec!["org.example.Game".into()];
        let state = LimitsState::default();
        assert!(decide(&config, &state, "org.example.Game", 1, 23 * 60).blocked());
        assert!(!decide(&config, &state, "org.example.Chat", 1, 23 * 60).blocked());
    }

    #[test]
    fn config_and_state_round_trip() {
        let dir = std::env::temp_dir().join(format!("aether-limits-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("limits.toml");
        let state_path = dir.join("state.json");
        config().save(&config_path).unwrap();
        let loaded = LimitsConfig::load(&config_path).unwrap();
        assert_eq!(loaded, config());

        let mut state = LimitsState::default();
        state.record(5, "org.example.Chat", 120);
        state.grant_extension(5, "org.example.Chat");
        state.save(&state_path).unwrap();
        let loaded = LimitsState::load(&state_path).unwrap();
        assert_eq!(loaded.used(5, "org.example.Chat"), 120);
        assert!(loaded.extension_granted(5, "org.example.Chat"));
        let _ = fs::remove_dir_all(dir);
    }
}
