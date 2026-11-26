//! Activity Manager shortcut contract and matcher (F13 section 2, F9 AP-14,
//! TM-AC-01).
//!
//! The window itself accepts the chord while it has focus; opening the window
//! from anywhere is the compositor's job, so the same binding is published as
//! JSON (`aether-taskmanager --shortcut`) for the shell to register instead of
//! being hard-coded in two places.

use serde::{Deserialize, Serialize};

/// Action the shell registers for the accelerator.
pub const OPEN_ACTION: &str = "open-activity-manager";
/// Default accelerator (F13 section 2).
pub const DEFAULT_ACCEL: &str = "Super+Escape";
/// Accelerator under the Windows profile (F9 AP-14, TM-AC-01).
pub const WINDOWS_PROFILE_ACCEL: &str = "Ctrl+Shift+Esc";
/// TM-AC-01 opens the window within this budget.
pub const OPEN_BUDGET_MS: u64 = 500;

/// The published shortcut contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutBinding {
    pub action: String,
    pub default_accel: String,
    pub windows_profile_accel: String,
    pub budget_ms: u64,
}

impl Default for ShortcutBinding {
    fn default() -> Self {
        Self {
            action: OPEN_ACTION.into(),
            default_accel: DEFAULT_ACCEL.into(),
            windows_profile_accel: WINDOWS_PROFILE_ACCEL.into(),
            budget_ms: OPEN_BUDGET_MS,
        }
    }
}

impl ShortcutBinding {
    /// The accelerator the UI shows for the active profile.
    pub fn accel(&self, windows_profile: bool) -> &str {
        if windows_profile {
            &self.windows_profile_accel
        } else {
            &self.default_accel
        }
    }

    /// True when `pressed` is the accelerator for the active profile.
    pub fn matches(&self, pressed: &KeyChord, windows_profile: bool) -> bool {
        KeyChord::parse(self.accel(windows_profile))
            .map(|chord| chord == *pressed)
            .unwrap_or(false)
    }

    /// True when `pressed` is either published accelerator.
    pub fn matches_any(&self, pressed: &KeyChord) -> bool {
        self.default_accel_matches(pressed) || self.matches(pressed, true)
    }

    fn default_accel_matches(&self, pressed: &KeyChord) -> bool {
        KeyChord::parse(&self.default_accel)
            .map(|chord| chord == *pressed)
            .unwrap_or(false)
    }
}

/// One key with its modifier state, independent of the toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub key: KeyName,
}

/// The key part of a chord (normalised to lowercase).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyName {
    Escape,
    Char(char),
}

impl KeyChord {
    /// Parse an accelerator such as `"Ctrl+Shift+Esc"` or `"Super+Escape"`.
    pub fn parse(accel: &str) -> Option<Self> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut meta = false;
        let mut key = None;
        for part in accel.split('+') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "super" | "meta" | "win" => meta = true,
                "esc" | "escape" => key = Some(KeyName::Escape),
                other => {
                    let mut chars = other.chars();
                    match (chars.next(), chars.next()) {
                        (Some(character), None) => key = Some(KeyName::Char(character)),
                        _ => return None,
                    }
                }
            }
        }
        Some(Self {
            ctrl,
            shift,
            alt,
            meta,
            key: key?,
        })
    }

    /// Build a chord from a toolkit key event (`text` plus modifiers). Escape
    /// arrives as `"\u{1b}"`; unknown multi-character strings are ignored.
    pub fn from_event(text: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> Option<Self> {
        let key = if text == "\u{1b}" || text.eq_ignore_ascii_case("escape") {
            KeyName::Escape
        } else {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(character), None) => KeyName::Char(character.to_ascii_lowercase()),
                _ => return None,
            }
        };
        Some(Self {
            ctrl,
            shift,
            alt,
            meta,
            key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_binding_publishes_both_accelerators() {
        let binding = ShortcutBinding::default();
        assert_eq!(binding.action, OPEN_ACTION);
        assert_eq!(binding.accel(false), "Super+Escape");
        assert_eq!(binding.accel(true), "Ctrl+Shift+Esc");
        assert_eq!(binding.budget_ms, 500);
        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains("\"action\":\"open-activity-manager\""));
        assert!(json.contains("\"windows_profile_accel\":\"Ctrl+Shift+Esc\""));
    }

    #[test]
    fn ctrl_shift_escape_matches_only_with_the_windows_profile() {
        let binding = ShortcutBinding::default();
        let pressed = KeyChord {
            ctrl: true,
            shift: true,
            alt: false,
            meta: false,
            key: KeyName::Escape,
        };
        assert!(!binding.matches(&pressed, false));
        assert!(binding.matches(&pressed, true));
        assert!(binding.matches_any(&pressed));

        // Ctrl+Shift+Esc without Shift is not the shortcut.
        let no_shift = KeyChord {
            shift: false,
            ..pressed
        };
        assert!(!binding.matches_any(&no_shift));
    }

    #[test]
    fn super_escape_is_the_default_on_every_profile() {
        let binding = ShortcutBinding::default();
        let pressed = KeyChord {
            ctrl: false,
            shift: false,
            alt: false,
            meta: true,
            key: KeyName::Escape,
        };
        assert!(binding.matches(&pressed, false));
        assert!(binding.matches_any(&pressed));
    }

    #[test]
    fn parses_toolkit_events_into_the_same_chord() {
        let from_accel = KeyChord::parse("Ctrl+Shift+Esc").unwrap();
        let from_event = KeyChord::from_event("\u{1b}", true, true, false, false).unwrap();
        assert_eq!(from_accel, from_event);
        assert_eq!(from_accel.key, KeyName::Escape);
        // Letters normalise, so Shift+E and Shift+e agree.
        assert_eq!(
            KeyChord::from_event("E", false, true, false, false).unwrap(),
            KeyChord::parse("Shift+e").unwrap()
        );
        assert!(KeyChord::from_event("F9", false, false, false, false).is_none());
        assert!(KeyChord::parse("Ctrl+Shift").is_none());
    }
}
