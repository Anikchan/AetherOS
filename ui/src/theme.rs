//! Theme helpers shared by the shell and the apps (P4, S3).
//!
//! Tokens themselves live in `design.slint` (D1 section 5). This module maps
//! the effect tiers and appearance settings that arrive from F4 configuration
//! files into the Slint enum, and provides the accessibility presets from
//! design D1 section 9.

use crate::GlassTier;

/// Effect tier (design D1 section 4 / F6). `reduce-transparency` forces the
/// minimal tier, which is a hard accessibility requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Full,
    Reduced,
    Minimal,
}

impl Tier {
    /// Choose the tier from the GPU capability check and user preferences.
    pub fn choose(reduce_transparency: bool, gpu_capable: bool, user: Option<Tier>) -> Tier {
        if reduce_transparency {
            return Tier::Minimal;
        }
        match user {
            Some(Tier::Full) if !gpu_capable => Tier::Reduced,
            Some(tier) => tier,
            None if gpu_capable => Tier::Full,
            None => Tier::Reduced,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Reduced => "reduced",
            Tier::Minimal => "minimal",
        }
    }

    pub fn parse(value: &str) -> Option<Tier> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Tier::Full),
            "reduced" => Some(Tier::Reduced),
            "minimal" => Some(Tier::Minimal),
            _ => None,
        }
    }

    pub fn to_slint(self) -> GlassTier {
        match self {
            Tier::Full => GlassTier::Full,
            Tier::Reduced => GlassTier::Reduced,
            Tier::Minimal => GlassTier::Minimal,
        }
    }
}

/// Appearance settings resolved from `~/.config/aether/theme/tokens.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

impl Appearance {
    /// Light/dark/automatic selection. `hour` is the local hour (0..24).
    pub fn resolve(preference: &str, hour: u8) -> Appearance {
        match preference.trim().to_ascii_lowercase().as_str() {
            "light" => Appearance::Light,
            "dark" => Appearance::Dark,
            "auto" | "automatic" => {
                if (7..19).contains(&hour) {
                    Appearance::Light
                } else {
                    Appearance::Dark
                }
            }
            _ => Appearance::Dark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_transparency_wins() {
        assert_eq!(Tier::choose(true, true, Some(Tier::Full)), Tier::Minimal);
    }

    #[test]
    fn full_downgrades_without_gpu() {
        assert_eq!(Tier::choose(false, false, Some(Tier::Full)), Tier::Reduced);
        assert_eq!(Tier::choose(false, false, Some(Tier::Reduced)), Tier::Reduced);
        assert_eq!(Tier::choose(false, false, Some(Tier::Minimal)), Tier::Minimal);
        assert_eq!(Tier::choose(false, true, None), Tier::Full);
        assert_eq!(Tier::choose(false, false, None), Tier::Reduced);
    }

    #[test]
    fn tier_names_round_trip() {
        for tier in [Tier::Full, Tier::Reduced, Tier::Minimal] {
            assert_eq!(Tier::parse(tier.as_str()), Some(tier));
        }
        assert_eq!(Tier::parse("solid"), None);
    }

    #[test]
    fn automatic_appearance_uses_daylight_hours() {
        assert_eq!(Appearance::resolve("auto", 12), Appearance::Light);
        assert_eq!(Appearance::resolve("auto", 2), Appearance::Dark);
        assert_eq!(Appearance::resolve("dark", 12), Appearance::Dark);
        assert_eq!(Appearance::resolve("something-else", 12), Appearance::Dark);
    }
}
