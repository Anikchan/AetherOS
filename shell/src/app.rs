//! Shell controller: the only bridge between the pure model and Slint (P4, S3).
//!
//! `ShellController` owns the state, wires every UI callback, applies system
//! service changes and persists settings through F4 files. Tests construct it
//! with `MockServices` and a temporary config root, then drive it through the
//! headless element queries.

use chrono::Timelike;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::IconKind;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::a11y::{self, A11ySettings};
use crate::auth::{self, AuthOutcome, Authenticator};
use crate::config::{self, Category, SettingKind, SettingsStore};
use crate::desktop;
use crate::effects::{self, EffectSettings};
use crate::features::FeaturesPage;
use crate::kbdnav::{self, Focus, Surface, Tray};
use crate::model::{self, Notification, Overlay, PowerMode, SearchKind, ShellState, Window};
use crate::nohang::{self, DialogAction, DialogEvent, HangEvent, NoHangDialog, StartLedger, StartOutcome};
use crate::services::{Battery, Services};
use crate::spawn;
use aether_ui::theme::Tier;
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
    /// How a launcher id becomes a process: the registry argv through this
    /// seam. Tests install a recording spawner.
    spawner: RefCell<Box<dyn spawn::Spawner>>,
    /// Lock-screen backend: PAM on Linux builds, the local prototype
    /// elsewhere (P4-T06). Tests substitute a scripted backend.
    authenticator: RefCell<Box<dyn Authenticator>>,
    /// Settings → Features state (P7-T07); the manager owns `features.toml`.
    features: RefCell<FeaturesPage>,
    /// Effect tier: the persisted request and the resolved tier (P4-T04).
    effects: RefCell<EffectSettings>,
    /// F1 SHL-18: reduce motion, reduce transparency and text scaling.
    a11y: RefCell<A11ySettings>,
    /// F1 SHL-18: keyboard focus (surface + item) and ring visibility.
    keyboard: RefCell<Focus>,
    /// G-NH2: the "Not responding" dialog (P3-T08 markers).
    nohang: RefCell<NoHangDialog>,
    /// G-NH2: consecutive starts / safe mode, persisted across restarts.
    startup: RefCell<StartLedger>,
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

        let features_path = store.root().join("features").join("features.toml");
        // P4-T04: the effect tier is a settings value the control centre owns.
        let effects = EffectSettings::load(&store);
        // F1 SHL-18: the accessibility switches share `theme/tokens.toml`.
        let a11y = A11ySettings::load(&store);
        // G-NH2: read-only here; `--start-begin` owns the counting so a render
        // or `--status` never looks like a shell start.
        let startup = StartLedger::load(&StartLedger::path(&store));
        let controller = Rc::new(ShellController {
            state: RefCell::new(state),
            ui,
            services: RefCell::new(services),
            store,
            spawn_apps: RefCell::new(false),
            spawner: RefCell::new(Box::new(spawn::ProcessSpawner)),
            authenticator: RefCell::new(auth::default_authenticator()),
            features: RefCell::new(FeaturesPage::load(features_path)),
            effects: RefCell::new(effects),
            a11y: RefCell::new(a11y),
            keyboard: RefCell::new(Focus::default()),
            nohang: RefCell::new(NoHangDialog::with_default_grace()),
            startup: RefCell::new(startup),
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

    /// Replace the process seam and enable spawning: the launcher hands every
    /// start to `spawner` (tests record it, the host may supervise it).
    pub fn set_spawner(&self, spawner: Box<dyn spawn::Spawner>) {
        *self.spawner.borrow_mut() = spawner;
        *self.spawn_apps.borrow_mut() = true;
    }

    /// The argv a launcher id would start, without starting it.
    pub fn launch_argv(&self, id: &str) -> Option<Vec<String>> {
        let exec = {
            let state = self.state.borrow();
            state
                .apps
                .iter()
                .find(|app| app.id == id)
                .map(|app| app.exec.clone())
        }?;
        Some(spawn::launch_argv(id, &exec).0)
    }

    /// Replace the lock-screen backend (tests, and hosts that bring their own
    /// PAM broker). P4-T06.
    pub fn set_authenticator(&self, authenticator: Box<dyn Authenticator>) {
        *self.authenticator.borrow_mut() = authenticator;
    }

    /// Name of the active lock-screen backend, for diagnostics.
    pub fn authenticator_name(&self) -> String {
        self.authenticator.borrow().name().to_string()
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
        // Focus: an optional daily window from notifications.toml.
        let focus_enabled = read_bool("notifications.focus-enabled", false);
        let focus_start = self
            .store
            .load("notifications.focus-start")
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(model::parse_hhmm).flatten())
            .unwrap_or(9 * 60);
        let focus_end = self
            .store
            .load("notifications.focus-end")
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(model::parse_hhmm).flatten())
            .unwrap_or(17 * 60);
        if focus_enabled {
            state.focus_schedule = Some(model::FocusSchedule::new(focus_start, focus_end));
        }
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
        {
            let weak = Rc::downgrade(self);
            self.ui.on_effect_tier_changed(move |tier| {
                if let Some(controller) = weak.upgrade() {
                    controller.apply_effect_tier(tier.as_str());
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
        {
            let weak = Rc::downgrade(self);
            self.ui.on_settings_page_requested(move |index| {
                if let Some(controller) = weak.upgrade() {
                    let page = (index.max(0) as usize).min(Category::ALL.len() - 1);
                    controller.open_settings_category(Category::ALL[page]);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_feature_install(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.install_feature(id.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_feature_remove(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.remove_feature(id.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_feature_enabled(move |id, enabled| {
                if let Some(controller) = weak.upgrade() {
                    controller.set_feature_enabled(id.as_str(), enabled);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_workspace_requested(move |index| {
                if let Some(controller) = weak.upgrade() {
                    controller.state.borrow_mut().workspace = (index.max(0) as usize).min(3);
                    controller.sync();
                }
            });
        }
        self.ui.on_window_activated(on(Self::activate_window));
        {
            let weak = Rc::downgrade(self);
            self.ui.on_nohang_wait(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.dialog_action(DialogAction::Wait, Instant::now());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_nohang_force_quit(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.dialog_action(DialogAction::ForceQuit, Instant::now());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_safe_mode_reset(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.reset_safe_mode();
                }
            });
        }
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
        self.ui
            .set_focus_label(state.focus_label().unwrap_or_default().into());
        self.ui.set_night_light(state.night_light);
        self.ui.set_volume(state.volume);
        self.ui.set_brightness(state.brightness);
        self.ui.set_battery(state.battery as i32);
        self.ui.set_charging(state.charging);
        self.ui.set_power_mode(state.power_mode.as_str().into());
        // P4-T04: every panel renders at the resolved tier (glass degrades with
        // it) and the control centre shows which one is in effect.
        {
            let effects = self.effects.borrow();
            let effective = nohang::resolve_tier(self.safe_mode(), effects.effective());
            self.ui.set_tier(effects::to_slint(effective));
            self.ui
                .set_effects_tier(effective.as_str().into());
            self.ui.set_effects_note(effects.subtitle().into());
        }
        // F1 SHL-18: text scaling and reduce motion reach every widget through
        // the Tokens globals (`ui/design.slint`).
        {
            let a11y = self.a11y.borrow();
            self.ui.set_text_scale(a11y.text_scale);
            self.ui.set_reduce_motion(a11y.reduce_motion);
            let tokens = self.ui.global::<crate::Tokens>();
            tokens.set_text_scale(a11y.text_scale);
            tokens.set_reduce_motion(a11y.reduce_motion);
        }
        // F1 SHL-18: keyboard focus (surface + item). The index is clamped to
        // the current list so a shrinking dock or launcher never leaves focus
        // on a missing item.
        {
            let counts = kbdnav::counts(&state, menus_for().len());
            let mut focus = self.keyboard.borrow_mut();
            focus.clamp(&counts);
            self.ui.set_focus_surface(focus.surface.code());
            self.ui.set_focus_index(focus.index as i32);
            self.ui.set_focus_visible(focus.visible);
        }
        // G-NH2: the "Not responding" dialog and the safe-mode banner.
        {
            let dialog = self.nohang.borrow();
            match dialog.current() {
                Some(app) => {
                    self.ui.set_nohang_open(true);
                    self.ui.set_nohang_app(app.name.into());
                    self.ui.set_nohang_detail(
                        "The window stopped answering. Wait for it to recover, or force it to quit."
                            .into(),
                    );
                }
                None => {
                    self.ui.set_nohang_open(false);
                    self.ui.set_nohang_app(Default::default());
                    self.ui.set_nohang_detail(Default::default());
                }
            }
        }
        {
            let startup = self.startup.borrow();
            self.ui.set_safe_mode(startup.safe_mode);
            let note = if startup.safe_mode {
                format!(
                    "Safe mode: {}. Glass effects are off; reset to leave it.",
                    startup.safe_mode_reason()
                )
            } else {
                String::new()
            };
            self.ui.set_safe_mode_note(note.into());
        }
        self.ui.set_unread(state.unread_badge().into());
        self.ui.set_overlay(state.overlay.code());
        self.ui.set_settings_page(state.settings_page as i32);
        self.ui.set_workspace(state.workspace as i32);
        self.ui.set_dock_auto_hide(state.dock_auto_hide);
        self.ui.set_search_query(state.search_query.clone().into());
        self.ui.set_search_selected(state.search_selected as i32);
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
        self.sync_features();
    }

    // ------------------------------------------------------------- handlers ---

    fn dock_chosen(&self, id: &str) {
        self.note_activation("dock", id);
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
        // Shell-owned surfaces (Settings) are overlays the shell draws itself;
        // there is no process to start for them.
        if desktop::SHELL_OWNED_IDS.contains(&id) {
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
        let Some(exec) = exec else {
            return;
        };
        // P4 launch path: registry ids start with `<exec> --launch <id>`, which
        // every app's `handle_launch_arg` handshake expects; anything the
        // registry does not own keeps its `.desktop` Exec line.
        let spawner = self.spawner.borrow();
        let (argv, source, outcome) = spawn::launch(id, &exec, spawner.as_ref());
        drop(spawner);
        let mut log = self.log.borrow_mut();
        match outcome {
            Ok(()) => log.push(format!(
                "spawned {id} source={} argv={}",
                source.as_str(),
                argv.join(" ")
            )),
            Err(error) => log.push(format!("spawn {id} failed: {error}")),
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
            "Close window" => {
                // The compositor asks for this from the window controls; here it
                // also clears the dock's running indicator (P4-T02).
                let id = {
                    let state = self.state.borrow();
                    state
                        .apps
                        .iter()
                        .find(|app| app.name == state.active_app)
                        .map(|app| app.id.clone())
                        .or_else(|| state.windows.first().map(|window| window.app.clone()))
                };
                if let Some(id) = id {
                    self.close_app(&id);
                }
            }
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
        self.note_activation("launcher", &index.to_string());
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
        if id == "focus" {
            // Focus is a real setting: persist the toggle and keep the parsed
            // schedule (the shell falls back to "until 18:00" without one).
            let _ = self.store.save(
                "notifications.focus-enabled",
                toml::Value::Boolean(value),
            );
            let start = self
                .store
                .load("notifications.focus-start")
                .ok()
                .flatten()
                .and_then(|value| value.as_str().map(model::parse_hhmm).flatten());
            let end = self
                .store
                .load("notifications.focus-end")
                .ok()
                .flatten()
                .and_then(|value| value.as_str().map(model::parse_hhmm).flatten());
            if let (Some(start), Some(end)) = (start, end) {
                let mut state = self.state.borrow_mut();
                state.focus_schedule = if value {
                    Some(model::FocusSchedule::new(start, end))
                } else {
                    None
                };
            }
        }
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

    /// P4-T04: the control centre's Full/Reduced/Minimal switch. The request is
    /// persisted through aether-config and published as the compositor's tier
    /// env (`AETHER_COMPOSITOR_EFFECT_TIER`); the running compositor keeps its
    /// own tier until restart, which `--effects-check` reports as a mismatch.
    fn apply_effect_tier(&self, tier: &str) {
        let Some(tier) = Tier::parse(tier) else {
            self.log
                .borrow_mut()
                .push(format!("effects: unknown tier {tier}"));
            return;
        };
        match EffectSettings::save(&self.store, tier) {
            Ok(settings) => {
                self.log.borrow_mut().push(format!(
                    "effects tier={} effective={} env={}",
                    settings.requested.as_str(),
                    settings.effective().as_str(),
                    settings.env_line()
                ));
                *self.effects.borrow_mut() = settings;
            }
            Err(error) => self.log.borrow_mut().push(format!("effects: {error}")),
        }
        self.refresh_settings_rows();
        self.sync();
    }

    /// Lock screen submit (P4-T06): the backend decides, the model rate-limits,
    /// and the surface shows the reason. Never keeps the password.
    fn submit_unlock(&self, password: &str) {
        if self.state.borrow().lockout_active() {
            // Rate-limited: do not even ask the backend (SHL-AC-08).
            self.ui.set_lock_password(slint::SharedString::new());
            self.sync();
            return;
        }
        let user = auth::session_user();
        let outcome = self.authenticator.borrow().authenticate(&user, password);
        let unlocked = outcome.is_allowed();
        match outcome {
            AuthOutcome::Allowed => {
                self.state.borrow_mut().unlock_allowed();
                self.log.borrow_mut().push("unlocked".to_string());
            }
            AuthOutcome::Unavailable(message) => {
                // Backend down: say so, but do not count it as a wrong password.
                let mut state = self.state.borrow_mut();
                state.unlocking = false;
                state.lock_error = message;
            }
            AuthOutcome::Denied(message) => {
                self.state.borrow_mut().unlock_denied(&message);
                self.log.borrow_mut().push("unlock-failed".to_string());
            }
        }
        self.ui.set_lock_password(slint::SharedString::new());
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
        self.focus_overlay();
        self.sync();
    }

    /// F1 SHL-18: an opened surface takes the keyboard focus, and a closed one
    /// returns it to the menu bar. Visibility stays as it was, so a mouse-only
    /// session draws no ring until the first Tab.
    fn focus_overlay(&self) {
        let overlay = self.state.borrow().overlay;
        let mut focus = self.keyboard.borrow_mut();
        match overlay {
            Overlay::Launcher => {
                focus.surface = Surface::Launcher;
                focus.index = 0;
            }
            Overlay::ControlCenter => {
                focus.surface = Surface::ControlCenter;
                focus.index = 0;
            }
            Overlay::Notifications => {
                focus.surface = Surface::Notifications;
                focus.index = 0;
            }
            // No overlay (and Settings, overview, lock which are not one of the
            // five navigable surfaces): the stop is the always-visible menu bar.
            _ => {
                focus.surface = Surface::Menubar;
                focus.index = kbdnav::MENU_LOGO;
            }
        }
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

    // -------------------------------------------------------------- features ---

    fn sync_features(&self) {
        let page = self.features.borrow();
        self.ui
            .set_feature_rows(ModelRc::new(VecModel::from(page.rows())));
        self.ui.set_feature_budget(page.budget_label().into());
        self.ui.set_feature_usage(page.usage_label().into());
        self.ui.set_feature_message(page.message().into());
        self.ui.set_feature_failed(page.failed());
    }

    /// Settings → Features (P7-T07). Every action persists through the manager.
    pub fn install_feature(&self, id: &str) {
        self.features.borrow_mut().install(id);
        self.sync_features();
    }

    pub fn remove_feature(&self, id: &str) {
        self.features.borrow_mut().remove(id);
        self.sync_features();
    }

    pub fn set_feature_enabled(&self, id: &str, enabled: bool) {
        self.features.borrow_mut().set_enabled(id, enabled);
        self.sync_features();
    }

    pub fn features_page(&self) -> std::cell::Ref<'_, FeaturesPage> {
        self.features.borrow()
    }

    // --------------------------------------------------------------- effects ---

    /// Resolved effect settings (requested tier + accessibility override).
    pub fn effect_settings(&self) -> EffectSettings {
        *self.effects.borrow()
    }

    /// Tier every panel and the compositor should use (safe mode forces the
    /// Minimal tier: G-NH2 runs the reduced session).
    pub fn effect_tier(&self) -> Tier {
        nohang::resolve_tier(self.safe_mode(), self.effects.borrow().effective())
    }

    /// `AETHER_COMPOSITOR_EFFECT_TIER=<tier>` for the session environment.
    pub fn effect_env_line(&self) -> String {
        format!(
            "{}={}",
            effects::ENV_TIER,
            self.effect_tier().as_str()
        )
    }

    // ---------------------------------------------------------------- nohang ---

    /// G-NH2: feed one compositor health marker to the "Not responding" dialog.
    /// The host reads `AETHER_COMPOSITOR_APP_STATE` from the journal; tests call
    /// this directly.
    pub fn observe_health(&self, event: HangEvent, now: Instant) -> DialogEvent {
        let outcome = self.nohang.borrow_mut().observe(event, now);
        let marker = match outcome {
            DialogEvent::Shown(client) => Some(self.nohang.borrow().dialog_line(client)),
            DialogEvent::Recovered(client) => Some(self.nohang.borrow().recovered_line(client)),
            DialogEvent::Queued(client) => {
                Some(format!("AETHER_SHELL_NOHANG_QUEUED: client={client}"))
            }
            DialogEvent::None => None,
        };
        if let Some(marker) = marker {
            self.log.borrow_mut().push(marker);
        }
        self.sync();
        outcome
    }

    /// The dialog's clock: re-offers a waived app after the wait grace.
    pub fn tick_dialog(&self, now: Instant) -> Option<u64> {
        let raised = self.nohang.borrow_mut().tick(now);
        if let Some(client) = raised {
            let line = self.nohang.borrow().dialog_line(client);
            self.log.borrow_mut().push(line);
            self.sync();
        }
        raised
    }

    /// Wait / Force Quit. Force Quit returns the client the host must terminate
    /// (the compositor's `xdg_toplevel` unresponsive error does the kill).
    pub fn dialog_action(&self, action: DialogAction, now: Instant) -> Option<u64> {
        let acted = self.nohang.borrow_mut().act(action, now);
        if let Some((action, client)) = acted {
            let line = self.nohang.borrow().action_line(client, action);
            self.log.borrow_mut().push(line);
            if action == DialogAction::ForceQuit {
                self.log
                    .borrow_mut()
                    .push(format!("AETHER_SHELL_NOHANG_TERMINATE: client={client}"));
            }
        }
        self.sync();
        acted.map(|(_, client)| client)
    }

    /// Register the app identity behind a compositor client id (the journal only
    /// carries ids).
    pub fn register_client(&self, client: u64, id: &str, name: &str) {
        self.nohang.borrow_mut().register_name(client, id, name);
    }

    pub fn nohang(&self) -> std::cell::Ref<'_, NoHangDialog> {
        self.nohang.borrow()
    }

    /// Safe mode is on: the reduced session until the banner's reset.
    pub fn safe_mode(&self) -> bool {
        self.startup.borrow().safe_mode
    }

    pub fn startup(&self) -> std::cell::Ref<'_, StartLedger> {
        self.startup.borrow()
    }

    /// Fold the compositor's shell-watchdog markers into the ledger (the
    /// interactive host feeds them as they arrive). Three stalls arm safe mode;
    /// returns `true` when this call armed it.
    pub fn observe_watchdog(&self, report: &nohang::WatchdogReport) -> bool {
        let armed = self.startup.borrow_mut().observe_watchdog(report);
        if armed {
            let line = self.startup.borrow().safe_mode_line();
            self.log.borrow_mut().push(line.trim_end().to_string());
        }
        self.persist_startup();
        self.sync();
        armed
    }

    /// Count a start attempt; called by the session launcher before the layer
    /// surfaces bind (`aether-shell --start-begin`). Three in a row enter safe
    /// mode.
    pub fn begin_session_start(&self) -> StartOutcome {
        let outcome = self.startup.borrow_mut().begin_start();
        self.persist_startup();
        self.sync();
        outcome
    }

    /// The session is ready: the start counted, so clear the failure loop.
    pub fn session_ready(&self) {
        self.startup.borrow_mut().confirm_start();
        self.persist_startup();
        self.sync();
    }

    /// The safe-mode banner's reset: leave safe mode and start counting again.
    pub fn reset_safe_mode(&self) {
        self.startup.borrow_mut().reset();
        self.persist_startup();
        self.log
            .borrow_mut()
            .push("AETHER_SHELL_SAFE_MODE_RESET: attempts=0".to_string());
        self.sync();
    }

    fn persist_startup(&self) {
        let ledger = self.startup.borrow();
        if let Err(error) = ledger.save(&StartLedger::path(&self.store)) {
            self.log
                .borrow_mut()
                .push(format!("nohang: {error}"));
        }
    }

    fn set_setting_bool(&self, key: &str, value: bool) {
        let _ = self.store.save(key, toml::Value::Boolean(value));
        if let Some(def) = config::find_setting(key) {
            // Settings use dotted file keys; live state uses short ids. Without
            // this mapping a settings toggle wrote the file but left the shell
            // (and its services) on the old value.
            let live_id = match key {
                "network.wifi" => "wifi",
                "network.bluetooth" => "bluetooth",
                "display.night-light" => "night-light",
                "notifications.focus-enabled" => "focus",
                other => other,
            };
            self.state.borrow_mut().set_toggle(live_id, value);
            if def.key.starts_with("theme.") {
                self.log.borrow_mut().push(format!("theme {key}={value}"));
            }
        }
        // Reduce transparency forces the Minimal tier (D1 section 9).
        if key == effects::SETTING_REDUCE_TRANSPARENCY {
            *self.effects.borrow_mut() = EffectSettings::load(&self.store);
            *self.a11y.borrow_mut() = A11ySettings::load(&self.store);
        }
        // Reduce motion collapses slides to a 120 ms fade (D1 section 8).
        if key == a11y::SETTING_REDUCE_MOTION {
            let settings = A11ySettings::load(&self.store);
            self.log.borrow_mut().push(format!(
                "AETHER_SHELL_A11Y: reduce-motion={} text-scale={}% motion_ms={}",
                settings.reduce_motion as u8,
                settings.text_scale_percent(),
                settings.motion_ms()
            ));
            *self.a11y.borrow_mut() = settings;
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
                if key == effects::SETTING_TIER {
                    // The settings app writes the same key the control centre
                    // does; keep the live tier and the compositor env in step.
                    *self.effects.borrow_mut() = EffectSettings::load(&self.store);
                    self.log.borrow_mut().push(format!(
                        "effects tier={} source=settings",
                        self.effects.borrow().requested.as_str()
                    ));
                }
                if key == a11y::SETTING_TEXT_SCALE {
                    // The Appearance row cycles 100 % → 200 %; the surfaces read
                    // the clamped factor from the accessibility settings.
                    let settings = A11ySettings::load(&self.store);
                    self.log.borrow_mut().push(format!(
                        "AETHER_SHELL_A11Y: text-scale={}% source=settings",
                        settings.text_scale_percent()
                    ));
                    *self.a11y.borrow_mut() = settings;
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
        let minute = (now.hour() * 60 + now.minute()) as u16;
        let focus_changed = {
            let mut state = self.state.borrow_mut();
            state.clock = model::format_clock(now);
            state.date_label = model::format_date(now);
            state.refresh_time(minute)
        };
        self.ui.set_clock(model::format_clock(now).into());
        self.ui.set_date_label(model::format_date(now).into());
        if focus_changed {
            // Focus started or ended with the clock: refresh badges and cards.
            self.sync();
        }
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

    /// Close an app: clears its windows and its dock running indicator.
    pub fn close_app(&self, id: &str) {
        self.state.borrow_mut().close_app(id);
        self.sync();
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
            // F1 SHL-18: Tab/arrows/Enter/Space are the keyboard navigation
            // entry point; the shell host feeds raw keysyms here too.
            _ => self.handle_key(shortcut),
        }
    }

    /// Keyboard navigation across the shell surfaces (F1 SHL-18, D1 section 9).
    ///
    /// Tab / Shift+Tab change surface, the arrows move and wrap inside one, and
    /// Enter or Space activates the focused item through the same handler a
    /// click uses. Returns `false` for keys the shell does not own so the
    /// compositor can pass them to the focused app.
    pub fn handle_key(&self, key: &str) -> bool {
        let key = key.trim().to_ascii_lowercase();
        let overlay = self.state.borrow().overlay;
        match key.as_str() {
            "tab" => {
                let counts = self.keyboard_counts();
                self.keyboard.borrow_mut().cycle(&counts, 1);
                self.after_focus();
                true
            }
            "shift+tab" | "backtab" => {
                let counts = self.keyboard_counts();
                self.keyboard.borrow_mut().cycle(&counts, -1);
                self.after_focus();
                true
            }
            "left" | "arrow-left" => {
                self.step_focus(-1);
                true
            }
            "right" | "arrow-right" => {
                self.step_focus(1);
                true
            }
            "up" | "arrow-up" | "down" | "arrow-down" => {
                let delta = if key.contains("up") { -1 } else { 1 };
                if overlay == Overlay::Launcher {
                    // The launcher keeps its own selection; Enter opens it and
                    // the focus ring follows the highlighted row.
                    self.state.borrow_mut().move_selection(delta);
                    let selected = self.state.borrow().search_selected;
                    let mut focus = self.keyboard.borrow_mut();
                    focus.surface = Surface::Launcher;
                    focus.index = selected;
                    focus.visible = true;
                    drop(focus);
                    self.sync();
                } else {
                    self.step_focus(delta);
                }
                true
            }
            "enter" | "return" | "space" => {
                self.activate_focus();
                true
            }
            "escape" | "esc" => {
                self.request_overlay(Overlay::None);
                true
            }
            _ => false,
        }
    }

    fn step_focus(&self, delta: i32) {
        let surface = self.keyboard.borrow().surface;
        let count = self
            .keyboard_counts()
            .into_iter()
            .find(|(candidate, _)| *candidate == surface)
            .map(|(_, count)| count)
            .unwrap_or(0);
        self.keyboard.borrow_mut().move_within(count, delta);
        self.after_focus();
    }

    fn after_focus(&self) {
        self.sync();
        let line = self.keyboard.borrow().line();
        self.log.borrow_mut().push(line);
    }

    /// Focusable counts per surface, in Tab order.
    pub fn keyboard_counts(&self) -> Vec<(Surface, usize)> {
        let state = self.state.borrow();
        kbdnav::counts(&state, menus_for().len())
    }

    /// The current keyboard focus (tests and the host log).
    pub fn focus(&self) -> Focus {
        *self.keyboard.borrow()
    }

    /// `--a11y-status` keyboard section: one line per surface.
    pub fn keyboard_plan(&self) -> String {
        let focus = self.focus();
        let mut out = String::new();
        for (surface, count) in self.keyboard_counts() {
            let marker = if surface == focus.surface { " *" } else { "" };
            out.push_str(&format!(
                "A11Y_KEYBOARD: surface={} items={}{marker}\n",
                surface.as_str(),
                count
            ));
        }
        out
    }

    /// `AETHER_SHELL_ACTIVATE: surface=<s> item=<id>` — which handler ran, for
    /// the host journal (keyboard Enter/Space and pointer clicks alike).
    fn note_activation(&self, surface: &str, item: &str) {
        self.log.borrow_mut().push(format!(
            "AETHER_SHELL_ACTIVATE: surface={surface} item={item}"
        ));
    }

    /// Enter/Space on the focused item: the same handlers the pointer uses.
    ///
    /// Each handler logs `AETHER_SHELL_ACTIVATE` itself, so a click and a key
    /// press over the same item produce the same marker.
    fn activate_focus(&self) {
        let focus = self.focus();
        match focus.surface {
            Surface::Menubar => self.activate_menubar(focus.index),
            Surface::Dock => {
                let id = {
                    let state = self.state.borrow();
                    state.dock_apps().get(focus.index).map(|app| app.id.clone())
                };
                if let Some(id) = id {
                    self.dock_chosen(&id);
                }
            }
            Surface::Launcher => {
                if focus.index < self.state.borrow().search_results.len() {
                    self.accept_search(focus.index);
                }
            }
            Surface::ControlCenter => self.activate_control_center(focus.index),
            Surface::Notifications => self.activate_notification(focus.index),
        }
    }

    fn activate_menubar(&self, index: usize) {
        self.note_activation("menubar", &index.to_string());
        let menus = menus_for();
        if index == kbdnav::MENU_LOGO {
            self.request_overlay(Overlay::Settings);
            return;
        }
        if index >= 1 && index <= menus.len() {
            let open = (index - 1) as i32;
            self.ui.set_menu_open(open);
            self.log
                .borrow_mut()
                .push(format!("keyboard menu={}", menus[index - 1].title));
            self.sync();
            return;
        }
        let tray = index - kbdnav::tray_start(menus.len());
        match Tray::ALL.get(tray) {
            Some(Tray::Wifi) => {
                let value = !self.state.borrow().wifi;
                self.quick_toggle("wifi", value);
            }
            Some(Tray::Bluetooth) => {
                let value = !self.state.borrow().bluetooth;
                self.quick_toggle("bluetooth", value);
            }
            Some(Tray::Sound) | Some(Tray::Battery) => {
                self.request_overlay(Overlay::ControlCenter);
            }
            Some(Tray::Notifications) => self.request_overlay(Overlay::Notifications),
            Some(Tray::Clock) => {
                self.log.borrow_mut().push("keyboard clock".to_string());
            }
            None => {}
        }
    }

    fn activate_control_center(&self, index: usize) {
        self.note_activation("control-center", &index.to_string());
        use kbdnav::control_center as item;
        match index {
            item::WIFI => {
                let value = !self.state.borrow().wifi;
                self.quick_toggle("wifi", value);
            }
            item::BLUETOOTH => {
                let value = !self.state.borrow().bluetooth;
                self.quick_toggle("bluetooth", value);
            }
            item::FOCUS => {
                let value = !self.state.borrow().focus_active();
                self.quick_toggle("focus", value);
            }
            item::NIGHT_LIGHT => {
                let value = !self.state.borrow().night_light;
                self.quick_toggle("night-light", value);
            }
            item::VOLUME => {
                // Enter nudges a slider by 10 % and wraps at the top.
                let value = self.state.borrow().volume;
                self.apply_volume(if value >= 0.95 { 0.0 } else { value + 0.1 });
            }
            item::BRIGHTNESS => {
                let value = self.state.borrow().brightness;
                self.apply_brightness(if value >= 0.95 { 0.0 } else { value + 0.1 });
            }
            item::POWER_MODE => {
                let next = self.state.borrow().power_mode.next();
                self.apply_power_mode(next.as_str());
            }
            item::EFFECTS => {
                let next = effects::next(self.effects.borrow().requested);
                self.apply_effect_tier(next.as_str());
            }
            item::SETTINGS => self.request_overlay(Overlay::Settings),
            item::LOCK => self.request_overlay(Overlay::Lock),
            _ => {}
        }
    }

    fn activate_notification(&self, index: usize) {
        self.note_activation("notifications", &index.to_string());
        if index == kbdnav::NOTIFICATIONS_CLEAR_ALL {
            self.state.borrow_mut().clear_notifications();
            self.sync();
            return;
        }
        let notifications: Vec<String> = self
            .state
            .borrow()
            .visible_notifications()
            .iter()
            .map(|notification| notification.id.clone())
            .collect();
        if let Some(id) = notifications.get(index - 1) {
            // Enter marks the card read; the dismiss button is the mouse path.
            self.state.borrow_mut().mark_read(id);
            self.sync();
            return;
        }
        // Last item: Focus settings.
        self.open_settings_category(Category::Notifications);
    }

    // -------------------------------------------------------- accessibility ---

    /// F1 SHL-18: resolved reduce-motion / reduce-transparency / text scale.
    pub fn a11y(&self) -> A11ySettings {
        *self.a11y.borrow()
    }

    /// Change the text scale (clamped to 100 – 200 %), persist it and resync.
    pub fn set_text_scale(&self, percent: u32) {
        match A11ySettings::save_text_scale(&self.store, percent) {
            Ok(settings) => {
                self.log.borrow_mut().push(format!(
                    "AETHER_SHELL_A11Y: text-scale={}% motion_ms={}",
                    settings.text_scale_percent(),
                    settings.motion_ms()
                ));
                *self.a11y.borrow_mut() = settings;
            }
            Err(error) => self.log.borrow_mut().push(format!("a11y: {error}")),
        }
        self.refresh_settings_rows();
        self.sync();
    }

    /// Toggle reduce motion, persist it and resync (slides collapse to a fade).
    pub fn set_reduce_motion(&self, reduce_motion: bool) {
        match A11ySettings::save_reduce_motion(&self.store, reduce_motion) {
            Ok(settings) => {
                self.log.borrow_mut().push(format!(
                    "AETHER_SHELL_A11Y: reduce-motion={} slide_ms={} fade_ms={}",
                    settings.reduce_motion as u8,
                    settings.slide_ms(),
                    settings.fade_ms()
                ));
                *self.a11y.borrow_mut() = settings;
            }
            Err(error) => self.log.borrow_mut().push(format!("a11y: {error}")),
        }
        self.refresh_settings_rows();
        self.sync();
    }

    /// Environment lines the session launcher exports for apps.
    pub fn a11y_env_lines(&self) -> Vec<String> {
        self.a11y.borrow().env_lines()
    }

    /// `--a11y-status`: settings summary plus the keyboard plan. The OK marker
    /// closes the whole report (the settings summary carries its own).
    pub fn a11y_summary(&self) -> String {
        let mut out = self.a11y.borrow().summary();
        if let Some(position) = out.rfind("AETHER_SHELL_A11Y_OK") {
            out.truncate(position);
        }
        out.push_str(&self.keyboard_plan());
        out.push_str(&self.focus().line());
        out.push('\n');
        out.push_str("AETHER_SHELL_A11Y_OK\n");
        out
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
