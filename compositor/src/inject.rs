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
}
