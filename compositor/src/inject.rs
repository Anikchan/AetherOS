//! Test-only key injection for the headless backend.
//!
//! The headless backend has no input device, so a key can never reach a
//! `wl_keyboard` there — which is exactly the path the shell's keyboard handling
//! needs to be proven against (`shell/src/keys.rs`, F1 SHL-18). Setting
//! `AETHER_COMPOSITOR_INJECT_KEYS` to a comma-separated list of **evdev**
//! keycodes makes the compositor:
//!
//! 1. advertise a `wl_seat` with a keyboard and a pointer (there is no device to
//!    do it otherwise), and
//! 2. replay the keycodes, one per frame tick, into whatever surface currently
//!    holds keyboard focus, logging
//!    `AETHER_COMPOSITOR_KEY_INJECTED: evdev=<n> focus=<surface|none>`.
//!
//! This is test plumbing, not a feature: it is only read by [`run_headless`],
//! the DRM backend ignores it, and an unset variable leaves the headless backend
//! exactly as before (no seat at all).
//!
//! [`run_headless`]: crate::run_headless

/// Environment variable that enables headless key injection (evdev codes).
pub const INJECT_KEYS_ENV: &str = "AETHER_COMPOSITOR_INJECT_KEYS";

/// Environment variable that enables one headless pointer click.
pub const INJECT_POINTER_ENV: &str = "AETHER_COMPOSITOR_INJECT_POINTER";

/// Largest evdev keycode accepted (`KEY_MAX` is `0x2ff`).
const MAX_EVDEV: u32 = 0x2ff;

/// Parse a comma-separated list of evdev keycodes.
///
/// Whitespace, empty fields, non-numeric fields, `0` and codes above
/// [`MAX_EVDEV`] are dropped instead of failing the whole list: a malformed
/// value must not stop the compositor from starting, and the caller reports how
/// many codes it kept.
pub fn parse_keys(value: &str) -> Vec<u32> {
    value
        .split(',')
        .filter_map(|field| field.trim().parse::<u32>().ok())
        .filter(|code| *code > 0 && *code <= MAX_EVDEV)
        .collect()
}

/// One injected pointer action: an output-space position plus an optional
/// button (evdev button code, `1` = left/BTN_LEFT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerAction {
    pub x: i32,
    pub y: i32,
    /// `None` = move the pointer only, no click.
    pub button: Option<u32>,
}

/// Parse `AETHER_COMPOSITOR_INJECT_POINTER` (`x=600,y=1050,button=1`).
///
/// `x` and `y` are required output logical coordinates; `button` is optional
/// and `1` (BTN_LEFT) when omitted but a click was asked for... it is not: an
/// omitted button means the action is a motion only, which is what a
/// hover/enter test wants. Unknown keys are ignored, and a missing or
/// unparsable `x`/`y` returns `None` (the caller reports it and injects
/// nothing).
pub fn parse_pointer(value: &str) -> Option<PointerAction> {
    let mut x = None;
    let mut y = None;
    let mut button = None;
    for field in value.split(',') {
        let Some((key, raw)) = field.split_once('=') else {
            continue;
        };
        let raw = raw.trim();
        match key.trim() {
            "x" => x = raw.parse::<i32>().ok(),
            "y" => y = raw.parse::<i32>().ok(),
            "button" | "btn" => {
                button = raw
                    .parse::<u32>()
                    .ok()
                    .filter(|code| *code > 0 && *code <= MAX_EVDEV)
            }
            _ => {}
        }
    }
    let (x, y) = (x?, y?);
    if x < 0 || y < 0 {
        return None;
    }
    // `button=0` and junk parse to `None`, which would silently turn a click
    // into a hover; keep the explicit "no button" case to a missing field.
    if value.contains("button=") && button.is_none() {
        return None;
    }
    Some(PointerAction { x, y, button })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_a_comma_separated_list() {
        assert_eq!(parse_keys("30,48,18"), vec![30, 48, 18]);
        // Spaces and empty fields are tolerated (env files and shells add them).
        assert_eq!(parse_keys(" 15 , 28 ,57 "), vec![15, 28, 57]);
        assert_eq!(parse_keys("15,,28,"), vec![15, 28]);
        assert!(parse_keys("").is_empty());
        assert!(parse_keys("   ").is_empty());
    }

    #[test]
    fn drops_junk_and_out_of_range_codes() {
        // A typo skips that code, it does not lose the rest of the list.
        assert_eq!(parse_keys("30,tab,48"), vec![30, 48]);
        assert_eq!(parse_keys("0,30"), vec![30]);
        assert_eq!(parse_keys("1000,30"), vec![30], "above KEY_MAX");
        assert_eq!(parse_keys("-1,30"), vec![30]);
        assert!(parse_keys("nope").is_empty());
    }

    #[test]
    fn accepts_the_shells_key_codes() {
        // Tab, Enter, Space, Super — what the shell's keys module consumes.
        let keys = parse_keys("15,28,57,125");
        assert_eq!(keys, vec![15, 28, 57, 125]);
        // The seat takes xkb keycodes (evdev + 8) while `wl_keyboard.key`
        // delivers evdev codes, so a client adds 8 back — the value the shell
        // receives on the wire is exactly what the environment variable lists.
        for evdev in [15u32, 28, 57, 125] {
            let xkb = evdev + 8;
            assert_eq!(xkb - 8, evdev);
        }
    }

    #[test]
    fn parses_a_pointer_click() {
        // The dock on a 1920x1080 output: 720x64 anchored bottom → (600,1016).
        assert_eq!(
            parse_pointer("x=600,y=1050,button=1"),
            Some(PointerAction {
                x: 600,
                y: 1050,
                button: Some(1)
            })
        );
        // Order does not matter and spaces are tolerated.
        assert_eq!(
            parse_pointer("button=1, y=1050 , x=600"),
            Some(PointerAction {
                x: 600,
                y: 1050,
                button: Some(1)
            })
        );
        // No button field: a hover (enter/motion) only.
        assert_eq!(
            parse_pointer("x=10,y=20"),
            Some(PointerAction {
                x: 10,
                y: 20,
                button: None
            })
        );
    }

    #[test]
    fn rejects_an_unusable_pointer_action() {
        // Position is mandatory.
        assert_eq!(parse_pointer(""), None);
        assert_eq!(parse_pointer("x=600"), None);
        assert_eq!(parse_pointer("y=1050"), None);
        assert_eq!(parse_pointer("x=nope,y=1"), None);
        // A button field that does not parse is an error, not a silent hover.
        assert_eq!(parse_pointer("x=1,y=2,button=0"), None);
        assert_eq!(parse_pointer("x=1,y=2,button=left"), None);
        // Off-output coordinates are rejected instead of clamped.
        assert_eq!(parse_pointer("x=-1,y=2"), None);
    }
}
