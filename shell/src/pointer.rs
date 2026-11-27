//! Pointer routing into the Slint scene (P3-T02 → P4).
//!
//! The layer host is one Slint scene split across five surfaces, so a
//! `wl_pointer` event arrives in the coordinates of *one* surface while Slint
//! works in the coordinates of the whole composed window. This module is that
//! translation plus the dispatch: `surface-local + the surface's placement =
//! window position`, then `PointerMoved` / `PointerPressed` / `PointerReleased`
//! / `PointerExited` into the window. Because the handlers behind those events
//! are the ones the design system already wires (`clicked` on the dock tiles,
//! the tray buttons, the launcher rows), a click over the socket reaches the
//! same code a click in the composed desktop does.
//!
//! [`crate::wayland`] feeds the events it reads off `wl_pointer` into
//! [`PointerRouter::route`]; `shell/tests/pointer_click.rs` drives the same
//! function with a synthetic dock click, so the host and the test share one
//! path. The compositor has no headless pointer injection yet, so the socket
//! leg asserts the binding and the router's own events.

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};

use crate::ShellRoot;

/// Linux input event button codes (`BTN_LEFT`, `BTN_RIGHT`, `BTN_MIDDLE`).
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// One pointer event, in the terms the host reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerKind {
    /// The pointer entered the surface (no button).
    Enter,
    /// Movement inside the surface (no button).
    Motion,
    /// A button was pressed (`BTN_*`).
    Press(u32),
    /// A button was released (`BTN_*`).
    Release(u32),
    /// The pointer left the surface.
    Leave,
}

/// One pointer event addressed to a shell surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    /// Layer-surface namespace the event arrived on.
    pub surface: &'static str,
    /// Surface-local logical position.
    pub local: (f64, f64),
    /// Where that surface sits in the composed window (logical pixels).
    pub placement: (i32, i32),
    pub kind: PointerKind,
}

impl PointerEvent {
    /// Position in the composed window Slint lays out.
    pub fn window(&self) -> (f64, f64) {
        (
            self.placement.0 as f64 + self.local.0,
            self.placement.1 as f64 + self.local.1,
        )
    }

    /// `AETHER_SHELL_POINTER: surface=<ns> x=<n> y=<n> button=<n|none>` plus
    /// the press/release state and the window position.
    pub fn line(&self) -> String {
        let (button, state) = match self.kind {
            PointerKind::Enter => ("none".to_string(), "enter"),
            PointerKind::Motion => ("none".to_string(), "motion"),
            PointerKind::Press(button) => (button.to_string(), "press"),
            PointerKind::Release(button) => (button.to_string(), "release"),
            PointerKind::Leave => ("none".to_string(), "leave"),
        };
        let (x, y) = self.window();
        format!(
            "AETHER_SHELL_POINTER: surface={} x={} y={} button={button} state={state} window={},{}",
            self.surface, self.local.0 as i32, self.local.1 as i32, x as i32, y as i32
        )
    }
}

/// Slint's button for a Linux input button code.
pub fn slint_button(button: u32) -> PointerEventButton {
    match button {
        BTN_RIGHT => PointerEventButton::Right,
        BTN_MIDDLE => PointerEventButton::Middle,
        _ => PointerEventButton::Left,
    }
}

/// Routes pointer events into the composed Slint scene and remembers what the
/// pointer is over, so a release after leaving a widget does not click it.
#[derive(Debug, Default)]
pub struct PointerRouter {
    /// Surface the pointer is currently on, and its window position.
    last: Option<(&'static str, (f64, f64))>,
}

impl PointerRouter {
    pub fn new() -> PointerRouter {
        PointerRouter::default()
    }

    /// Deliver one event to the scene; returns the marker line to publish.
    pub fn route(&mut self, ui: &ShellRoot, event: PointerEvent) -> String {
        let (x, y) = event.window();
        let position = LogicalPosition::new(x as f32, y as f32);
        match event.kind {
            PointerKind::Enter | PointerKind::Motion => {
                self.last = Some((event.surface, (x, y)));
                ui.window()
                    .dispatch_event(WindowEvent::PointerMoved { position });
            }
            PointerKind::Press(code) => {
                self.last = Some((event.surface, (x, y)));
                ui.window().dispatch_event(WindowEvent::PointerPressed {
                    position,
                    button: slint_button(code),
                });
            }
            PointerKind::Release(code) => {
                // Slint only treats it as a click when the release lands on the
                // widget that was pressed; moving away in between cancels it.
                ui.window().dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: slint_button(code),
                });
            }
            PointerKind::Leave => {
                self.last = None;
                ui.window().dispatch_event(WindowEvent::PointerExited);
            }
        }
        event.line()
    }

    /// What the pointer is over, if anything (`AETHER_SHELL_POINTER_STATE`).
    pub fn current(&self) -> Option<(&'static str, (f64, f64))> {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: PointerKind) -> PointerEvent {
        PointerEvent {
            surface: "aether-shell-dock",
            local: (261.0, 32.0),
            placement: (600, 1016),
            kind,
        }
    }

    #[test]
    fn surface_local_coordinates_become_window_coordinates() {
        // The dock sits at 600,1016 on a 1920x1080 output; a tile at 261,32 in
        // the surface is at 861,1048 in the composed window.
        let moved = event(PointerKind::Motion);
        assert_eq!(moved.window(), (861.0, 1048.0));
        assert_eq!(
            moved.line(),
            "AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=none \
             state=motion window=861,1048"
        );
        assert_eq!(
            event(PointerKind::Press(BTN_LEFT)).line(),
            "AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=272 \
             state=press window=861,1048"
        );
        assert!(event(PointerKind::Leave).line().starts_with(
            "AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=none state=leave"
        ));
    }

    #[test]
    fn buttons_map_to_slint_buttons() {
        assert_eq!(slint_button(BTN_LEFT), PointerEventButton::Left);
        assert_eq!(slint_button(BTN_RIGHT), PointerEventButton::Right);
        assert_eq!(slint_button(BTN_MIDDLE), PointerEventButton::Middle);
        assert_eq!(slint_button(0x999), PointerEventButton::Left);
    }
}
