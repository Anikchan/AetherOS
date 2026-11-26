//! Accessibility settings (F1 SHL-18, design D1 sections 8 and 9).
//!
//! Three switches are resolved here and pushed into the Slint surfaces:
//!
//! * `theme.reduce-motion` — springs and slides collapse to a 120 ms fade.
//! * `theme.reduce-transparency` — forces the Minimal glass tier (the override
//!   is owned by [`crate::effects`]; this module reads the same key so one
//!   summary covers the whole accessibility contract).
//! * `theme.text-scale` — 100 % to 200 %, clamped, applied to every font token,
//!   the menu bar height and the taller shell rows.
//!
//! The resolved values are also published as environment variables for apps
//! (`AETHER_SHELL_*`), mirroring `--effects-env`, and summarised as
//! `AETHER_SHELL_A11Y_*` markers for the on-device harness.
//!
//! The focus indicator contract (2 px accent outline) lives here too, because
//! D1 section 9 treats it as an accessibility requirement rather than a theme
//! detail.

use crate::config::SettingsStore;
use aether_ui::theme::Tier;

/// Reduce motion (springs and scaling become a fade, D1 section 8).
pub const SETTING_REDUCE_MOTION: &str = "theme.reduce-motion";
/// Text scaling as a percentage (100 % – 200 %, D1 section 9).
pub const SETTING_TEXT_SCALE: &str = "theme.text-scale";
/// Reduce transparency forces the Minimal tier.
pub const SETTING_REDUCE_TRANSPARENCY: &str = "theme.reduce-transparency";

/// Smallest and largest text scale the shell accepts.
pub const MIN_TEXT_SCALE: f32 = 1.0;
pub const MAX_TEXT_SCALE: f32 = 2.0;
/// Values the Appearance page cycles through (`theme.text-scale`).
pub const TEXT_SCALE_STEPS: [&str; 5] = ["100%", "125%", "150%", "175%", "200%"];

/// Every animation becomes this fade when motion is reduced.
pub const FADE_MS: u32 = 120;
/// Menu/panel motion when motion is allowed (D1 section 8 `motion.base`).
pub const BASE_MOTION_MS: u32 = 180;
/// Focus indicator outline width in pixels (D1 section 9).
pub const FOCUS_RING_PX: u32 = 2;

/// Resolved accessibility settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct A11ySettings {
    pub reduce_motion: bool,
    pub reduce_transparency: bool,
    /// 1.0 = 100 %, 2.0 = 200 %.
    pub text_scale: f32,
}

impl Default for A11ySettings {
    fn default() -> Self {
        A11ySettings {
            reduce_motion: false,
            reduce_transparency: false,
            text_scale: MIN_TEXT_SCALE,
        }
    }
}

impl A11ySettings {
    /// Read the three accessibility keys from the layered settings store.
    /// Unknown or unreadable values fall back to the defaults (motion on,
    /// transparency on, 100 %) instead of failing the shell.
    pub fn load(store: &SettingsStore) -> A11ySettings {
        let read_bool = |key: &str| {
            store
                .load(key)
                .ok()
                .flatten()
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        };
        let text_scale = store
            .load(SETTING_TEXT_SCALE)
            .ok()
            .flatten()
            .and_then(|value| parse_scale(&value))
            .unwrap_or(MIN_TEXT_SCALE);
        A11ySettings {
            reduce_motion: read_bool(SETTING_REDUCE_MOTION),
            reduce_transparency: read_bool(SETTING_REDUCE_TRANSPARENCY),
            text_scale: clamp_scale(text_scale),
        }
    }

    /// Duration for movement (a slide or a spring-like move).
    pub fn slide_ms(&self) -> u32 {
        if self.reduce_motion {
            0
        } else {
            BASE_MOTION_MS
        }
    }

    /// Duration for a fade; reduce motion keeps the 120 ms fade.
    pub fn fade_ms(&self) -> u32 {
        FADE_MS
    }

    /// Duration of ordinary motion (colour, hover) in milliseconds.
    pub fn motion_ms(&self) -> u32 {
        if self.reduce_motion {
            FADE_MS
        } else {
            BASE_MOTION_MS
        }
    }

    /// Text scale as the percentage the settings row shows (100 – 200).
    pub fn text_scale_percent(&self) -> u32 {
        (self.text_scale * 100.0).round() as u32
    }

    /// The tier the compositor and every panel must use: reduce transparency
    /// always wins over the requested tier (D1 section 9).
    pub fn effective_tier(&self, requested: Tier) -> Tier {
        Tier::choose(self.reduce_transparency, true, Some(requested))
    }

    /// Persist a text scale (100 – 200 %) and return the resolved settings.
    pub fn save_text_scale(store: &SettingsStore, percent: u32) -> Result<A11ySettings, String> {
        let clamped = percent.clamp(
            (MIN_TEXT_SCALE * 100.0) as u32,
            (MAX_TEXT_SCALE * 100.0) as u32,
        );
        store.save(
            SETTING_TEXT_SCALE,
            toml::Value::String(format!("{clamped}%")),
        )?;
        Ok(A11ySettings::load(store))
    }

    /// Persist the reduce-motion switch and return the resolved settings.
    pub fn save_reduce_motion(
        store: &SettingsStore,
        reduce_motion: bool,
    ) -> Result<A11ySettings, String> {
        store.save(SETTING_REDUCE_MOTION, toml::Value::Boolean(reduce_motion))?;
        Ok(A11ySettings::load(store))
    }

    /// Environment lines the session launcher exports for apps.
    pub fn env_lines(&self) -> Vec<String> {
        vec![
            format!(
                "AETHER_SHELL_REDUCE_MOTION={}",
                if self.reduce_motion { 1 } else { 0 }
            ),
            format!(
                "AETHER_SHELL_REDUCE_TRANSPARENCY={}",
                if self.reduce_transparency { 1 } else { 0 }
            ),
            format!("AETHER_SHELL_TEXT_SCALE={:.2}", self.text_scale),
            format!("AETHER_SHELL_MOTION_MS={}", self.motion_ms()),
            format!("AETHER_SHELL_FADE_MS={}", self.fade_ms()),
            format!("AETHER_SHELL_FOCUS_RING_PX={FOCUS_RING_PX}"),
        ]
    }

    /// Harness-readable summary (`--a11y-status`), ending in
    /// `AETHER_SHELL_A11Y_OK`.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "A11Y_REDUCE_MOTION: {}\n",
            if self.reduce_motion { "on" } else { "off" }
        ));
        out.push_str(&format!(
            "A11Y_REDUCE_TRANSPARENCY: {}\n",
            if self.reduce_transparency { "on" } else { "off" }
        ));
        out.push_str(&format!(
            "A11Y_TEXT_SCALE: {}% scale={:.2}\n",
            self.text_scale_percent(),
            self.text_scale
        ));
        out.push_str(&format!(
            "A11Y_MOTION: slide_ms={} fade_ms={} motion_ms={}\n",
            self.slide_ms(),
            self.fade_ms(),
            self.motion_ms()
        ));
        out.push_str(&format!(
            "A11Y_FOCUS_INDICATOR: width={FOCUS_RING_PX}px color=accent always_visible=true\n"
        ));
        out.push_str("AETHER_SHELL_A11Y_OK\n");
        out
    }
}

/// Clamp a text scale to the 100 % – 200 % range; NaN falls back to 100 %.
pub fn clamp_scale(scale: f32) -> f32 {
    if scale.is_nan() {
        MIN_TEXT_SCALE
    } else {
        scale.clamp(MIN_TEXT_SCALE, MAX_TEXT_SCALE)
    }
}

/// Parse `theme.text-scale`: `"150%"` (the settings row), `"1.5"`, or a TOML
/// float such as `1.5` all resolve to a factor.
pub fn parse_scale(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Float(number) => Some(clamp_scale(*number as f32)),
        toml::Value::Integer(number) => Some(clamp_scale(*number as f32)),
        toml::Value::String(text) => {
            let trimmed = text.trim();
            let factor = match trimmed.strip_suffix('%') {
                Some(percent) => percent.trim().parse::<f32>().ok()? / 100.0,
                None => trimmed.parse::<f32>().ok()?,
            };
            Some(clamp_scale(factor))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> (SettingsStore, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "aether-a11y-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (SettingsStore::new(root.clone()), root)
    }

    #[test]
    fn defaults_keep_motion_transparency_and_full_text() {
        let settings = A11ySettings::default();
        assert!(!settings.reduce_motion);
        assert!(!settings.reduce_transparency);
        assert_eq!(settings.text_scale, 1.0);
        assert_eq!(settings.slide_ms(), BASE_MOTION_MS);
        assert_eq!(settings.fade_ms(), FADE_MS);
        assert_eq!(settings.text_scale_percent(), 100);
        assert_eq!(settings.effective_tier(Tier::Full), Tier::Full);
    }

    #[test]
    fn reduce_motion_collapses_slides_to_a_120_ms_fade() {
        let settings = A11ySettings {
            reduce_motion: true,
            ..A11ySettings::default()
        };
        assert_eq!(settings.slide_ms(), 0, "movement is removed");
        assert_eq!(settings.motion_ms(), 120);
        assert_eq!(settings.fade_ms(), 120);
    }

    #[test]
    fn text_scale_parses_rows_floats_and_percentages() {
        assert_eq!(parse_scale(&toml::Value::String("150%".into())), Some(1.5));
        assert_eq!(parse_scale(&toml::Value::String("1.25".into())), Some(1.25));
        assert_eq!(parse_scale(&toml::Value::Float(1.75)), Some(1.75));
        assert_eq!(parse_scale(&toml::Value::Integer(2)), Some(2.0));
        assert_eq!(parse_scale(&toml::Value::String("50%".into())), Some(1.0), "clamped up");
        assert_eq!(parse_scale(&toml::Value::String("400%".into())), Some(2.0), "clamped down");
        assert_eq!(parse_scale(&toml::Value::String("large".into())), None);
        assert_eq!(clamp_scale(f32::NAN), 1.0);
    }

    #[test]
    fn reduce_transparency_forces_minimal_over_every_request() {
        let settings = A11ySettings {
            reduce_transparency: true,
            ..A11ySettings::default()
        };
        for requested in [Tier::Full, Tier::Reduced, Tier::Minimal] {
            assert_eq!(settings.effective_tier(requested), Tier::Minimal);
        }
    }

    #[test]
    fn settings_round_trip_through_the_store_and_clamp() {
        let (store, root) = temp_store("round-trip");
        let saved = A11ySettings::save_text_scale(&store, 300).unwrap();
        assert_eq!(saved.text_scale, 2.0, "300 % clamps to 200 %");
        assert_eq!(saved.text_scale_percent(), 200);
        let text = std::fs::read_to_string(root.join("theme/tokens.toml")).unwrap();
        assert!(text.contains("text-scale = \"200%\""), "{text}");
        let motion = A11ySettings::save_reduce_motion(&store, true).unwrap();
        assert!(motion.reduce_motion);
        assert_eq!(A11ySettings::load(&store), motion);
        let lowered = A11ySettings::save_text_scale(&store, 10).unwrap();
        assert_eq!(lowered.text_scale, 1.0, "10 % clamps to 100 %");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn summary_and_env_publish_the_contract() {
        let settings = A11ySettings {
            reduce_motion: true,
            reduce_transparency: true,
            text_scale: 1.5,
        };
        let env = settings.env_lines();
        assert!(env.contains(&"AETHER_SHELL_REDUCE_MOTION=1".to_string()));
        assert!(env.contains(&"AETHER_SHELL_REDUCE_TRANSPARENCY=1".to_string()));
        assert!(env.contains(&"AETHER_SHELL_TEXT_SCALE=1.50".to_string()));
        assert!(env.contains(&"AETHER_SHELL_MOTION_MS=120".to_string()));
        assert!(env.contains(&"AETHER_SHELL_FOCUS_RING_PX=2".to_string()));
        let summary = settings.summary();
        assert!(summary.contains("A11Y_REDUCE_MOTION: on"));
        assert!(summary.contains("A11Y_TEXT_SCALE: 150% scale=1.50"));
        assert!(summary.contains("A11Y_MOTION: slide_ms=0 fade_ms=120 motion_ms=120"));
        assert!(summary.contains("A11Y_FOCUS_INDICATOR: width=2px"));
        assert!(summary.ends_with("AETHER_SHELL_A11Y_OK\n"));
    }
}
