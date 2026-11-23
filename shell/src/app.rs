//! Shell controller: the only bridge between the pure model and Slint (P4, S3).
//!
//! `ShellController` owns the state, wires every UI callback, applies system
//! service changes and persists settings through F4 files. Tests construct it
//! with `MockServices` and a temporary config root, then drive it through the
//! headless element queries.

use std::cell::RefCell;
use std::rc::Rc;

use crate::IconKind;
use slint::{Model, ModelRc, VecModel};

use crate::config::{self, Category, SettingKind, SettingsStore};
use crate::desktop;
use crate::model::{self, Notification, Overlay, PowerMode, SearchKind, ShellState, Window};
use crate::services::{Battery, Services};
use crate::{
    AppEntry, MenuEntry, MenuGroup, NotificationEntry, SearchResultEntry,
    SettingsCategoryEntry, SettingsRowEntry, ShellRoot, WindowEntry,
};

/// Menu bar menus: the AetherOS menu plus the active app's menus (F1 SHL-01).
fn menus_for() -> Vec<MenuGroup> {
    let item = |label: &str, shortcut: &str, enabled: bool| MenuEntry {
        label: label.into(),
        shortcut: shortcut.into(),
        enabled,
    };
    let groups: Vec<(&str, Vec<MenuEntry>)> = vec![
        ("AetherOS", vec![
            item("About AetherOS", "", true),
            item("Settings…", "⌘,", true),
            item("", "", false),
            item("Lock screen", "⌃⌘Q", true),
            item("Sleep", "", true),
            item("Restart…", "", true),
            item("Shut down…", "", true),
        ]),
        ("File", vec![
            item("New window", "⌘N", true),
            item("Open…", "⌘O", true),
            item("Save", "⌘S", true),
            item("Export as PDF…", "", true),
            item("", "", false),
            item("Close window", "⌘W", true),
        ]),
        ("Edit", vec![
            item("Undo", "⌘Z", true),
            item("Redo", "⇧⌘Z", true),
            item("", "", false),
            item("Cut", "⌘X", true),
            item("Copy", "⌘C", true),
            item("Paste", "⌘V", true),
            item("Find…", "⌘F", true),
        ]),
        ("View", vec![
            item("Zoom in", "⌘+", true),
            item("Zoom out", "⌘−", true),
            item("Actual size", "⌘0", true),
            item("", "", false),
            item("Enter full screen", "⌃⌘F", true),
        ]),
        ("Window", vec![
            item("Minimize", "⌘M", true),
            item("Zoom", "", true),
            item("Move to workspace", "", true),
            item("", "", false),
            item("Bring all to front", "", true),
        ]),
    ];
    groups
        .into_iter()
        .map(|(title, items)| MenuGroup {
            title: title.into(),
            items: ModelRc::new(VecModel::from(items)),
        })
        .collect()
}

/// Demo content for screenshots and for the first boot experience. The
/// production shell starts with empty notification and window lists.
pub fn demo_notifications() -> Vec<Notification> {
    vec![
        Notification {
            id: "n-cal".into(),
            app: "Calendar".into(),
            title: "Stand-up in 5 minutes".into(),
            body: "Daily sync · Design room".into(),
            time: "09:55".into(),
            icon: IconKind::Calendar,
            unread: true,
        },
        Notification {
            id: "n-files".into(),
            app: "Files".into(),
            title: "Copy finished".into(),
            body: "412 files copied to Pictures/2026".into(),
            time: "09:41".into(),
            icon: IconKind::Folder,
            unread: true,
        },
        Notification {
            id: "n-power".into(),
            app: "Power".into(),
            title: "Battery at 84%".into(),
            body: "About 6 h 10 m remaining on Balanced".into(),
            time: "09:20".into(),
            icon: IconKind::Battery,
            unread: false,
        },
    ]
}

pub fn demo_windows() -> Vec<Window> {
    vec![
        Window {
            id: "w-notes".into(),
            title: "Notes.md — AetherPad".into(),
            app: "aether-text".into(),
            icon: IconKind::Document,
            workspace: 0,
        },
        Window {
            id: "w-files".into(),
            title: "Home — Files".into(),
            app: "aether-files".into(),
            icon: IconKind::Folder,
            workspace: 0,
        },
        Window {
            id: "w-settings".into(),
            title: "Settings — Appearance".into(),
            app: "aether-settings".into(),
            icon: IconKind::Gear,
            workspace: 1,
        },
    ]
}

pub struct ShellController {
    state: RefCell<ShellState>,
    ui: ShellRoot,
    services: RefCell<Box<dyn Services>>,
    store: SettingsStore,
    /// Spawn app processes on launch. Off in tests and in the renderer.
    spawn_apps: RefCell<bool>,
    pub log: RefCell<Vec<String>>,
}

impl ShellController {
    pub fn new(
        ui: ShellRoot,
        services: Box<dyn Services>,
        store: SettingsStore,
    ) -> Rc<ShellController> {
        let mut state = ShellState::default();
        state.apps = desktop::merged_apps(desktop::scan_default());
        // Start with the real time; the host's minute timer keeps it fresh.
        let now = chrono::Local::now();
        state.clock = model::format_clock(&now);
        state.date_label = model::format_date(&now);

        let controller = Rc::new(ShellController {
            state: RefCell::new(state),
            ui,
            services: RefCell::new(services),
            store,
            spawn_apps: RefCell::new(false),
            log: RefCell::new(Vec::new()),
        });
        controller.load_settings();
        controller.wire();
        controller.sync();
        controller
    }

    /// Same as [`Self::new`] but with notification, window and settings demo
    /// content, used by the screenshot renderer.
    pub fn with_demo_content(
        ui: ShellRoot,
        services: Box<dyn Services>,
        store: SettingsStore,
    ) -> Rc<ShellController> {
        let controller = ShellController::new(ui, services, store);
        {
            let mut state = controller.state.borrow_mut();
            state.notifications = demo_notifications();
            state.windows = demo_windows();
            let open_apps: Vec<String> =
                state.windows.iter().map(|window| window.app.clone()).collect();
            for app in state.apps.iter_mut() {
                if open_apps.contains(&app.id) {
                    app.running = true;
                }
            }
            state.settings_page = 1;
            state.settings_rows = controller.store.rows_for(Category::Display);
            // Deterministic clock for reproducible evidence screenshots.
            state.clock = "09:41".to_string();
            state.date_label = "Monday, 23 November".to_string();
        }
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &ShellRoot {
        &self.ui
    }

    pub fn state(&self) -> std::cell::Ref<'_, ShellState> {
        self.state.borrow()
    }

    pub fn set_spawn_apps(&self, enabled: bool) {
        *self.spawn_apps.borrow_mut() = enabled;
    }

    pub fn store(&self) -> &SettingsStore {
        &self.store
    }

    /// Apply persisted settings to the live state at startup.
    fn load_settings(&self) {
        let mut state = self.state.borrow_mut();
        let read_bool = |key: &str, default: bool| {
            self.store
                .load(key)
                .ok()
                .flatten()
                .and_then(|value| value.as_bool())
                .unwrap_or(default)
        };
        let read_float = |key: &str, default: f32| {
            self.store
                .load(key)
                .ok()
                .flatten()
                .and_then(|value| match value {
                    toml::Value::Float(number) => Some(number as f32),
                    toml::Value::Integer(number) => Some(number as f32),
                    _ => None,
                })
                .unwrap_or(default)
        };
        state.wifi = read_bool("network.wifi", state.wifi);
        state.bluetooth = read_bool("network.bluetooth", state.bluetooth);
        state.volume = read_float("sound.output-volume", state.volume);
        state.brightness = read_float("display.brightness", state.brightness);
        state.night_light = read_bool("display.night-light", state.night_light);
        if let Ok(Some(toml::Value::String(mode))) = self.store.load("power.power-mode") {
            if let Some(mode) = PowerMode::parse(&mode) {
                state.power_mode = mode;
            }
        }
        drop(state);

        // Battery is polled once at startup; P5 owns the polling loop.
        if let Some(Battery { percent, charging }) = self.services.borrow_mut().battery() {
            let mut state = self.state.borrow_mut();
            state.battery = percent;
            state.charging = charging;
        }
    }

    fn wire(self: &Rc<Self>) {
        let on = |f: fn(&ShellController, &str)| {
            let weak = Rc::downgrade(self);
            move |id: slint::SharedString| {
                if let Some(controller) = weak.upgrade() {
                    f(&controller, id.as_str());
                }
            }
        };

        self.ui.on_dock_chosen(on(Self::dock_chosen));
        self.ui.on_launch_app(on(Self::launch_app));

        {
            let weak = Rc::downgrade(self);
            self.ui.on_tray_chosen(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.tray_chosen(id.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_menu_chosen(move |menu, index| {
                if let Some(controller) = weak.upgrade() {
                    controller.menu_chosen(menu as usize, index as usize);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_search_edited(move |text| {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_search(text.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_search_accepted(move |index| {
                if let Some(controller) = weak.upgrade() {
                    controller.accept_search(index as usize);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_quick_toggle(move |id, value| {
                if let Some(controller) = weak.upgrade() {
                    controller.quick_toggle(id.as_str(), value);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_volume_changed(move |value| {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_volume(value);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_brightness_changed(move |value| {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_brightness(value);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_power_mode_changed(move |mode| {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_power_mode(mode.as_str());
                }
            });
        }
        self.ui.on_notification_dismissed(on(Self::dismiss_notification));
        {
            let weak = Rc::downgrade(self);
            self.ui.on_notifications_cleared(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.state.borrow_mut().clear_notifications();
                    controller.sync();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_unlock_submitted(move |password| {
                if let Some(controller) = weak.upgrade() {
                    controller.submit_unlock(password.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_overlay_requested(move |code| {
                if let Some(controller) = weak.upgrade() {
                    controller.request_overlay(Overlay::from_code(code));
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_settings_row_toggled(move |key, value| {
                if let Some(controller) = weak.upgrade() {
                    controller.set_setting_bool(key.as_str(), value);
                }
            });
        }
        self.ui
            .on_settings_row_activated(on(Self::activate_setting));
        self.ui.on_window_activated(on(Self::activate_window));
    }

    /// Push the model into the UI. Called after every mutation.
    pub fn sync(&self) {
        let state = self.state.borrow();
        self.ui.set_active_app(state.active_app.clone().into());
        self.ui.set_clock(state.clock.clone().into());
        self.ui.set_date_label(state.date_label.clone().into());
        self.ui.set_wifi(state.wifi);
        self.ui.set_bluetooth(state.bluetooth);
        self.ui.set_focus_on(state.focus_active());
        self.ui.set_night_light(state.night_light);
        self.ui.set_volume(state.volume);
        self.ui.set_brightness(state.brightness);
        self.ui.set_battery(state.battery as i32);
        self.ui.set_charging(state.charging);
        self.ui.set_power_mode(state.power_mode.as_str().into());
        self.ui.set_unread(state.unread_badge().into());
        self.ui.set_overlay(state.overlay.code());
        self.ui.set_settings_page(state.settings_page as i32);
        self.ui.set_workspace(state.workspace as i32);
        self.ui.set_dock_auto_hide(state.dock_auto_hide);
        self.ui.set_search_query(state.search_query.clone().into());
        self.ui.set_search_selected(state.search_selected as i32);
        self.ui.set_lock_password(slint::SharedString::new());
        self.ui.set_lock_error(state.lock_error.clone().into());
        self.ui.set_unlocking(state.unlocking);

        self.ui
            .set_menus(ModelRc::new(VecModel::from(menus_for())));

        let apps: Vec<AppEntry> = state
            .dock_apps()
            .into_iter()
            .map(|app| AppEntry {
                id: app.id.clone().into(),
                name: app.name.clone().into(),
                icon: app.icon,
                running: app.running,
                pinned: app.pinned,
                badge: app.badge as i32,
            })
            .collect();
        self.ui.set_apps(ModelRc::new(VecModel::from(apps)));

        let notifications: Vec<NotificationEntry> = state
            .visible_notifications()
            .into_iter()
            .map(|notification| NotificationEntry {
                id: notification.id.clone().into(),
                app: notification.app.clone().into(),
                title: notification.title.clone().into(),
                body: notification.body.clone().into(),
                time: notification.time.clone().into(),
                icon: notification.icon,
                unread: notification.unread,
            })
            .collect();
        self.ui
            .set_notifications(ModelRc::new(VecModel::from(notifications)));

        let windows: Vec<WindowEntry> = state
            .windows
            .iter()
            .map(|window| WindowEntry {
                id: window.id.clone().into(),
                title: window.title.clone().into(),
                app: window.app.clone().into(),
                icon: window.icon,
                workspace: window.workspace as i32,
            })
            .collect();
        self.ui.set_windows(ModelRc::new(VecModel::from(windows)));

        let results: Vec<SearchResultEntry> = state
            .search_results
            .iter()
            .map(|hit| SearchResultEntry {
                id: hit.id.clone().into(),
                title: hit.title.clone().into(),
                subtitle: hit.subtitle.clone().into(),
                icon: hit.icon,
                kind: hit.kind.label().into(),
            })
            .collect();
        self.ui.set_results(ModelRc::new(VecModel::from(results)));

        let categories: Vec<SettingsCategoryEntry> = Category::ALL
            .iter()
            .map(|category| SettingsCategoryEntry {
                id: category.name().to_lowercase().into(),
                name: category.name().into(),
                icon: category.icon(),
            })
            .collect();
        self.ui
            .set_settings_categories(ModelRc::new(VecModel::from(categories)));

        let rows: Vec<SettingsRowEntry> = state
            .settings_rows
            .iter()
            .map(|row| SettingsRowEntry {
                key: row.key.clone().into(),
                label: row.label.clone().into(),
                file: row.file.clone().into(),
                kind: row.kind.clone().into(),
                value: row.value.clone().into(),
                on: row.on,
            })
            .collect();
        self.ui
            .set_settings_rows(ModelRc::new(VecModel::from(rows)));
    }

    // ------------------------------------------------------------- handlers ---

    fn dock_chosen(&self, id: &str) {
        self.state.borrow_mut().activate_from_dock(id);
        self.spawn_if_enabled(id);
        self.sync();
    }

    fn launch_app(&self, id: &str) {
        self.state.borrow_mut().launch(id);
        self.spawn_if_enabled(id);
        self.sync();
    }

    fn spawn_if_enabled(&self, id: &str) {
        if !*self.spawn_apps.borrow() {
            return;
        }
        let exec = {
            let state = self.state.borrow();
            state
                .apps
                .iter()
                .find(|app| app.id == id)
                .map(|app| app.exec.clone())
        };
        if let Some(exec) = exec {
            let mut parts = exec.split_whitespace();
            if let Some(program) = parts.next() {
                match std::process::Command::new(program).args(parts).spawn() {
                    Ok(_) => self.log.borrow_mut().push(format!("spawned {id}")),
                    Err(error) => self
                        .log
                        .borrow_mut()
                        .push(format!("spawn {id} failed: {error}")),
                }
            }
        }
    }

    fn tray_chosen(&self, id: &str) {
        match id {
            "wifi" => {
                let value = !self.state.borrow().wifi;
                self.quick_toggle("wifi", value);
            }
            "bluetooth" => {
                let value = !self.state.borrow().bluetooth;
                self.quick_toggle("bluetooth", value);
            }
            "sound" => self.request_overlay(Overlay::ControlCenter),
            "battery" => self.request_overlay(Overlay::ControlCenter),
            "notifications" => self.request_overlay(Overlay::Notifications),
            _ => {}
        }
    }

    fn menu_chosen(&self, menu: usize, index: usize) {
        let label = menus_for()
            .get(menu)
            .and_then(|group| group.items.iter().nth(index).map(|item| item.label.clone()))
            .unwrap_or_default();
        match label.as_str() {
            "Settings…" => self.request_overlay(Overlay::Settings),
            "About AetherOS" => self.request_overlay(Overlay::Settings),
            "Lock screen" => self.request_overlay(Overlay::Lock),
            "Quit" | "Shut down…" | "Restart…" | "Sleep" => {
                self.log.borrow_mut().push(format!("menu action: {label}"));
            }
            _ => {}
        }
        self.log
            .borrow_mut()
            .push(format!("menu {menu}/{index}: {label}"));
    }

    fn apply_search(&self, text: &str) {
        let definitions = config::SETTINGS;
        self.state.borrow_mut().set_search_query(text, definitions);
        self.sync();
    }

    fn accept_search(&self, index: usize) {
        let hit = {
            let mut state = self.state.borrow_mut();
            state.search_selected = index.min(state.search_results.len().saturating_sub(1));
            state.selected_result().cloned()
        };
        let Some(hit) = hit else { return };
        match hit.kind {
            SearchKind::App => self.launch_app(&hit.id),
            SearchKind::Settings => {
                let key = hit.id.trim_start_matches("setting:");
                if let Some(def) = config::find_setting(key) {
                    self.open_settings_category(def.category);
                }
            }
            SearchKind::Calculation => {
                self.log.borrow_mut().push(format!("calculation {}", hit.title));
            }
            SearchKind::File => {}
        }
    }

    fn quick_toggle(&self, id: &str, value: bool) {
        self.state.borrow_mut().set_toggle(id, value);
        let keyed = match id {
            "wifi" => Some(("network.wifi", "wifi")),
            "bluetooth" => Some(("network.bluetooth", "bluetooth")),
            "night-light" => Some(("display.night-light", "night-light")),
            _ => None,
        };
        if let Some((key, service_id)) = keyed {
            let _ = self
                .store
                .save(key, toml::Value::Boolean(value))
                .map_err(|error| self.log.borrow_mut().push(error));
            let result = match service_id {
                "wifi" => self.services.borrow_mut().set_wifi(value),
                "bluetooth" => self.services.borrow_mut().set_bluetooth(value),
                _ => Ok(()),
            };
            if let Err(error) = result {
                self.log.borrow_mut().push(format!("{service_id}: {error}"));
            }
        }
        self.sync();
    }

    fn apply_volume(&self, value: f32) {
        self.state.borrow_mut().set_volume(value);
        let volume = self.state.borrow().volume;
        let _ = self.services.borrow_mut().set_volume(volume);
        let _ = self.store.save(
            "sound.output-volume",
            toml::Value::Float(volume as f64),
        );
        self.sync();
    }

    fn apply_brightness(&self, value: f32) {
        self.state.borrow_mut().set_brightness(value);
        let brightness = self.state.borrow().brightness;
        let _ = self.services.borrow_mut().set_brightness(brightness);
        let _ = self.store.save(
            "display.brightness",
            toml::Value::Float(brightness as f64),
        );
        self.sync();
    }

    fn apply_power_mode(&self, mode: &str) {
        let Some(mode) = PowerMode::parse(mode) else {
            return;
        };
        self.state.borrow_mut().power_mode = mode;
        let _ = self.services.borrow_mut().set_power_mode(mode);
        let _ = self.store.save(
            "power.power-mode",
            toml::Value::String(mode.as_str().to_string()),
        );
        self.sync();
    }

    fn dismiss_notification(&self, id: &str) {
        self.state.borrow_mut().dismiss_notification(id);
        self.sync();
    }

    fn submit_unlock(&self, password: &str) {
        let unlocked = self.state.borrow_mut().submit_unlock(password);
        if unlocked {
            self.ui.set_lock_password(slint::SharedString::new());
            self.log.borrow_mut().push("unlocked".to_string());
        }
        self.sync();
        if unlocked {
            self.ui.set_overlay(Overlay::None.code());
            self.ui.set_lock_error(slint::SharedString::new());
        }
    }

    fn request_overlay(&self, overlay: Overlay) {
        self.state.borrow_mut().toggle_overlay(overlay);
        if overlay == Overlay::Lock {
            let _ = self.services.borrow_mut().lock_session();
        }
        if overlay == Overlay::Settings {
            self.refresh_settings_rows();
        }
        self.sync();
    }

    pub fn open_settings_category(&self, category: Category) {
        let page = Category::ALL
            .iter()
            .position(|candidate| *candidate == category)
            .unwrap_or(0);
        {
            let mut state = self.state.borrow_mut();
            state.settings_page = page;
            state.overlay = Overlay::Settings;
        }
        self.refresh_settings_rows();
        self.sync();
    }

    fn refresh_settings_rows(&self) {
        let page = self.state.borrow().settings_page;
        let category = Category::ALL[page.min(Category::ALL.len() - 1)];
        let rows = self.store.rows_for(category);
        self.state.borrow_mut().settings_rows = rows;
    }

    fn set_setting_bool(&self, key: &str, value: bool) {
        let _ = self.store.save(key, toml::Value::Boolean(value));
        if let Some(def) = config::find_setting(key) {
            self.state.borrow_mut().set_toggle(key, value);
            if def.key.starts_with("theme.") {
                self.log.borrow_mut().push(format!("theme {key}={value}"));
            }
        }
        self.refresh_settings_rows();
        self.sync();
    }

    /// Clicking a select/value row cycles select values so the whole settings
    /// app is drivable with a mouse and a test can assert the file write.
    fn activate_setting(&self, key: &str) {
        let Some(def) = config::find_setting(key) else {
            return;
        };
        match def.kind {
            SettingKind::Toggle => {
                let value = !self
                    .state
                    .borrow()
                    .settings_rows
                    .iter()
                    .find(|row| row.key == key)
                    .map(|row| row.on)
                    .unwrap_or(false);
                self.set_setting_bool(key, value);
            }
            SettingKind::Select(values) => {
                let current = Some(
                    self.store
                        .load(key)
                        .ok()
                        .flatten()
                        .map(|value| match value {
                            toml::Value::String(text) => text,
                            other => config::render_value(&other),
                        })
                        .unwrap_or_else(|| def.default.to_string()),
                );
                let index = current
                    .and_then(|text| values.iter().position(|value| *value == text))
                    .map(|index| (index + 1) % values.len())
                    .unwrap_or(0);
                let _ = self.store.save(
                    key,
                    toml::Value::String(values[index].to_string()),
                );
                if key == "power.power-mode" {
                    self.apply_power_mode(values[index]);
                }
            }
            SettingKind::Value => {
                self.log.borrow_mut().push(format!("value setting {key}"));
            }
        }
        self.refresh_settings_rows();
        self.sync();
    }

    fn activate_window(&self, id: &str) {
        let (app_name, workspace) = {
            let state = self.state.borrow();
            match state.windows.iter().find(|window| window.id == id) {
                Some(window) => (
                    state
                        .apps
                        .iter()
                        .find(|app| app.id == window.app)
                        .map(|app| app.name.clone())
                        .unwrap_or_else(|| window.app.clone()),
                    window.workspace,
                ),
                None => return,
            }
        };
        {
            let mut state = self.state.borrow_mut();
            state.active_app = app_name;
            state.workspace = workspace;
            state.overlay = Overlay::None;
        }
        self.sync();
    }

    // ------------------------------------------------------------ public API ---

    /// Update the clock and date (called on a one-minute timer by the binary).
    pub fn tick(&self, now: &chrono::DateTime<chrono::Local>) {
        {
            let mut state = self.state.borrow_mut();
            state.clock = model::format_clock(now);
            state.date_label = model::format_date(now);
        }
        self.ui.set_clock(model::format_clock(now).into());
        self.ui.set_date_label(model::format_date(now).into());
    }

    pub fn open(&self, overlay: Overlay) {
        self.request_overlay(overlay);
    }

    pub fn search(&self, text: &str) {
        self.apply_search(text);
    }

    pub fn launch(&self, id: &str) {
        self.launch_app(id);
    }

    pub fn activate(&self, id: &str) {
        self.dock_chosen(id);
    }

    pub fn dismiss(&self, id: &str) {
        self.dismiss_notification(id);
    }

    pub fn set_toggle(&self, id: &str, value: bool) {
        self.quick_toggle(id, value);
    }

    pub fn submit_password(&self, password: &str) {
        self.submit_unlock(password);
    }

    pub fn settings_category(&self, category: Category) {
        self.open_settings_category(category);
    }

    pub fn toggle_setting(&self, key: &str, value: bool) {
        self.set_setting_bool(key, value);
    }

    pub fn cycle_setting(&self, key: &str) {
        self.activate_setting(key);
    }

    /// Keyboard shortcut entry point (F1 SHL-03: launcher by keyboard, SHL-05
    /// Focus, SHL-10 lock). The layer-shell host feeds XKB keysyms here; the
    /// names below are also what `input/keybindings.toml` stores.
    pub fn handle_shortcut(&self, shortcut: &str) -> bool {
        match shortcut.trim().to_ascii_lowercase().as_str() {
            "super+space" | "launcher" => {
                self.request_overlay(Overlay::Launcher);
                true
            }
            "super+a" | "overview" => {
                self.request_overlay(Overlay::Overview);
                true
            }
            "super+n" | "notifications" => {
                self.request_overlay(Overlay::Notifications);
                true
            }
            "super+c" | "control-centre" | "control-center" => {
                self.request_overlay(Overlay::ControlCenter);
                true
            }
            "super+l" | "lock" => {
                self.request_overlay(Overlay::Lock);
                true
            }
            "super+f" | "focus" => {
                let focus = !self.state.borrow().focus_active();
                self.quick_toggle("focus", focus);
                true
            }
            "escape" | "esc" => {
                self.request_overlay(Overlay::None);
                true
            }
            "arrow-down" | "down" => {
                self.state.borrow_mut().move_selection(1);
                self.sync();
                true
            }
            "arrow-up" | "up" => {
                self.state.borrow_mut().move_selection(-1);
                self.sync();
                true
            }
            "enter" | "return" => {
                let selected = self.state.borrow().search_selected;
                self.accept_search(selected);
                true
            }
            _ => false,
        }
    }

    /// Items currently rendered in the launcher (used by tests).
    pub fn result_titles(&self) -> Vec<String> {
        self.state
            .borrow()
            .search_results
            .iter()
            .map(|hit| hit.title.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_cover_the_standard_groups() {
        let menus = menus_for();
        assert_eq!(menus[0].title, "AetherOS");
        assert_eq!(menus[0].items.row_data(0).unwrap().label, "About AetherOS");
        let edit = menus.iter().find(|group| group.title == "Edit").unwrap();
        assert!((0..edit.items.row_count()).any(|i| edit.items.row_data(i).unwrap().label == "Copy"));
        assert!(menus.len() >= 5);
    }

    #[test]
    fn demo_content_is_complete() {
        assert_eq!(demo_notifications().len(), 3);
        assert_eq!(demo_notifications().iter().filter(|n| n.unread).count(), 2);
        assert_eq!(demo_windows().len(), 3);
    }
}
