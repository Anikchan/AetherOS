//! Slint window binding for the Activity Manager (P5-T07, F13; P5-T15, F7 PWR-12).
//!
//! The window is presentation only: it renders [`ProcessRow`] values produced by
//! the core, reads every Energy-column string from [`EnergyColumnContract`], and
//! renders the Performance/Battery/Sensors tabs from the view models in
//! [`crate::tabs`]. That makes the published contracts the single source of
//! truth for labels, thresholds, graph values and sort direction, so changing a
//! contract changes the window without touching Slint code.
//!
//! Headless rendering (`--render`) uses the same binding as the on-device
//! window, which is how the P5 evidence screenshots are produced.

slint::include_modules!();

use aether_lifecycled::state::Band;
use aether_powerd::analytics::BatteryAnalytics;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::manager::{human_bytes, ActivityManager, ConfirmationDialog};
use crate::model::{
    EnergyColumnContract, EnergyImpact, PerformanceHistory, ProcessRow, SystemSample, Tab,
};
use crate::shortcut::ShortcutBinding;
use crate::tabs::{BatteryTabView, MetricLine, PerformanceView, SensorsTabView, TableView};

/// Height/width used for the headless evidence render.
pub const RENDER_WIDTH: u32 = 1080;
pub const RENDER_HEIGHT: u32 = 640;

/// What one live Processes refresh produced (timer tick and headless test).
#[derive(Debug, Clone)]
pub struct ProcessRefresh {
    /// Rows visible after the energy contract's sort and the empty query.
    pub visible: usize,
    /// The sampled rows, kept so a tab switch can reuse them.
    pub rows: Vec<ProcessRow>,
    /// The CPU/memory summary for this sample.
    pub system: SystemSample,
    /// The performance series, kept so the Performance tab follows the timer.
    pub history: PerformanceHistory,
}

/// Controller around the Slint component.
pub struct ActivityManagerWindow {
    ui: ActivityManagerApp,
    contract: EnergyColumnContract,
}

impl ActivityManagerWindow {
    /// Build the window with the shipped contract.
    pub fn new() -> Result<Self, slint::PlatformError> {
        let ui = ActivityManagerApp::new()?;
        Ok(Self::with_contract(ui, EnergyImpact::contract()))
    }

    /// Build the window with an explicit contract (tests, theme overrides).
    pub fn with_contract(ui: ActivityManagerApp, contract: EnergyColumnContract) -> Self {
        let window = Self { ui, contract };
        window.apply_contract();
        window
    }

    pub fn ui(&self) -> &ActivityManagerApp {
        &self.ui
    }

    /// The contract this window is bound to.
    pub fn contract(&self) -> &EnergyColumnContract {
        &self.contract
    }

    /// Re-apply a different contract (used when settings change the column).
    pub fn set_contract(&mut self, contract: EnergyColumnContract) {
        self.contract = contract;
        self.apply_contract();
    }

    fn apply_contract(&self) {
        self.ui
            .set_energy_header(self.contract.header.clone().into());
        self.ui
            .set_energy_null_label(self.contract.null_label.clone().into());
        self.ui
            .set_energy_tooltip(self.contract.tooltip.clone().into());
        self.ui
            .set_energy_sort(self.contract.default_sort.clone().into());
    }

    /// Show rows. `system` fills the CPU/memory summary line; `query` filters
    /// process names case-insensitively. Returns the number of visible rows.
    pub fn bind(
        &self,
        rows: &[ProcessRow],
        system: Option<&SystemSample>,
        query: &str,
    ) -> usize {
        let needle = query.trim().to_ascii_lowercase();
        let mut visible: Vec<&ProcessRow> = rows
            .iter()
            .filter(|row| needle.is_empty() || row.name.to_ascii_lowercase().contains(&needle)
                || row.command.to_ascii_lowercase().contains(&needle))
            .collect();
        visible.sort_by(|a, b| {
            let order = b.energy.score().partial_cmp(&a.energy.score());
            match order {
                Some(std::cmp::Ordering::Equal) | None => b
                    .cpu_percent
                    .partial_cmp(&a.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                Some(other) => other,
            }
        });
        if !self.contract.descending() {
            visible.reverse();
        }

        let views: Vec<ProcessRowView> = visible
            .iter()
            .map(|row| self.row_view(row))
            .collect();
        let count = views.len();
        self.ui.set_rows(ModelRc::new(VecModel::from(views)));
        self.ui.set_process_count(count as i32);
        if let Some(system) = system {
            self.ui.set_cpu_summary(
                format!(
                    "CPU {:.1}% · {} cores",
                    system.cpu_total_percent,
                    system.per_core_percent.len().max(1)
                )
                .into(),
            );
            self.ui.set_memory_summary(
                format!(
                    "Memory {} used of {}",
                    human_bytes(
                        system.mem_total_bytes.saturating_sub(system.mem_available_bytes)
                    ),
                    human_bytes(system.mem_total_bytes)
                )
                .into(),
            );
        }
        self.ui.set_energy_sort(self.contract.default_sort.clone().into());
        self.ui
            .set_status(format!("{count} of {} processes", rows.len()).into());
        count
    }

    /// One live refresh of the Processes tab: sample through `manager`, apply
    /// the energy contract and bind the rows. Hosts without `/proc` (the macOS
    /// dev host) fall back to the demo rows so the window still shows content.
    pub fn refresh_processes(
        &self,
        manager: &mut ActivityManager,
        analytics: Option<&BatteryAnalytics>,
        now_ms: u64,
    ) -> ProcessRefresh {
        let (mut rows, system, history) = match manager.refresh(now_ms) {
            Ok(snapshot) => (snapshot.rows, snapshot.system, snapshot.history),
            Err(_) => (
                demo_rows(),
                demo_system(),
                PerformanceHistory::default(),
            ),
        };
        if let Some(analytics) = analytics {
            manager.apply_energy_impact(&mut rows, analytics);
        }
        let visible = self.bind(&rows, Some(&system), "");
        ProcessRefresh {
            visible,
            rows,
            system,
            history,
        }
    }

    fn row_view(&self, row: &ProcessRow) -> ProcessRowView {
        let level = if self.contract.level_spec(row.energy).is_some() {
            row.energy
        } else {
            EnergyImpact::None
        };
        ProcessRowView {
            pid: row.pid as i32,
            name: row.name.clone().into(),
            state: row.state.to_string().into(),
            cpu: format!("{:.1}%", row.cpu_percent).into(),
            memory: human_bytes(row.memory_bytes).into(),
            band: row
                .band
                .map(band_label)
                .unwrap_or("")
                .into(),
            energy_label: self.contract.label_for(level).into(),
            energy_tone: self.contract.tone_for(level).into(),
            energy_share: (row.energy.score() / EnergyImpact::High.score()) as f32,
        }
    }

    /// Bind the tab bar and make `active` the visible tab.
    pub fn bind_tabs(&self, tabs: &[Tab], active: Tab) {
        let labels: Vec<slint::SharedString> = tabs
            .iter()
            .map(|tab| tab.label().into())
            .collect();
        self.ui
            .set_tab_labels(ModelRc::new(VecModel::from(labels)));
        let index = tabs
            .iter()
            .position(|tab| *tab == active)
            .unwrap_or(0);
        self.ui.set_tab_index(index as i32);
        self.ui.set_active_tab(active.key().into());
    }

    /// Bind the shortcut hint (F13 section 2, F9 AP-14).
    pub fn bind_shortcut(&self, binding: &ShortcutBinding, windows_profile: bool) {
        self.ui
            .set_shortcut_label(binding.accel(windows_profile).into());
    }

    /// Bind the Performance tab (TM-04..TM-09).
    pub fn bind_performance(&self, view: &PerformanceView) {
        let cards: Vec<GraphCardView> = view
            .cards
            .iter()
            .map(|card| GraphCardView {
                key: card.key.clone().into(),
                label: card.label.clone().into(),
                current: card.current.clone().into(),
                peak: card.peak.clone().into(),
                hint: card.hint.clone().into(),
                tone: card.tone.clone().into(),
                available: card.available,
                values: ModelRc::new(VecModel::from(card.values.clone())),
            })
            .collect();
        self.ui
            .set_graph_cards(ModelRc::new(VecModel::from(cards)));
        self.ui.set_cpu_lines(lines(&view.cpu));
        self.ui
            .set_core_bars(ModelRc::new(VecModel::from(view.per_core.clone())));
        self.ui.set_memory_lines(lines(&view.memory));
        self.ui.set_disk_lines(lines(&view.disk));
        self.ui.set_network_lines(lines(&view.network));
        self.ui.set_gpu_lines(lines(&view.gpu));
    }

    /// Bind the Battery tab (TM-11, P5-T16 history).
    pub fn bind_battery(&self, view: &BatteryTabView) {
        self.ui.set_battery_percent(view.percent.clone().into());
        self.ui.set_battery_state(view.state.clone().into());
        self.ui.set_battery_lines(lines(&[
            MetricLine {
                label: "Power draw".into(),
                value: view.power.clone(),
            },
            MetricLine {
                label: "Time remaining".into(),
                value: view.time_remaining.clone(),
            },
            MetricLine {
                label: "Health".into(),
                value: view.health.clone(),
            },
            MetricLine {
                label: "Cycle count".into(),
                value: view.cycles.clone(),
            },
            MetricLine {
                label: "Estimated cycles".into(),
                value: view.estimated_cycles.clone(),
            },
            MetricLine {
                label: "Temperature".into(),
                value: view.temperature.clone(),
            },
        ]));
        let apps: Vec<AppEnergyView> = view
            .apps
            .iter()
            .map(|app| AppEnergyView {
                app: app.app.clone().into(),
                energy: app.energy.clone().into(),
                share: app.share,
                label: app.label.clone().into(),
                tone: app.tone.clone().into(),
            })
            .collect();
        self.ui
            .set_battery_apps(ModelRc::new(VecModel::from(apps)));
        self.ui.set_battery_health_trend(ModelRc::new(VecModel::from(
            view.health_trend.clone(),
        )));
        self.ui
            .set_battery_history_label(view.history_label.clone().into());
        self.ui
            .set_battery_note(view.empty_reason.clone().unwrap_or_default().into());
    }

    /// Bind the Sensors tab (TM-10).
    pub fn bind_sensors(&self, view: &SensorsTabView) {
        self.ui
            .set_temperature_sensors(sensor_rows(view, true));
        self.ui.set_fan_sensors(sensor_rows(view, false));
        self.ui.set_sensor_gpu_lines(lines(&view.gpu));
        let alerts: Vec<slint::SharedString> =
            view.alerts.iter().map(|alert| alert.as_str().into()).collect();
        self.ui
            .set_sensor_alerts(ModelRc::new(VecModel::from(alerts)));
        let unavailable: Vec<slint::SharedString> = view
            .unavailable
            .iter()
            .map(|note| note.as_str().into())
            .collect();
        self.ui
            .set_sensor_unavailable(ModelRc::new(VecModel::from(unavailable)));
    }

    /// Bind a generic table tab (App history, Startup apps, Users, Details,
    /// Services). Call `bind_tabs` first; this selects the shared table view.
    pub fn bind_table(&self, view: &TableView) {
        self.ui.set_table_title(view.title.clone().into());
        let headers: Vec<slint::SharedString> =
            view.headers.iter().map(|header| header.as_str().into()).collect();
        self.ui
            .set_table_headers(ModelRc::new(VecModel::from(headers)));
        let rows: Vec<TableRowView> = view
            .rows
            .iter()
            .map(|row| TableRowView {
                cells: ModelRc::new(VecModel::from(
                    row.cells
                        .iter()
                        .map(|cell| slint::SharedString::from(cell.as_str()))
                        .collect::<Vec<_>>(),
                )),
                tone: row.tone.clone().into(),
                hint: row.hint.clone().into(),
                actions: ModelRc::new(VecModel::from(
                    row.actions
                        .iter()
                        .map(|action| RowActionView {
                            id: action.id.clone().into(),
                            label: action.label.clone().into(),
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect();
        self.ui.set_table_rows(ModelRc::new(VecModel::from(rows)));
        self.ui
            .set_table_summary(view.summary.clone().into());
        self.ui.set_table_note(view.note.clone().into());
        self.ui
            .set_table_empty_text(view.empty_text.clone().into());
        self.ui.set_active_tab("table".into());
    }

    /// Register the handler for a table row action (Startup apps enable/disable).
    pub fn on_row_action<F>(&self, handler: F)
    where
        F: Fn(String) + 'static,
    {
        self.ui.on_row_action(move |action| {
            handler(action.to_string());
        });
    }

    /// Show the end-task / force-quit dialog for `pid` (TM-02, TM-AC-03).
    pub fn show_dialog(&self, pid: u32, dialog: &ConfirmationDialog) {
        self.ui.set_dialog_pid(pid as i32);
        self.ui.set_dialog_title(dialog.title.clone().into());
        self.ui.set_dialog_message(dialog.message.clone().into());
        self.ui
            .set_dialog_confirm_label(dialog.confirm_label.clone().into());
        self.ui
            .set_dialog_cancel_label(dialog.cancel_label.clone().into());
        self.ui.set_dialog_hung(dialog.hung);
        self.ui.set_dialog_visible(true);
    }

    pub fn hide_dialog(&self) {
        self.ui.set_dialog_visible(false);
    }

    pub fn dialog_visible(&self) -> bool {
        self.ui.get_dialog_visible()
    }

    /// Register the handler for the dialog buttons: cancel does nothing,
    /// "End task" sends SIGTERM and "Force Quit" sends SIGKILL.
    pub fn on_end_task<F>(&self, handler: F)
    where
        F: Fn(u32, bool) + 'static,
    {
        let weak = self.ui.as_weak();
        self.ui.on_dialog_choice(move |choice| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_dialog_visible(false);
            if choice == 0 {
                return;
            }
            let pid = ui.get_dialog_pid().max(0) as u32;
            handler(pid, choice == 2);
        });
    }

    /// Register the shortcut handler (the window only sees it while focused;
    /// the shell registers the global accelerator from the same contract).
    pub fn on_shortcut<F>(&self, handler: F)
    where
        F: Fn() + 'static,
    {
        self.ui.on_shortcut_activated(handler);
    }
}

fn lines(values: &[MetricLine]) -> ModelRc<MetricLineView> {
    let views: Vec<MetricLineView> = values
        .iter()
        .map(|line| MetricLineView {
            label: line.label.clone().into(),
            value: line.value.clone().into(),
        })
        .collect();
    ModelRc::new(VecModel::from(views))
}

fn sensor_rows(view: &SensorsTabView, temperatures: bool) -> ModelRc<SensorRowView> {
    let rows: &[crate::tabs::SensorRowView] = if temperatures {
        &view.temperatures
    } else {
        &view.fans
    };
    let views: Vec<SensorRowView> = rows
        .iter()
        .map(|row| SensorRowView {
            name: row.name.clone().into(),
            value: row.value.clone().into(),
            tone: row.tone.clone().into(),
        })
        .collect();
    ModelRc::new(VecModel::from(views))
}

fn band_label(band: Band) -> &'static str {
    match band {
        Band::System => "System",
        Band::Foreground => "Foreground",
        Band::Visible => "Visible",
        Band::Protected => "Protected",
        Band::Background => "Background",
        Band::Suspended => "Suspended",
        Band::Hibernated => "Hibernated",
        Band::Terminated => "Terminated",
    }
}

/// Sample system values for `--render` on hosts without `/proc`, so the
/// evidence screenshots show laid-out graphs instead of empty cards. Every
/// value is demo data and is documented as such.
pub fn demo_system() -> SystemSample {
    let mut system = SystemSample {
        cpu_total_percent: 24.0,
        per_core_percent: vec![18.0, 42.0, 12.0, 30.0, 8.0, 55.0, 21.0, 14.0],
        cpu_mhz: Some(2800),
        governor: Some("schedutil".into()),
        uptime_secs: 7_420,
        process_count: 187,
        mem_total_bytes: 16 * 1024 * 1024 * 1024,
        mem_available_bytes: 10 * 1024 * 1024 * 1024,
        mem_cached_bytes: 5 * 1024 * 1024 * 1024,
        swap_total_bytes: 4 * 1024 * 1024 * 1024,
        swap_free_bytes: 4 * 1024 * 1024 * 1024,
        zram_used_bytes: Some(420 * 1024 * 1024),
        disk_read_bytes_per_sec: 2.4e6,
        disk_write_bytes_per_sec: 0.9e6,
        network_rx_bytes_per_sec: 320e3,
        network_tx_bytes_per_sec: 48e3,
        gpu_busy_percent: Some(14.0),
        gpu_memory_used_bytes: Some(512 * 1024 * 1024),
        ..Default::default()
    };
    system.temperatures_c.insert("coretemp/Package id 0".into(), 52.0);
    system.temperatures_c.insert("nvme/Composite".into(), 38.5);
    system
}

/// 60 one-second points for the Performance tab when no `/proc` exists.
pub fn demo_history(system: &SystemSample) -> PerformanceHistory {
    let mut history = PerformanceHistory::default();
    for step in 0..60u64 {
        let wave = (step % 20) as f32;
        let sample = SystemSample {
            cpu_total_percent: 12.0 + wave * 1.6,
            mem_total_bytes: system.mem_total_bytes,
            mem_available_bytes: system.mem_available_bytes.saturating_sub(step * 4_000_000),
            disk_read_bytes_per_sec: 1.2e6 + f64::from(wave) * 9.0e4,
            disk_write_bytes_per_sec: 0.5e6 + f64::from(wave) * 4.0e4,
            network_rx_bytes_per_sec: 220e3 + f64::from(wave) * 9.0e3,
            network_tx_bytes_per_sec: 30e3 + f64::from(wave) * 1.5e3,
            gpu_busy_percent: Some(6.0 + wave * 0.7),
            ..Default::default()
        };
        history.push(&sample, step * 1_000);
    }
    history
}

/// Sample autostart entries for `--render` (documented demo data).
pub fn demo_startup() -> Vec<crate::model::StartupItem> {
    use crate::model::{StartupItem, StartupScope};
    vec![
        StartupItem {
            name: "Aether Mail".into(),
            exec: "aether-mail --background".into(),
            desktop_file: "/home/demo/.config/autostart/aether-mail.desktop".into(),
            enabled: true,
            scope: StartupScope::User,
        },
        StartupItem {
            name: "Backup".into(),
            exec: "aether-backup --daily".into(),
            desktop_file: "/etc/xdg/autostart/aether-backup.desktop".into(),
            enabled: true,
            scope: StartupScope::System,
        },
        StartupItem {
            name: "Podcast sync".into(),
            exec: "aether-podcasts --sync".into(),
            desktop_file: "/home/demo/.config/autostart/podcasts.desktop".into(),
            enabled: false,
            scope: StartupScope::User,
        },
    ]
}

/// Sample systemd units for `--render` (documented demo data).
pub fn demo_services() -> Vec<crate::model::ServiceRow> {
    use crate::model::ServiceRow;
    [
        ("aether-powerd.service", "active", "running", "AetherOS power daemon"),
        ("aether-lifecycled.service", "active", "running", "AetherOS lifecycle daemon"),
        ("aether-compositor.service", "active", "running", "AetherOS Wayland compositor"),
        ("aether-updated.service", "inactive", "dead", "AetherOS update service"),
        ("systemd-timesyncd.service", "failed", "failed", "Network time synchronisation"),
    ]
    .into_iter()
    .map(|(unit, active, sub, description)| ServiceRow {
        unit: unit.into(),
        load: "loaded".into(),
        active: active.into(),
        sub: sub.into(),
        description: description.into(),
    })
    .collect()
}

/// Sample logind sessions for `--render` (documented demo data).
pub fn demo_users() -> Vec<crate::model::UserSession> {
    use crate::model::UserSession;
    vec![
        UserSession {
            user: "demo".into(),
            uid: 1000,
            seat: Some("seat0".into()),
        },
        UserSession {
            user: "build".into(),
            uid: 1001,
            seat: None,
        },
    ]
}

/// Sample Pack D history for the App history evidence render (documented demo
/// data). It runs the real integrators — [`AppCpuTime::observe`] and
/// [`AppNetworkHistory::update`] — hourly for seven days, so the screenshot
/// exercises the same roll-up code AetherOS will run.
pub fn demo_pack_d() -> (crate::pack_d::AppCpuTime, crate::pack_d::AppNetworkHistory) {
    use crate::pack_d::{AppCpuTime, AppNetworkCounters, AppNetworkHistory};
    use std::collections::BTreeMap;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let start_ms = now_ms.saturating_sub(7 * 24 * 3_600_000);
    let apps = [
        ("org.aether.Browser", 62.0f32, 9_400_000u64),
        ("org.aether.Player", 18.0, 1_200_000),
        ("org.aether.Files", 3.0, 120_000),
    ];
    let mut cpu = AppCpuTime::default();
    let mut network = AppNetworkHistory::default();
    let mut counters: BTreeMap<&str, (u64, u64)> = apps
        .iter()
        .map(|(app, _, _)| (*app, (0, 0)))
        .collect();
    for hour in 0..(7 * 24u64) {
        let at = start_ms + hour * 3_600_000;
        let rows: Vec<ProcessRow> = apps
            .iter()
            .enumerate()
            .map(|(index, (app, cpu_percent, _))| ProcessRow {
                pid: 4_200 + index as u32,
                ppid: 1,
                name: app.rsplit('.').next().unwrap_or(app).to_lowercase(),
                command: format!("/usr/bin/{app}"),
                user: Some(1000),
                threads: 4,
                open_files: 16,
                state: 'S',
                cpu_percent: *cpu_percent,
                memory_bytes: 64 * 1024 * 1024,
                disk_read_bytes: 0,
                disk_write_bytes: 0,
                network_tx_bytes: 0,
                network_rx_bytes: 0,
                gpu_percent: None,
                energy: EnergyImpact::Low,
                wakeups: 0,
                efficiency: false,
                app_state: None,
                band: None,
                cgroup: Some(format!(
                    "/aether.slice/aether-apps.slice/aether-app-{app}-1a2b3c4d.scope"
                )),
            })
            .collect();
        cpu.observe(&rows, at);
        for (app, _, rx_per_hour) in apps {
            let total = counters.get_mut(app).expect("counter exists");
            total.0 += rx_per_hour;
            total.1 += rx_per_hour / 4;
        }
        let snapshot: Vec<AppNetworkCounters> = counters
            .iter()
            .map(|(app, (rx, tx))| AppNetworkCounters {
                app: (*app).to_string(),
                rx_bytes: *rx,
                tx_bytes: *tx,
            })
            .collect();
        network.update(&snapshot, at);
    }
    (cpu, network)
}

/// Sample sensors for `--render` on hosts without hwmon/thermal sysfs.
pub fn demo_sensors() -> Vec<crate::model::SensorReading> {    vec![
        crate::model::SensorReading {
            name: "coretemp/Package id 0".into(),
            kind: crate::model::SensorKind::TemperatureC,
            value: 52.0,
        },
        crate::model::SensorReading {
            name: "nvme/Composite".into(),
            kind: crate::model::SensorKind::TemperatureC,
            value: 38.5,
        },
        crate::model::SensorReading {
            name: "BAT0/temp".into(),
            kind: crate::model::SensorKind::TemperatureC,
            value: 30.5,
        },
        crate::model::SensorReading {
            name: "thinkpad/fan1".into(),
            kind: crate::model::SensorKind::FanRpm,
            value: 2_800.0,
        },
    ]
}

/// Rows used by `--render` when the host has no `/proc` (evidence on macOS).
/// They are labelled as sample data in the status line.
pub fn demo_rows() -> Vec<ProcessRow> {
    vec![
        demo_row(4210, "aether-browser", 'R', 62.5, 980 * 1024 * 1024, EnergyImpact::High, Some(Band::Foreground)),
        demo_row(4311, "aether-player", 'S', 18.2, 420 * 1024 * 1024, EnergyImpact::Medium, Some(Band::Visible)),
        demo_row(4088, "aether-files", 'S', 3.4, 180 * 1024 * 1024, EnergyImpact::Low, None),
        demo_row(1220, "aether-lifecycled", 'S', 1.1, 42 * 1024 * 1024, EnergyImpact::Low, Some(Band::System)),
        demo_row(4902, "aether-bench", 'S', 0.0, 12 * 1024 * 1024, EnergyImpact::None, Some(Band::Background)),
    ]
}

fn demo_row(
    pid: u32,
    name: &str,
    state: char,
    cpu_percent: f32,
    memory_bytes: u64,
    energy: EnergyImpact,
    band: Option<Band>,
) -> ProcessRow {
    // Deterministic demo cgroups: app rows look like sandboxed app scopes,
    // service rows like system units, so the Details tab shows both levels.
    let cgroup = if matches!(name, "aether-lifecycled" | "aether-bench") {
        format!("/aether.slice/aether-system.slice/{name}.service")
    } else {
        let id = name.trim_start_matches("aether-");
        let mut chars = id.chars();
        let app = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => id.to_string(),
        };
        format!("/aether.slice/aether-apps.slice/aether-app-org.aether.{app}-1a2b3c4d.scope")
    };
    ProcessRow {
        pid,
        ppid: 1,
        name: name.into(),
        command: format!("/usr/bin/{name}"),
        user: Some(1000),
        threads: 4,
        open_files: 32,
        state,
        cpu_percent,
        memory_bytes,
        disk_read_bytes: 0,
        disk_write_bytes: 0,
        network_tx_bytes: 0,
        network_rx_bytes: 0,
        gpu_percent: None,
        energy,
        wakeups: 12,
        efficiency: false,
        app_state: None,
        band,
        cgroup: Some(cgroup),
    }
}
