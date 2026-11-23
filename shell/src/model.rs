//! Shell state and rules (P4, S3).
//!
//! Everything the shell decides lives here as plain Rust so it can be unit
//! tested without a display server: dock ordering, unread badges, Focus
//! suppression, power modes, launcher ranking and lock-screen verification.
//! `crate::app` is the only place that touches Slint properties.

use crate::IconKind;

/// Overlay surfaces. Codes are shared with the `.slint` side (`overlay`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Launcher,
    ControlCenter,
    Notifications,
    Overview,
    Settings,
    Lock,
}

impl Overlay {
    pub const ALL: [Overlay; 7] = [
        Overlay::None,
        Overlay::Launcher,
        Overlay::ControlCenter,
        Overlay::Notifications,
        Overlay::Overview,
        Overlay::Settings,
        Overlay::Lock,
    ];

    pub fn code(self) -> i32 {
        match self {
            Overlay::None => 0,
            Overlay::Launcher => 1,
            Overlay::ControlCenter => 2,
            Overlay::Notifications => 3,
            Overlay::Overview => 4,
            Overlay::Settings => 5,
            Overlay::Lock => 6,
        }
    }

    pub fn from_code(code: i32) -> Overlay {
        *Overlay::ALL
            .iter()
            .find(|overlay| overlay.code() == code)
            .unwrap_or(&Overlay::None)
    }

    /// Clicking the same trigger again closes the surface (toggle behavior).
    pub fn toggle(self, requested: Overlay) -> Overlay {
        if self == requested {
            Overlay::None
        } else {
            requested
        }
    }

    /// True when the surface is a panel anchored under the menu bar.
    pub fn is_anchored_panel(self) -> bool {
        matches!(
            self,
            Overlay::ControlCenter | Overlay::Notifications
        )
    }
}

/// Power profile (F7 / power-profiles-daemon names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    Saver,
    Balanced,
    Performance,
}

impl PowerMode {
    pub const ALL: [PowerMode; 3] = [PowerMode::Saver, PowerMode::Balanced, PowerMode::Performance];

    pub fn as_str(self) -> &'static str {
        match self {
            PowerMode::Saver => "saver",
            PowerMode::Balanced => "balanced",
            PowerMode::Performance => "performance",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PowerMode::Saver => "Power saver",
            PowerMode::Balanced => "Balanced",
            PowerMode::Performance => "Performance",
        }
    }

    pub fn parse(value: &str) -> Option<PowerMode> {
        match value.trim().to_ascii_lowercase().as_str() {
            "saver" | "power-saver" | "power_saver" => Some(PowerMode::Saver),
            "balanced" | "default" => Some(PowerMode::Balanced),
            "performance" => Some(PowerMode::Performance),
            _ => None,
        }
    }

    pub fn next(self) -> PowerMode {
        match self {
            PowerMode::Saver => PowerMode::Balanced,
            PowerMode::Balanced => PowerMode::Performance,
            PowerMode::Performance => PowerMode::Saver,
        }
    }
}

impl Default for PowerMode {
    fn default() -> Self {
        PowerMode::Balanced
    }
}

/// A launchable app (from a `.desktop` entry or a built-in default app).
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: IconKind,
    pub exec: String,
    pub running: bool,
    pub pinned: bool,
    pub badge: u32,
}

/// One notification card.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: String,
    pub app: String,
    pub title: String,
    pub body: String,
    pub time: String,
    pub icon: IconKind,
    pub unread: bool,
}

/// One window in the overview.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub app: String,
    pub icon: IconKind,
    pub workspace: usize,
}

/// Result of a launcher query.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub icon: IconKind,
    pub kind: SearchKind,
    pub score: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    App,
    Settings,
    Calculation,
    File,
}

impl SearchKind {
    pub fn label(self) -> &'static str {
        match self {
            SearchKind::App => "App",
            SearchKind::Settings => "Settings",
            SearchKind::Calculation => "= 1 + 1",
            SearchKind::File => "File",
        }
    }
}

/// Lock-screen credential check. The password is not stored in clear text in
/// the shipped config; this prototype keeps a salted hash in the shell state
/// and compares it in constant time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    salt: String,
    digest: String,
}

impl Credential {
    pub fn new(salt: &str, password: &str) -> Credential {
        Credential {
            salt: salt.to_string(),
            digest: hash(salt, password),
        }
    }

    pub fn verify(&self, password: &str) -> bool {
        constant_time_eq(&hash(&self.salt, password), &self.digest)
    }
}

/// FNV-1a based digest. Real authentication moves to PAM/greetd (P4-T06b); this
/// keeps the prototype dependency-free while never comparing in variable time.
fn hash(salt: &str, password: &str) -> String {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in salt.bytes().chain([b':']).chain(password.bytes()) {
        state ^= byte as u64;
        state = state.wrapping_mul(0x100_0000_01b3);
    }
    format!("{state:016x}")
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// A daily Focus window (F1 SHL-05). Minutes are since midnight; a window that
/// wraps past midnight (for example 22:00 to 07:00) is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusSchedule {
    pub start_minute: u16,
    pub end_minute: u16,
}

impl FocusSchedule {
    pub fn new(start_minute: u16, end_minute: u16) -> FocusSchedule {
        FocusSchedule {
            start_minute: start_minute % 1440,
            end_minute: end_minute % 1440,
        }
    }

    pub fn active_at(&self, minute_of_day: u16) -> bool {
        let now = minute_of_day % 1440;
        if self.start_minute == self.end_minute {
            return false;
        }
        if self.start_minute < self.end_minute {
            (self.start_minute..self.end_minute).contains(&now)
        } else {
            now >= self.start_minute || now < self.end_minute
        }
    }

    pub fn label(&self) -> String {
        format!(
            "{:02}:{:02}–{:02}:{:02}",
            self.start_minute / 60,
            self.start_minute % 60,
            self.end_minute / 60,
            self.end_minute % 60
        )
    }
}

/// The whole shell model. Only `crate::app` mutates it in response to UI
/// callbacks; tests drive it directly.
#[derive(Debug, Clone)]
pub struct ShellState {
    pub active_app: String,
    pub clock: String,
    pub date_label: String,
    pub wifi: bool,
    pub bluetooth: bool,
    pub focus_until: Option<String>,
    pub focus_schedule: Option<FocusSchedule>,
    /// Minutes since midnight; refreshed by the host's minute timer.
    pub now_minute: u16,
    pub night_light: bool,
    pub volume: f32,
    pub brightness: f32,
    pub battery: u8,
    pub charging: bool,
    pub power_mode: PowerMode,
    pub apps: Vec<App>,
    pub notifications: Vec<Notification>,
    pub windows: Vec<Window>,
    pub overlay: Overlay,
    pub search_query: String,
    pub search_selected: usize,
    pub search_results: Vec<SearchHit>,
    pub workspace: usize,
    pub settings_page: usize,
    pub settings_rows: Vec<crate::config::SettingRow>,
    pub lock_error: String,
    pub unlocking: bool,
    pub dock_auto_hide: bool,
    pub credential: Credential,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            active_app: "AetherOS".to_string(),
            clock: "09:41".to_string(),
            date_label: "Monday, 23 November".to_string(),
            wifi: true,
            bluetooth: true,
            focus_until: None,
            focus_schedule: None,
            now_minute: 9 * 60 + 41,
            night_light: false,
            volume: 0.62,
            brightness: 0.8,
            battery: 84,
            charging: false,
            power_mode: PowerMode::Balanced,
            apps: Vec::new(),
            notifications: Vec::new(),
            windows: Vec::new(),
            overlay: Overlay::None,
            search_query: String::new(),
            search_selected: 0,
            search_results: Vec::new(),
            workspace: 0,
            settings_page: 0,
            settings_rows: Vec::new(),
            lock_error: String::new(),
            unlocking: false,
            dock_auto_hide: false,
            credential: Credential::new("aether-demo", "aether"),
        }
    }
}

impl ShellState {
    /// Apps shown in the dock: pinned first (config order), then running apps
    /// in launch order, without duplicates (F1 SHL-02).
    pub fn dock_apps(&self) -> Vec<&App> {
        let mut ordered: Vec<&App> = Vec::new();
        for app in &self.apps {
            if app.pinned {
                ordered.push(app);
            }
        }
        for app in &self.apps {
            if app.running && !app.pinned {
                ordered.push(app);
            }
        }
        ordered
    }

    /// Notification centres collapse when Focus is scheduled (F1 SHL-05).
    pub fn visible_notifications(&self) -> Vec<&Notification> {
        if self.focus_active() {
            Vec::new()
        } else {
            self.notifications.iter().collect()
        }
    }

    pub fn focus_active(&self) -> bool {
        if self.focus_until.is_some() {
            return true;
        }
        self.focus_schedule
            .is_some_and(|schedule| schedule.active_at(self.now_minute))
    }

    /// Label for the Focus banner: the manual end time or the schedule.
    pub fn focus_label(&self) -> Option<String> {
        if let Some(until) = &self.focus_until {
            return Some(until.clone());
        }
        self.focus_schedule.map(|schedule| schedule.label())
    }

    pub fn unread_count(&self) -> usize {
        if self.focus_active() {
            0
        } else {
            self.notifications.iter().filter(|n| n.unread).count()
        }
    }

    /// Menu-bar badge text ("" hides the badge, "9+" caps large counts).
    pub fn unread_badge(&self) -> String {
        match self.unread_count() {
            0 => String::new(),
            1..=9 => self.unread_count().to_string(),
            _ => "9+".to_string(),
        }
    }

    pub fn battery_label(&self) -> String {
        if self.charging {
            format!("{}% · charging", self.battery)
        } else {
            format!("{}%", self.battery)
        }
    }

    pub fn battery_is_low(&self) -> bool {
        self.battery < 20 && !self.charging
    }

    /// Dock visibility: auto-hide keeps a 6 px reveal strip.
    pub fn dock_visible(&self, hovered: bool) -> bool {
        !self.dock_auto_hide || hovered
    }

    pub fn set_overlay(&mut self, overlay: Overlay) {
        if overlay == Overlay::Launcher {
            self.search_selected = 0;
        }
        self.overlay = overlay;
    }

    pub fn toggle_overlay(&mut self, requested: Overlay) {
        self.set_overlay(self.overlay.toggle(requested));
    }

    pub fn set_search_query(&mut self, query: &str, settings: &[crate::config::SettingDef]) {
        self.search_query = query.to_string();
        self.search_results = crate::search::search(query, &self.apps, settings);
        self.search_selected = 0;
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.search_results.is_empty() {
            self.search_selected = 0;
            return;
        }
        let len = self.search_results.len() as i32;
        let next = (self.search_selected as i32 + delta).rem_euclid(len);
        self.search_selected = next as usize;
    }

    /// App id at the current launcher selection, if any.
    pub fn selected_result(&self) -> Option<&SearchHit> {
        self.search_results.get(self.search_selected)
    }

    pub fn launch(&mut self, id: &str) {
        let app = self.apps.iter_mut().find(|app| app.id == id);
        if let Some(app) = app {
            app.running = true;
            self.active_app = app.name.clone();
        }
        self.set_search_query("", &[]);
        self.overlay = Overlay::None;
    }

    pub fn close_app(&mut self, id: &str) {
        if let Some(app) = self.apps.iter_mut().find(|app| app.id == id) {
            app.running = false;
            app.badge = 0;
        }
        self.windows.retain(|window| window.app != id);
        if !self.apps.iter().any(|app| app.running) {
            self.active_app = "AetherOS".to_string();
        }
    }

    /// Clicking a dock icon launches the app or focuses an existing window.
    pub fn activate_from_dock(&mut self, id: &str) {
        if let Some(window) = self.windows.iter().find(|window| window.app == id) {
            let workspace = window.workspace;
            self.workspace = workspace;
            if let Some(app) = self.apps.iter_mut().find(|app| app.id == id) {
                app.running = true;
                self.active_app = app.name.clone();
            }
            return;
        }
        self.launch(id);
    }

    pub fn dismiss_notification(&mut self, id: &str) {
        self.notifications.retain(|notification| notification.id != id);
    }

    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
    }

    /// Quick-toggle handler used by the menu bar and the control centre.
    pub fn set_toggle(&mut self, id: &str, value: bool) {
        match id {
            "wifi" => self.wifi = value,
            "bluetooth" => self.bluetooth = value,
            "focus" => {
                self.focus_until = if value { Some("18:00".to_string()) } else { None };
            }
            "night-light" => self.night_light = value,
            _ => {}
        }
    }

    pub fn set_volume(&mut self, value: f32) {
        self.volume = clamp_unit(value);
    }

    pub fn set_brightness(&mut self, value: f32) {
        self.brightness = clamp_unit(value);
    }

    /// Lock screen: wrong passwords are rejected and cleared (F1 SHL-AC-08).
    pub fn submit_unlock(&mut self, password: &str) -> bool {
        if self.credential.verify(password) {
            self.unlocking = true;
            self.lock_error.clear();
            self.overlay = Overlay::None;
            true
        } else {
            self.unlocking = false;
            self.lock_error = "Incorrect password — try again".to_string();
            false
        }
    }
}

pub fn clamp_unit(value: f32) -> f32 {
    if value.is_nan() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

/// Clock and date strings for the menu bar and lock screen.
pub fn format_clock(now: &chrono::DateTime<chrono::Local>) -> String {
    now.format("%H:%M").to_string()
}

pub fn format_date(now: &chrono::DateTime<chrono::Local>) -> String {
    now.format("%A, %-d %B").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, running: bool, pinned: bool) -> App {
        App {
            id: id.to_string(),
            name: name.to_string(),
            icon: IconKind::Folder,
            exec: format!("/usr/bin/{id}"),
            running,
            pinned,
            badge: 0,
        }
    }

    #[test]
    fn dock_lists_pinned_then_running_without_duplicates() {
        let mut state = ShellState::default();
        state.apps = vec![
            app("terminal", "Terminal", true, true),
            app("notes", "Notes", false, false),
            app("files", "Files", true, false),
        ];
        let ids: Vec<&str> = state.dock_apps().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["terminal", "files"]);
    }

    #[test]
    fn focus_suppresses_notifications_and_badge() {
        let mut state = ShellState::default();
        state.notifications.push(Notification {
            id: "n1".into(),
            app: "Calendar".into(),
            title: "Stand-up in 5 minutes".into(),
            body: "Daily sync".into(),
            time: "09:55".into(),
            icon: IconKind::Calendar,
            unread: true,
        });
        assert_eq!(state.unread_badge(), "1");
        state.set_toggle("focus", true);
        assert_eq!(state.unread_badge(), "");
        assert!(state.visible_notifications().is_empty());
        state.set_toggle("focus", false);
        assert_eq!(state.visible_notifications().len(), 1);
        assert_eq!(state.notifications.len(), 1, "focus hides, never deletes");
    }

    #[test]
    fn badge_caps_at_nine_plus() {
        let mut state = ShellState::default();
        for index in 0..12 {
            state.notifications.push(Notification {
                id: format!("n{index}"),
                app: "Mail".into(),
                title: "New message".into(),
                body: String::new(),
                time: "now".into(),
                icon: IconKind::Bell,
                unread: true,
            });
        }
        assert_eq!(state.unread_badge(), "9+");
    }

    #[test]
    fn focus_schedule_handles_daytime_and_overnight_windows() {
        let work = FocusSchedule::new(9 * 60, 17 * 60);
        assert!(work.active_at(12 * 60));
        assert!(!work.active_at(8 * 60 + 59));
        assert!(!work.active_at(17 * 60));
        let night = FocusSchedule::new(22 * 60, 7 * 60);
        assert!(night.active_at(23 * 60));
        assert!(night.active_at(6 * 60));
        assert!(!night.active_at(12 * 60));
        assert!(!FocusSchedule::new(60, 60).active_at(60), "empty window is never active");
        assert_eq!(work.label(), "09:00–17:00");
    }

    #[test]
    fn scheduled_focus_suppresses_without_a_manual_toggle() {
        let mut state = ShellState::default();
        state.notifications.push(Notification {
            id: "n1".into(),
            app: "Mail".into(),
            title: "Hello".into(),
            body: String::new(),
            time: "now".into(),
            icon: IconKind::Bell,
            unread: true,
        });
        state.now_minute = 10 * 60;
        state.focus_schedule = Some(FocusSchedule::new(9 * 60, 12 * 60));
        assert!(state.focus_active());
        assert!(state.visible_notifications().is_empty());
        assert_eq!(state.unread_badge(), "");
        assert_eq!(state.focus_label().as_deref(), Some("09:00–12:00"));
        state.now_minute = 12 * 60;
        assert!(!state.focus_active());
        assert_eq!(state.visible_notifications().len(), 1);
    }

    #[test]
    fn dock_auto_hide_keeps_reveal_strip() {
        let mut state = ShellState::default();
        assert!(state.dock_visible(false));
        state.dock_auto_hide = true;
        assert!(!state.dock_visible(false));
        assert!(state.dock_visible(true));
    }

    #[test]
    fn overlay_toggle_closes_the_same_surface() {
        let mut state = ShellState::default();
        state.toggle_overlay(Overlay::Launcher);
        assert_eq!(state.overlay, Overlay::Launcher);
        state.toggle_overlay(Overlay::Launcher);
        assert_eq!(state.overlay, Overlay::None);
        state.toggle_overlay(Overlay::ControlCenter);
        state.toggle_overlay(Overlay::Notifications);
        assert_eq!(state.overlay, Overlay::Notifications);
    }

    #[test]
    fn launching_marks_running_and_focuses_name() {
        let mut state = ShellState::default();
        state.apps = vec![app("notes", "AetherPad", false, false)];
        state.launch("notes");
        assert!(state.apps[0].running);
        assert_eq!(state.active_app, "AetherPad");
        assert_eq!(state.overlay, Overlay::None);
    }

    #[test]
    fn closing_the_last_app_resets_the_menu_bar_title() {
        let mut state = ShellState::default();
        state.apps = vec![app("notes", "AetherPad", true, false)];
        state.windows.push(Window {
            id: "w1".into(),
            title: "Notes".into(),
            app: "notes".into(),
            icon: IconKind::Document,
            workspace: 0,
        });
        state.close_app("notes");
        assert!(!state.apps[0].running);
        assert!(state.windows.is_empty());
        assert_eq!(state.active_app, "AetherOS");
    }

    #[test]
    fn dock_activation_focuses_existing_window_workspace() {
        let mut state = ShellState::default();
        state.apps = vec![app("notes", "AetherPad", true, false)];
        state.windows.push(Window {
            id: "w1".into(),
            title: "Notes".into(),
            app: "notes".into(),
            icon: IconKind::Document,
            workspace: 2,
        });
        state.activate_from_dock("notes");
        assert_eq!(state.workspace, 2);
        assert_eq!(state.active_app, "AetherPad");
    }

    #[test]
    fn wrong_password_is_rejected_and_cleared_state_recovers() {
        let mut state = ShellState::default();
        state.overlay = Overlay::Lock;
        assert!(!state.submit_unlock("nope"));
        assert!(state.lock_error.contains("Incorrect"));
        assert_eq!(state.overlay, Overlay::Lock);
        assert!(state.submit_unlock("aether"));
        assert!(state.unlocking);
        assert!(state.lock_error.is_empty());
        assert_eq!(state.overlay, Overlay::None);
    }

    #[test]
    fn credentials_are_salted() {
        let a = Credential::new("salt-a", "aether");
        let b = Credential::new("salt-b", "aether");
        assert_ne!(a, b);
        assert!(a.verify("aether"));
        assert!(!b.verify("aether "));
    }

    #[test]
    fn sliders_clamp_to_unit_interval() {
        let mut state = ShellState::default();
        state.set_volume(1.7);
        assert_eq!(state.volume, 1.0);
        state.set_brightness(-3.0);
        assert_eq!(state.brightness, 0.0);
        state.set_brightness(f32::NAN);
        assert_eq!(state.brightness, 0.0);
    }

    #[test]
    fn power_modes_round_trip_and_cycle() {
        for mode in PowerMode::ALL {
            assert_eq!(PowerMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(PowerMode::Balanced.next(), PowerMode::Performance);
        assert_eq!(PowerMode::Performance.next(), PowerMode::Saver);
        assert_eq!(PowerMode::parse("unknown"), None);
    }

    #[test]
    fn overlay_codes_round_trip() {
        for overlay in Overlay::ALL {
            assert_eq!(Overlay::from_code(overlay.code()), overlay);
        }
        assert_eq!(Overlay::from_code(99), Overlay::None);
    }

    #[test]
    fn clock_and_date_format() {
        use chrono::TimeZone;
        let now = chrono::Local.with_ymd_and_hms(2026, 11, 23, 9, 5, 0).unwrap();
        assert_eq!(format_clock(&now), "09:05");
        assert_eq!(format_date(&now), "Monday, 23 November");
    }

    #[test]
    fn launcher_selection_wraps() {
        let mut state = ShellState::default();
        state.search_results.push(SearchHit {
            id: "a".into(),
            title: "A".into(),
            subtitle: String::new(),
            icon: IconKind::Folder,
            kind: SearchKind::App,
            score: 0,
        });
        state.move_selection(-1);
        assert_eq!(state.search_selected, 0);
        state.search_results.push(SearchHit {
            id: "b".into(),
            title: "B".into(),
            subtitle: String::new(),
            icon: IconKind::Folder,
            kind: SearchKind::App,
            score: 1,
        });
        state.move_selection(1);
        assert_eq!(state.search_selected, 1);
        state.move_selection(1);
        assert_eq!(state.search_selected, 0);
    }
}
