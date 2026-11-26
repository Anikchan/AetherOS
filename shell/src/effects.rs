//! Effect-tier control (P4-T04 ↔ P3-T05) and compositor marker checks.
//!
//! The control centre exposes the compositor's glass tiers (Full, Reduced,
//! Minimal). The contract with the compositor is one environment variable plus
//! the compositor's own markers, which this module reads back:
//!
//! * `AETHER_COMPOSITOR_EFFECT_TIER=full|reduced|minimal` — read by the
//!   compositor once at startup (`AetherCompositor::new`).
//! * `AETHER_COMPOSITOR_EFFECT_TIER: <tier>`,
//!   `AETHER_COMPOSITOR_EFFECT_TIER_SWITCH: from=… to=… reason=…`,
//!   `AETHER_COMPOSITOR_EFFECT_SUMMARY: tier=… tier_switches=…`,
//!   `AETHER_COMPOSITOR_FRAME: … tier=… est_ms=…`,
//!   `AETHER_COMPOSITOR_FRAME_STATS: frames=… idle_skips=… idle_pct=…`.
//!
//! The choice is persisted through the F4 settings store (`theme.effects-tier`
//! in `theme/tokens.toml`, `theme.reduce-transparency` for the accessibility
//! override) and published as the env above. `aether-shell --effects-tier`,
//! `--effects-env` and `--effects-check` are the stable interface for the
//! session launcher and the on-device acceptance harness.
//!
//! `reduce-transparency` always wins: D1 section 9 makes solid panels a hard
//! accessibility requirement, so it resolves to the Minimal tier regardless of
//! the requested tier.
//!
//! A running compositor keeps its tier until restart (the tier is read once at
//! startup; runtime switches come from its own frame-budget policy). The shell
//! therefore surfaces the mismatch instead of pretending the change is live:
//! [`EffectsReport::problems`] names it and `--effects-check` exits 1.

use crate::config::SettingsStore;
use crate::host::field;
use aether_ui::theme::Tier;

/// Environment variable the compositor reads at startup (P3-T05).
pub const ENV_TIER: &str = "AETHER_COMPOSITOR_EFFECT_TIER";
/// F4 settings key the control centre writes (`theme/tokens.toml`).
pub const SETTING_TIER: &str = "theme.effects-tier";
/// Accessibility override: forcing transparency off selects the Minimal tier.
pub const SETTING_REDUCE_TRANSPARENCY: &str = "theme.reduce-transparency";
/// Frame-rate the compositor budgets against when it reports none (P3-T06).
pub const DEFAULT_TARGET_FPS: f64 = 60.0;

/// Human label for a tier (`Full`, `Reduced`, `Minimal`).
pub fn label(tier: Tier) -> &'static str {
    match tier {
        Tier::Full => "Full",
        Tier::Reduced => "Reduced",
        Tier::Minimal => "Minimal",
    }
}

/// The next tier in the control-centre order Full → Reduced → Minimal → Full.
pub fn next(tier: Tier) -> Tier {
    match tier {
        Tier::Full => Tier::Reduced,
        Tier::Reduced => Tier::Minimal,
        Tier::Minimal => Tier::Full,
    }
}

/// Slint enum used by the shell surfaces (`surfaces.slint` compiles its own
/// `GlassTier`, so `aether_ui`'s enum cannot be assigned directly).
pub fn to_slint(tier: Tier) -> crate::GlassTier {
    match tier {
        Tier::Full => crate::GlassTier::Full,
        Tier::Reduced => crate::GlassTier::Reduced,
        Tier::Minimal => crate::GlassTier::Minimal,
    }
}

/// Resolved effect settings: what the user asked for and what wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSettings {
    /// The tier the user selected (`theme.effects-tier`), default Full.
    pub requested: Tier,
    /// The accessibility override (`theme.reduce-transparency`).
    pub reduce_transparency: bool,
}

impl Default for EffectSettings {
    fn default() -> Self {
        EffectSettings {
            requested: Tier::Full,
            reduce_transparency: false,
        }
    }
}

impl EffectSettings {
    /// Read `theme.effects-tier` and `theme.reduce-transparency` from the
    /// layered settings store. Unknown or unreadable values fall back to the
    /// schema default (Full, transparency on) instead of failing.
    pub fn load(store: &SettingsStore) -> EffectSettings {
        let requested = store
            .load(SETTING_TIER)
            .ok()
            .flatten()
            .and_then(|value| value.as_str().and_then(Tier::parse))
            .unwrap_or(Tier::Full);
        let reduce_transparency = store
            .load(SETTING_REDUCE_TRANSPARENCY)
            .ok()
            .flatten()
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        EffectSettings {
            requested,
            reduce_transparency,
        }
    }

    /// The tier the compositor and every panel should use.
    pub fn effective(&self) -> Tier {
        // The GPU capability check is the compositor's own tier policy (it
        // downgrades Full when a frame overruns); the shell only resolves the
        // user preference and the accessibility override.
        Tier::choose(self.reduce_transparency, true, Some(self.requested))
    }

    /// Persist a new requested tier and return the resolved settings.
    pub fn save(store: &SettingsStore, tier: Tier) -> Result<EffectSettings, String> {
        store.save(
            SETTING_TIER,
            toml::Value::String(tier.as_str().to_string()),
        )?;
        Ok(EffectSettings::load(store))
    }

    /// `AETHER_COMPOSITOR_EFFECT_TIER=<tier>` for the session environment.
    pub fn env_line(&self) -> String {
        format!("{ENV_TIER}={}", self.effective().as_str())
    }

    /// Short control-centre subtitle for the resolved tier (D1 section 4).
    pub fn subtitle(&self) -> &'static str {
        match self.effective() {
            Tier::Full => "Blur, refraction and shadow",
            Tier::Reduced => "Half-resolution blur",
            Tier::Minimal if self.reduce_transparency => "Solid panels (accessibility)",
            Tier::Minimal => "Solid panels, no blur",
        }
    }
}

/// One runtime tier switch reported by the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierSwitch {
    pub from: String,
    pub to: String,
    pub reason: String,
}

/// What a compositor log says about effects and frame scheduling.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectsReport {
    /// `AETHER_COMPOSITOR_EFFECT_TIER` / `EFFECT_SUMMARY tier=`.
    pub tier: Option<String>,
    /// Runtime switches (`EFFECT_SUMMARY tier_switches=` or the marker count).
    pub tier_switches: u32,
    /// Last recorded switch, when the compositor printed one.
    pub last_switch: Option<TierSwitch>,
    /// `AETHER_COMPOSITOR_FRAME_STATS frames=`.
    pub frames: Option<u64>,
    /// `AETHER_COMPOSITOR_FRAME_STATS idle_skips=`.
    pub idle_skips: Option<u64>,
    /// `AETHER_COMPOSITOR_FRAME_STATS idle_pct=` (or the `IDLE` marker).
    pub idle_pct: Option<f64>,
    /// `AETHER_COMPOSITOR_FRAME` lines seen (printed at n≤5 and every 120).
    pub frame_markers: u64,
    /// `AETHER_COMPOSITOR_FRAME_CALLBACKS n=` / `EFFECT_SUMMARY frame_callbacks=`.
    pub frame_callbacks: Option<u64>,
    /// Last `AETHER_COMPOSITOR_FRAME est_ms=` (modelled frame cost).
    pub est_ms: Option<f64>,
    /// Last `AETHER_COMPOSITOR_FRAME tier=`.
    pub frame_tier: Option<String>,
    /// Target frame rate the budget is derived from.
    pub target_fps: f64,
}

impl Default for EffectsReport {
    fn default() -> Self {
        EffectsReport {
            tier: None,
            tier_switches: 0,
            last_switch: None,
            frames: None,
            idle_skips: None,
            idle_pct: None,
            frame_markers: 0,
            frame_callbacks: None,
            est_ms: None,
            frame_tier: None,
            target_fps: DEFAULT_TARGET_FPS,
        }
    }
}

impl EffectsReport {
    /// Read the effects/frame markers out of a compositor log.
    pub fn scan(log: &str) -> EffectsReport {
        let mut report = EffectsReport::default();
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_FRAME_RATE:") {
                if let Ok(fps) = rest.trim().parse::<f64>() {
                    if fps > 0.0 {
                        report.target_fps = fps;
                    }
                }
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_EFFECT_TIER:") {
                report.tier = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_EFFECT_TIER_SWITCH:") {
                report.tier_switches += 1;
                if let (Some(from), Some(to)) = (field(rest, "from"), field(rest, "to")) {
                    report.last_switch = Some(TierSwitch {
                        from: from.to_string(),
                        to: to.to_string(),
                        reason: field(rest, "reason").unwrap_or("unknown").to_string(),
                    });
                }
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_EFFECT_SUMMARY:") {
                if let Some(tier) = field(rest, "tier") {
                    report.tier = Some(tier.to_string());
                }
                if let Some(count) = field(rest, "tier_switches").and_then(|value| value.parse().ok())
                {
                    report.tier_switches = count;
                }
                if let Some(count) = field(rest, "frame_callbacks").and_then(|value| value.parse().ok())
                {
                    report.frame_callbacks = Some(count);
                }
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_FRAME:") {
                report.frame_markers += 1;
                if let Some(tier) = field(rest, "tier") {
                    report.frame_tier = Some(tier.to_string());
                }
                if let Some(ms) = field(rest, "est_ms").and_then(|value| value.parse().ok()) {
                    report.est_ms = Some(ms);
                }
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_FRAME_CALLBACKS:") {
                if let Some(count) = field(rest, "n").and_then(|value| value.parse().ok()) {
                    report.frame_callbacks = Some(count);
                }
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_IDLE:") {
                if let Some(pct) = field(rest, "idle_pct").and_then(|value| value.parse().ok()) {
                    report.idle_pct = Some(pct);
                }
            } else if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_FRAME_STATS:") {
                if let Some(count) = field(rest, "frames").and_then(|value| value.parse().ok()) {
                    report.frames = Some(count);
                }
                if let Some(count) = field(rest, "idle_skips").and_then(|value| value.parse().ok()) {
                    report.idle_skips = Some(count);
                }
                if let Some(pct) = field(rest, "idle_pct").and_then(|value| value.parse().ok()) {
                    report.idle_pct = Some(pct);
                }
                if let Some(fps) = field(rest, "target_fps").and_then(|value| value.parse().ok()) {
                    report.target_fps = fps;
                }
            }
        }
        report
    }

    /// Frame budget in milliseconds for the reported target frame rate.
    pub fn budget_ms(&self) -> f64 {
        1000.0 / self.target_fps
    }

    /// Harness-readable problems; empty means the compositor agrees with the
    /// setting and reported both frames and idle time.
    pub fn problems(&self, expected: Tier) -> Vec<String> {
        let mut problems = Vec::new();
        match &self.tier {
            None => problems.push(
                "no AETHER_COMPOSITOR_EFFECT_TIER marker: is this a compositor log?".to_string(),
            ),
            Some(tier) if *tier != expected.as_str() => problems.push(format!(
                "compositor runs tier {tier}, the setting resolves to {} — restart the compositor to apply it",
                expected.as_str()
            )),
            Some(_) => {}
        }
        match self.frames {
            None => problems.push(
                "no AETHER_COMPOSITOR_FRAME_STATS marker: the compositor did not report frames"
                    .to_string(),
            ),
            Some(0) => problems.push(
                "the compositor presented no frames: the desktop drew nothing".to_string(),
            ),
            Some(_) => {}
        }
        if self.idle_pct.is_none() {
            problems.push(
                "no idle measurement: AETHER_COMPOSITOR_FRAME_STATS carries no idle_pct"
                    .to_string(),
            );
        }
        if let Some(ms) = self.est_ms {
            let budget = self.budget_ms();
            if ms > budget {
                problems.push(format!(
                    "frame cost {ms:.2} ms exceeds the {budget:.2} ms budget at tier {}",
                    self.frame_tier.as_deref().unwrap_or(expected.as_str())
                ));
            }
        }
        problems
    }

    /// True when the log proves the tier and the frame/idle pipeline.
    pub fn is_complete(&self, expected: Tier) -> bool {
        self.problems(expected).is_empty()
    }

    /// One line per measurement plus a verdict, for `--effects-check`.
    pub fn summary(&self, expected: Tier) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "EFFECT_TIER expected={} compositor={} switches={} last_switch={}\n",
            expected.as_str(),
            self.tier.as_deref().unwrap_or("none"),
            self.tier_switches,
            self.last_switch
                .as_ref()
                .map(|switch| format!("{}→{} ({})", switch.from, switch.to, switch.reason))
                .unwrap_or_else(|| "none".to_string())
        ));
        out.push_str(&format!(
            "EFFECT_FRAMES frames={} idle_skips={} idle_pct={} callbacks={} markers={} est_ms={} budget_ms={:.2}\n",
            self.frames.map(|value| value.to_string()).unwrap_or_else(|| "-".into()),
            self.idle_skips
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
            self.idle_pct
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "-".into()),
            self.frame_callbacks
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into()),
            self.frame_markers,
            self.est_ms
                .map(|value| format!("{value:.3}"))
                .unwrap_or_else(|| "-".into()),
            self.budget_ms()
        ));
        let problems = self.problems(expected);
        if problems.is_empty() {
            out.push_str("AETHER_SHELL_EFFECTS_OK\n");
        } else {
            for problem in &problems {
                out.push_str(&format!("AETHER_SHELL_EFFECTS_PROBLEM: {problem}\n"));
            }
            out.push_str("AETHER_SHELL_EFFECTS_FAILED\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "\
AETHER_COMPOSITOR_FRAME_RATE: 60
AETHER_COMPOSITOR_EFFECT_TIER: reduced
AETHER_COMPOSITOR_EFFECT_PIPELINE: tier=reduced blur=half
AETHER_COMPOSITOR_FRAME: n=1 time_ms=16 damage_rects=2 damage_px=4096 windows=1 tier=reduced est_ms=1.601
";

    const TAIL: &str = "\
AETHER_COMPOSITOR_FRAME_CALLBACKS: n=3 time_ms=160
AETHER_COMPOSITOR_IDLE: idle_ms=900 idle_pct=93.8 frames=12
AETHER_COMPOSITOR_FRAME_STATS: frames=12 idle_skips=48 idle_ms=900 idle_pct=93.8 presented_px=49152 avg_gap_ms=16.02 last_gap_ms=16.01 target_fps=60
AETHER_COMPOSITOR_EFFECT_SUMMARY: tier=reduced tier_switches=1 frame_callbacks=3
AETHER_COMPOSITOR_DONE
";

    fn complete_log() -> String {
        format!("{HEAD}{TAIL}")
    }

    #[test]
    fn defaults_resolve_to_full_with_transparency() {
        let settings = EffectSettings::default();
        assert_eq!(settings.requested, Tier::Full);
        assert_eq!(settings.effective(), Tier::Full);
        assert_eq!(settings.env_line(), "AETHER_COMPOSITOR_EFFECT_TIER=full");
        assert_eq!(settings.subtitle(), "Blur, refraction and shadow");
    }

    #[test]
    fn reduce_transparency_forces_minimal_and_says_why() {
        let settings = EffectSettings {
            requested: Tier::Full,
            reduce_transparency: true,
        };
        assert_eq!(settings.effective(), Tier::Minimal);
        assert_eq!(settings.env_line(), "AETHER_COMPOSITOR_EFFECT_TIER=minimal");
        assert_eq!(settings.subtitle(), "Solid panels (accessibility)");
        let explicit = EffectSettings {
            requested: Tier::Minimal,
            reduce_transparency: false,
        };
        assert_eq!(explicit.subtitle(), "Solid panels, no blur");
    }

    #[test]
    fn tiers_cycle_in_control_centre_order() {
        assert_eq!(next(Tier::Full), Tier::Reduced);
        assert_eq!(next(Tier::Reduced), Tier::Minimal);
        assert_eq!(next(Tier::Minimal), Tier::Full);
        assert_eq!(label(Tier::Reduced), "Reduced");
    }

    #[test]
    fn settings_round_trip_through_the_store() {
        let root = std::env::temp_dir().join(format!(
            "aether-effects-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SettingsStore::new(&root);
        let saved = EffectSettings::save(&store, Tier::Reduced).unwrap();
        assert_eq!(saved.requested, Tier::Reduced);
        assert_eq!(EffectSettings::load(&store), saved);
        let text = std::fs::read_to_string(root.join("theme/tokens.toml")).unwrap();
        assert!(text.contains("effects-tier = \"reduced\""), "{text}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_complete_log_passes_every_check() {
        let report = EffectsReport::scan(&complete_log());
        assert_eq!(report.tier.as_deref(), Some("reduced"));
        assert_eq!(report.frames, Some(12));
        assert_eq!(report.idle_skips, Some(48));
        assert_eq!(report.idle_pct, Some(93.8));
        assert_eq!(report.frame_callbacks, Some(3));
        assert_eq!(report.est_ms, Some(1.601));
        assert!(report.problems(Tier::Reduced).is_empty(), "{:?}", report.problems(Tier::Reduced));
        let summary = report.summary(Tier::Reduced);
        assert!(summary.contains("EFFECT_TIER expected=reduced compositor=reduced switches=1"));
        assert!(summary.contains("budget_ms=16.67"));
        assert!(summary.ends_with("AETHER_SHELL_EFFECTS_OK\n"));
    }

    #[test]
    fn a_tier_mismatch_names_the_restart() {
        let report = EffectsReport::scan(&complete_log());
        let problems = report.problems(Tier::Full);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("compositor runs tier reduced"));
        assert!(problems[0].contains("restart the compositor"));
        assert!(report.summary(Tier::Full).ends_with("AETHER_SHELL_EFFECTS_FAILED\n"));
    }

    #[test]
    fn missing_frames_idle_and_cost_overruns_are_reported() {
        let empty = EffectsReport::scan("hello\n");
        let problems = empty.problems(Tier::Full);
        assert!(problems.iter().any(|p| p.contains("no AETHER_COMPOSITOR_EFFECT_TIER")));
        assert!(problems.iter().any(|p| p.contains("no AETHER_COMPOSITOR_FRAME_STATS")));
        assert!(problems.iter().any(|p| p.contains("no idle measurement")));

        let idle_only = EffectsReport::scan(&format!(
            "AETHER_COMPOSITOR_EFFECT_TIER: full\nAETHER_COMPOSITOR_FRAME_STATS: frames=0 idle_skips=9 idle_ms=100 idle_pct=100.0 presented_px=0 avg_gap_ms=0.00 last_gap_ms=0.00 target_fps=60\n"
        ));
        assert!(idle_only
            .problems(Tier::Full)
            .iter()
            .any(|p| p.contains("presented no frames")));

        let overrun = EffectsReport::scan(&format!(
            "{}AETHER_COMPOSITOR_FRAME: n=120 time_ms=2000 damage_rects=1 damage_px=10 windows=3 tier=full est_ms=18.400\n{TAIL}",
            HEAD.replace("reduced", "full")
        ));
        assert!(overrun
            .problems(Tier::Full)
            .iter()
            .any(|p| p.contains("18.40 ms exceeds the 16.67 ms budget")));
    }

    #[test]
    fn switches_are_counted_and_the_last_one_kept() {
        let log = format!(
            "{HEAD}AETHER_COMPOSITOR_EFFECT_TIER_SWITCH: from=full to=reduced reason=frame-budget\n{TAIL}"
        );
        let report = EffectsReport::scan(&log);
        let switch = report.last_switch.as_ref().expect("switch marker parsed");
        assert_eq!(switch.from, "full");
        assert_eq!(switch.to, "reduced");
        assert_eq!(switch.reason, "frame-budget");
        // The summary marker wins over the per-switch count.
        assert_eq!(report.tier_switches, 1);
    }
}
