//! Keyboard navigation model (F1 SHL-18, design D1 section 9).
//!
//! Every shell surface is reachable without a pointer. The rules are pure data
//! so they are unit-tested without a display server; [`crate::app`] maps the
//! focused (surface, index) pair onto the same handlers a click uses.
//!
//! Tab and Shift+Tab move between the five surfaces (menu bar, dock, launcher,
//! control centre, notifications); the arrow keys move inside one surface and
//! wrap; Enter/Space activate the focused item; Escape closes the overlay. The
//! focus ring itself is drawn by the Slint surfaces (2 px accent outline).

use crate::model::ShellState;

/// Surfaces with keyboard navigation. The codes are shared with the Slint side
/// (`focus-surface` in `ShellRoot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Menubar,
    Dock,
    Launcher,
    ControlCenter,
    Notifications,
}

impl Surface {
    pub const ALL: [Surface; 5] = [
        Surface::Menubar,
        Surface::Dock,
        Surface::Launcher,
        Surface::ControlCenter,
        Surface::Notifications,
    ];

    pub fn code(self) -> i32 {
        match self {
            Surface::Menubar => 0,
            Surface::Dock => 1,
            Surface::Launcher => 2,
            Surface::ControlCenter => 3,
            Surface::Notifications => 4,
        }
    }

    pub fn from_code(code: i32) -> Surface {
        *Surface::ALL
            .iter()
            .find(|surface| surface.code() == code)
            .unwrap_or(&Surface::Menubar)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Menubar => "menubar",
            Surface::Dock => "dock",
            Surface::Launcher => "launcher",
            Surface::ControlCenter => "control-center",
            Surface::Notifications => "notifications",
        }
    }
}

/// Tray items in the menu bar, in focus order after the menu titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tray {
    Wifi,
    Bluetooth,
    Sound,
    Battery,
    Notifications,
    Clock,
}

impl Tray {
    pub const ALL: [Tray; 6] = [
        Tray::Wifi,
        Tray::Bluetooth,
        Tray::Sound,
        Tray::Battery,
        Tray::Notifications,
        Tray::Clock,
    ];
}

/// Index of the AetherOS (logo) menu in the menu bar focus order.
pub const MENU_LOGO: usize = 0;

/// First index of the tray icons given the number of menu titles.
pub fn tray_start(menus: usize) -> usize {
    1 + menus
}

/// Focus index of one tray icon.
pub fn tray_index(menus: usize, tray: Tray) -> usize {
    let offset = match tray {
        Tray::Wifi => 0,
        Tray::Bluetooth => 1,
        Tray::Sound => 2,
        Tray::Battery => 3,
        Tray::Notifications => 4,
        Tray::Clock => 5,
    };
    tray_start(menus) + offset
}

/// Menu bar focusable count: logo + menu titles + six tray items.
pub fn menubar_count(menus: usize) -> usize {
    1 + menus + Tray::ALL.len()
}

/// Control centre focus order: four quick tiles, two sliders, the power and
/// effects segments, Settings and Lock.
pub const CONTROL_CENTER_COUNT: usize = 10;

/// Control centre item indices (shared with `shell/src/app.rs`).
pub mod control_center {
    pub const WIFI: usize = 0;
    pub const BLUETOOTH: usize = 1;
    pub const FOCUS: usize = 2;
    pub const NIGHT_LIGHT: usize = 3;
    pub const VOLUME: usize = 4;
    pub const BRIGHTNESS: usize = 5;
    pub const POWER_MODE: usize = 6;
    pub const EFFECTS: usize = 7;
    pub const SETTINGS: usize = 8;
    pub const LOCK: usize = 9;
}

/// Notification centre focus order: Clear all, one card per visible
/// notification, then Focus settings.
pub const NOTIFICATIONS_CLEAR_ALL: usize = 0;

pub fn notification_card(index: usize) -> usize {
    index + 1
}

/// Number of focusables in one surface for the current state.
pub fn count(state: &ShellState, menus: usize, surface: Surface) -> usize {
    match surface {
        Surface::Menubar => menubar_count(menus),
        Surface::Dock => state.dock_apps().len(),
        Surface::Launcher => state.search_results.len(),
        Surface::ControlCenter => CONTROL_CENTER_COUNT,
        Surface::Notifications => 2 + state.visible_notifications().len(),
    }
}

/// Every surface with its focusable count, in Tab order.
pub fn counts(state: &ShellState, menus: usize) -> Vec<(Surface, usize)> {
    Surface::ALL
        .iter()
        .map(|surface| (*surface, count(state, menus, *surface)))
        .collect()
}

/// Which item owns keyboard focus on which surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    pub surface: Surface,
    pub index: usize,
    /// False until the first keyboard step: an idle shell draws no ring.
    pub visible: bool,
}

impl Default for Focus {
    fn default() -> Self {
        Focus {
            surface: Surface::Menubar,
            index: MENU_LOGO,
            visible: false,
        }
    }
}

impl Focus {
    /// Move inside the current surface, wrapping at both ends. A surface with
    /// no focusables keeps index 0.
    pub fn move_within(&mut self, count: usize, delta: i32) {
        self.visible = true;
        if count == 0 {
            self.index = 0;
            return;
        }
        let len = count as i32;
        self.index = (self.index as i32 + delta).rem_euclid(len) as usize;
    }

    /// Tab / Shift+Tab across surfaces, skipping empty ones (a launcher with no
    /// results, a dock with no apps) so focus never lands on nothing.
    pub fn cycle(&mut self, counts: &[(Surface, usize)], step: i32) {
        self.visible = true;
        if counts.is_empty() {
            return;
        }
        let len = counts.len() as i32;
        let start = counts
            .iter()
            .position(|(surface, _)| *surface == self.surface)
            .unwrap_or(0) as i32;
        for offset in 1..=len {
            let candidate = (start + step.signum() * offset).rem_euclid(len);
            let (surface, count) = counts[candidate as usize];
            if count > 0 {
                self.surface = surface;
                self.index = 0;
                return;
            }
        }
    }

    /// Keep the index inside the current surface after its list changed.
    pub fn clamp(&mut self, counts: &[(Surface, usize)]) {
        let count = counts
            .iter()
            .find(|(surface, _)| *surface == self.surface)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        if count == 0 {
            self.index = 0;
        } else if self.index >= count {
            self.index = count - 1;
        }
    }

    /// Move focus to a surface and its first item (an overlay opened by a
    /// shortcut or a click).
    pub fn focus_surface(&mut self, surface: Surface) {
        self.surface = surface;
        self.index = 0;
        self.visible = true;
    }

    /// `AETHER_SHELL_FOCUS` marker for `--a11y-status` and the host log.
    pub fn line(&self) -> String {
        format!(
            "AETHER_SHELL_FOCUS: surface={} index={} visible={}",
            self.surface.as_str(),
            self.index,
            self.visible
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{App, Overlay};
    use crate::IconKind;

    fn state_with(apps: usize, results: usize, notifications: usize) -> ShellState {
        ShellState {
            apps: (0..apps)
                .map(|index| App {
                    id: format!("app{index}"),
                    name: format!("App {index}"),
                    icon: IconKind::Folder,
                    exec: "true".into(),
                    running: false,
                    pinned: true,
                    badge: 0,
                })
                .collect(),
            search_results: (0..results)
                .map(|index| crate::model::SearchHit {
                    id: format!("hit{index}"),
                    title: format!("Hit {index}"),
                    subtitle: String::new(),
                    icon: IconKind::File,
                    kind: crate::model::SearchKind::App,
                    score: 0,
                })
                .collect(),
            notifications: (0..notifications)
                .map(|index| crate::model::Notification {
                    id: format!("n{index}"),
                    app: "Mail".into(),
                    title: format!("Message {index}"),
                    body: String::new(),
                    time: "now".into(),
                    icon: IconKind::Bell,
                    unread: true,
                })
                .collect(),
            overlay: Overlay::None,
            ..ShellState::default()
        }
    }

    #[test]
    fn counts_cover_every_shell_surface() {
        let state = state_with(3, 2, 4);
        let counts = counts(&state, 5);
        assert_eq!(
            counts,
            vec![
                (Surface::Menubar, 1 + 5 + 6),
                (Surface::Dock, 3),
                (Surface::Launcher, 2),
                (Surface::ControlCenter, CONTROL_CENTER_COUNT),
                (Surface::Notifications, 2 + 4),
            ]
        );
        assert_eq!(tray_index(5, Tray::Wifi), 6);
        assert_eq!(tray_index(5, Tray::Clock), 11);
        assert_eq!(notification_card(0), 1);
    }

    #[test]
    fn arrows_wrap_inside_a_surface() {
        let mut focus = Focus::default();
        assert!(!focus.visible);
        focus.move_within(3, 1);
        assert_eq!((focus.index, focus.visible), (1, true));
        focus.move_within(3, 1);
        focus.move_within(3, 1);
        assert_eq!(focus.index, 0, "wraps forward");
        focus.move_within(3, -1);
        assert_eq!(focus.index, 2, "wraps backward");
        focus.move_within(0, 1);
        assert_eq!(focus.index, 0, "empty surface keeps index 0");
    }

    #[test]
    fn tab_cycles_surfaces_and_skips_empty_ones() {
        let state = state_with(0, 0, 0);
        let counts = counts(&state, 5);
        let mut focus = Focus::default();
        focus.cycle(&counts, 1);
        assert_eq!(focus.surface, Surface::ControlCenter, "dock and launcher are empty");
        focus.cycle(&counts, 1);
        assert_eq!(focus.surface, Surface::Notifications, "empty cards still have two actions");
        focus.cycle(&counts, 1);
        assert_eq!(focus.surface, Surface::Menubar);
        focus.cycle(&counts, -1);
        assert_eq!(focus.surface, Surface::Notifications, "Shift+Tab reverses");
        assert!(focus.visible);
    }

    #[test]
    fn focus_clamps_when_a_list_shrinks() {
        let state = state_with(2, 2, 1);
        let mut focus = Focus {
            surface: Surface::Dock,
            index: 1,
            visible: true,
        };
        focus.clamp(&counts(&state, 5));
        assert_eq!(focus.index, 1);
        let empty = state_with(0, 0, 0);
        focus.clamp(&counts(&empty, 5));
        assert_eq!(focus.index, 0);
        assert!(focus.line().starts_with("AETHER_SHELL_FOCUS: surface=dock index=0"));
    }

    #[test]
    fn surface_and_tray_codes_round_trip() {
        for surface in Surface::ALL {
            assert_eq!(Surface::from_code(surface.code()), surface);
            assert_eq!(Surface::from_code(surface.code() + 10), Surface::Menubar);
        }
        assert_eq!(MENU_LOGO, 0);
        assert_eq!(menubar_count(0), 7);
        assert_eq!(tray_start(5), 6);
    }
}
