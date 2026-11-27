//! XKB keysym → shell action, the one path keyboard input takes (F1 SHL-18).
//!
//! `wl_keyboard.key.key` is the **evdev keycode** itself — no offset. A keysym
//! comes from running the XKB keycode, `evdev + 8` ([`xkb_keycode`]), through
//! the keymap; this module carries the default `us` layout so the host can map
//! the keys the shell owns without a keymap round trip. The rest of the chain is
//! the shell's side of the contract: keycode → keysym →
//! [`ShellController::handle_key`] (or `handle_shortcut` when Super is held), the
//! same entry points the Slint surfaces call.
//!
//! [`crate::wayland`] feeds the events it reads off `wl_keyboard` into
//! [`dispatch`], and `shell/tests/keyboard_wire.rs` drives [`dispatch`] with the
//! same evdev values, so the test and the host run one code path. Keys the shell
//! does not own return `handled = false` and are left for the focused app.

use crate::kbdnav::Surface;
use crate::ShellController;

/// `wl_keyboard.modifiers.mods_depressed` bit for Shift (XKB state mask).
pub const MOD_SHIFT: u32 = 1 << 0;
/// `wl_keyboard.modifiers.mods_depressed` bit for Mod4 (Super).
pub const MOD_MOD4: u32 = 1 << 6;

/// XKB keycodes are evdev codes plus this offset (`evdev + 8`). The offset is
/// *not* on the wire: `wl_keyboard.key.key` is the evdev code.
pub const XKB_KEYCODE_OFFSET: u32 = 8;

/// XKB keycode for an evdev key (`evdev + 8`). A keysym is
/// `keymap(xkb_keycode(evdev))`; [`BINDINGS`] is that map for the default
/// `us` layout.
pub fn xkb_keycode(evdev: u32) -> u32 {
    evdev + XKB_KEYCODE_OFFSET
}

/// XKB keysym values (`xkbcommon-keysyms.h`) the shell binds.
pub const XKB_ESCAPE: u32 = 0xff1b;
pub const XKB_TAB: u32 = 0xff09;
pub const XKB_ISO_LEFT_TAB: u32 = 0xfe20;
pub const XKB_RETURN: u32 = 0xff0d;
pub const XKB_KP_ENTER: u32 = 0xff8d;
pub const XKB_SPACE: u32 = 0x0020;
pub const XKB_LEFT: u32 = 0xff51;
pub const XKB_UP: u32 = 0xff52;
pub const XKB_RIGHT: u32 = 0xff53;
pub const XKB_DOWN: u32 = 0xff54;

/// One row of the default `us` layout: evdev keycode, unshifted keysym and the
/// binding name the controller understands.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub evdev: u32,
    pub keysym: u32,
    pub name: &'static str,
}

/// Every key the shell owns. Anything else belongs to the focused app.
pub const BINDINGS: &[Binding] = &[
    Binding {
        evdev: 1,
        keysym: XKB_ESCAPE,
        name: "escape",
    },
    Binding {
        evdev: 15,
        keysym: XKB_TAB,
        name: "tab",
    },
    Binding {
        evdev: 28,
        keysym: XKB_RETURN,
        name: "enter",
    },
    Binding {
        evdev: 96,
        keysym: XKB_KP_ENTER,
        name: "enter",
    },
    Binding {
        evdev: 57,
        keysym: XKB_SPACE,
        name: "space",
    },
    Binding {
        evdev: 103,
        keysym: XKB_UP,
        name: "up",
    },
    Binding {
        evdev: 105,
        keysym: XKB_LEFT,
        name: "left",
    },
    Binding {
        evdev: 106,
        keysym: XKB_RIGHT,
        name: "right",
    },
    Binding {
        evdev: 108,
        keysym: XKB_DOWN,
        name: "down",
    },
    Binding {
        evdev: 30,
        keysym: 0x0061,
        name: "a",
    },
    Binding {
        evdev: 31,
        keysym: 0x0073,
        name: "s",
    },
    Binding {
        evdev: 32,
        keysym: 0x0064,
        name: "d",
    },
    Binding {
        evdev: 33,
        keysym: 0x0066,
        name: "f",
    },
    Binding {
        evdev: 34,
        keysym: 0x0067,
        name: "g",
    },
    Binding {
        evdev: 35,
        keysym: 0x0068,
        name: "h",
    },
    Binding {
        evdev: 36,
        keysym: 0x006a,
        name: "j",
    },
    Binding {
        evdev: 37,
        keysym: 0x006b,
        name: "k",
    },
    Binding {
        evdev: 38,
        keysym: 0x006c,
        name: "l",
    },
    Binding {
        evdev: 44,
        keysym: 0x007a,
        name: "z",
    },
    Binding {
        evdev: 45,
        keysym: 0x0078,
        name: "x",
    },
    Binding {
        evdev: 46,
        keysym: 0x0063,
        name: "c",
    },
    Binding {
        evdev: 47,
        keysym: 0x0076,
        name: "v",
    },
    Binding {
        evdev: 48,
        keysym: 0x0062,
        name: "b",
    },
    Binding {
        evdev: 49,
        keysym: 0x006e,
        name: "n",
    },
    Binding {
        evdev: 50,
        keysym: 0x006d,
        name: "m",
    },
];

/// One key press, in the terms `wl_keyboard` delivers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    /// evdev keycode, exactly as `wl_keyboard.key.key` carries it.
    pub evdev: u32,
    pub shift: bool,
    pub super_key: bool,
}

impl KeyInput {
    /// `wl_keyboard.key.key` (the evdev code, not `evdev + 8`) plus
    /// `wl_keyboard.modifiers.mods_depressed`.
    pub fn from_wire(keycode: u32, mods_depressed: u32) -> KeyInput {
        KeyInput {
            evdev: keycode,
            shift: mods_depressed & MOD_SHIFT != 0,
            super_key: mods_depressed & MOD_MOD4 != 0,
        }
    }
}

/// What one key press did, for the host log (`AETHER_SHELL_KEY`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub evdev: u32,
    /// Keysym the evdev keycode maps to on the default `us` layout.
    pub keysym: Option<u32>,
    /// Binding name handed to the controller (`tab`, `shift+tab`, `super+space`).
    pub binding: Option<String>,
    pub handled: bool,
    /// Layer surface that holds `wl_keyboard` focus, i.e. the one the
    /// compositor sent the key to (`None` when the host has not been told).
    pub surface: Option<&'static str>,
    /// The shell's own focus ring after the event, so the log shows the move.
    pub ring: Surface,
}

impl KeyEvent {
    /// `AETHER_SHELL_KEY: …` marker line. `focus=` is the surface that received
    /// the key (it matches the compositor's `SEAT_FOCUS … layer=`); `ring=` is
    /// the shell's focus ring after the event.
    pub fn line(&self) -> String {
        format!(
            "AETHER_SHELL_KEY: evdev={} keysym={} binding={} handled={} focus={} ring={}",
            self.evdev,
            self.keysym
                .map(|keysym| format!("0x{keysym:04x}"))
                .unwrap_or_else(|| "none".to_string()),
            self.binding.as_deref().unwrap_or("none"),
            self.handled,
            self.surface.unwrap_or("none"),
            self.ring.as_str()
        )
    }
}

/// The keysym an evdev keycode produces on the default `us` layout (shift
/// applied where it changes the symbol).
pub fn keysym(evdev: u32, shift: bool) -> Option<u32> {
    let binding = BINDINGS.iter().find(|binding| binding.evdev == evdev)?;
    if shift {
        match binding.keysym {
            XKB_TAB => return Some(XKB_ISO_LEFT_TAB),
            0x0061..=0x007a => return Some(binding.keysym - 0x20),
            _ => {}
        }
    }
    Some(binding.keysym)
}

/// The binding name for an evdev keycode, in controller terms.
pub fn binding(evdev: u32, shift: bool) -> Option<String> {
    let binding = BINDINGS.iter().find(|binding| binding.evdev == evdev)?;
    if shift && binding.name == "tab" {
        return Some("shift+tab".to_string());
    }
    Some(binding.name.to_string())
}

/// Feed one key press through the shared path: keycode → keysym → the
/// controller entry point the Slint surfaces use. Super+<key> goes to
/// `handle_shortcut` (Super+Space opens the launcher, Super+L locks, …);
/// everything else goes to `handle_key`.
pub fn dispatch(
    controller: &ShellController,
    input: KeyInput,
    surface: Option<&'static str>,
) -> KeyEvent {
    let keysym = keysym(input.evdev, input.shift);
    let name = binding(input.evdev, input.shift);
    let (binding, handled) = match name {
        Some(name) if input.super_key && !matches!(name.as_str(), "tab" | "shift+tab") => {
            let shortcut = format!("super+{name}");
            let handled = controller.handle_shortcut(&shortcut);
            (Some(shortcut), handled)
        }
        Some(name) => {
            let handled = controller.handle_key(&name);
            (Some(name), handled)
        }
        None => (None, false),
    };
    KeyEvent {
        evdev: input.evdev,
        keysym,
        binding,
        handled,
        surface,
        ring: controller.focus().surface,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_carries_the_evdev_code_not_the_xkb_code() {
        // `wl_keyboard.key.key` is the evdev code: an injected evdev 15 stays 15
        // here (it used to be read as 15 - 8 = 7, shifting every key).
        let input = KeyInput::from_wire(15, MOD_SHIFT);
        assert_eq!(input.evdev, 15);
        assert!(input.shift);
        assert!(!input.super_key);
        assert_eq!(KeyInput::from_wire(28, 0).evdev, 28);
        assert!(
            KeyInput::from_wire(125, MOD_MOD4).super_key,
            "Super is Mod4"
        );
        // A keymap is indexed with the XKB keycode: evdev + 8.
        assert_eq!(xkb_keycode(15), 23);
        assert_eq!(xkb_keycode(1), 9);
    }

    #[test]
    fn injected_evdev_codes_map_to_the_expected_keysyms() {
        // The three codes the compositor's headless injection replays.
        assert_eq!(keysym(15, false), Some(XKB_TAB), "evdev 15 (KEY_TAB)");
        assert_eq!(keysym(28, false), Some(XKB_RETURN), "evdev 28 (KEY_ENTER)");
        assert_eq!(keysym(1, false), Some(XKB_ESCAPE), "evdev 1 (KEY_ESC)");
        assert_eq!(binding(15, false).as_deref(), Some("tab"));
        assert_eq!(binding(28, false).as_deref(), Some("enter"));
        assert_eq!(binding(1, false).as_deref(), Some("escape"));
        assert_eq!(keysym(7, false), None, "7 is not a key the shell owns");
    }

    #[test]
    fn keysyms_follow_the_us_layout() {
        assert_eq!(keysym(15, false), Some(XKB_TAB));
        assert_eq!(
            keysym(15, true),
            Some(XKB_ISO_LEFT_TAB),
            "Shift+Tab is BackTab"
        );
        assert_eq!(keysym(28, false), Some(XKB_RETURN));
        assert_eq!(keysym(57, false), Some(XKB_SPACE));
        assert_eq!(keysym(30, false), Some(0x0061));
        assert_eq!(keysym(30, true), Some(0x0041), "Shift+A is 'A'");
        assert_eq!(keysym(113, false), None, "a key the shell does not own");
    }

    #[test]
    fn bindings_are_unique_and_stable() {
        let mut codes: Vec<u32> = BINDINGS.iter().map(|binding| binding.evdev).collect();
        codes.sort_unstable();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count, "one row per evdev keycode");
        assert_eq!(binding(15, true).as_deref(), Some("shift+tab"));
        assert_eq!(binding(28, false).as_deref(), Some("enter"));
        assert_eq!(binding(133, false), None, "Super alone is a modifier");
    }

    #[test]
    fn a_key_event_line_names_the_surface_and_the_ring() {
        let event = KeyEvent {
            evdev: 15,
            keysym: Some(XKB_TAB),
            binding: Some("tab".into()),
            handled: true,
            surface: Some("aether-shell-launcher"),
            ring: Surface::ControlCenter,
        };
        assert_eq!(
            event.line(),
            "AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true \
             focus=aether-shell-launcher ring=control-center"
        );
        let unowned = KeyEvent {
            evdev: 113,
            keysym: None,
            binding: None,
            handled: false,
            surface: None,
            ring: Surface::Menubar,
        };
        assert!(unowned
            .line()
            .contains("keysym=none binding=none handled=false focus=none"));
    }
}
