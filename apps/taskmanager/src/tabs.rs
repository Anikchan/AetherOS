//! Tab view models for the Activity Manager (P5-T07; F13 TM-04..TM-11).
//!
//! The Slint window is presentation only: every graph value, label and
//! fallback below is computed here and tested headlessly. Missing readings are
//! reported as "Not reported" instead of zero (D10, F13 section 6), and the
//! Energy labels in the Battery tab come from the same P5-T15 contract as the
//! Processes column.

use aether_powerd::analytics::BatteryAnalytics;

use crate::collect::pressure_label;
use crate::manager::human_bytes;
use crate::model::{
    BatteryView, EnergyColumnContract, EnergyImpact, GraphPoint, PerformanceHistory,
    SensorKind, SensorReading, SystemSample,
};
use crate::pack_d::{AppCpuTime, AppNetworkHistory, PackDContract};
use crate::service_actions::{
    action_id, ServiceAction, ServiceActionContract,
};

/// Label used wherever a reading is unavailable (D10, F14 honesty rules).
pub const NOT_REPORTED: &str = "Not reported";

/// Sensors tab alert thresholds in degrees Celsius (F13 TM-10).
pub const TEMP_WARN_C: f32 = 85.0;
pub const TEMP_CRIT_C: f32 = 95.0;

/// Highest number of apps shown in the Battery tab's energy table.
pub const BATTERY_APP_ROWS: usize = 6;

/// One graph in the Performance tab (TM-04).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphCard {
    pub key: String,
    pub label: String,
    /// Live value, pre-formatted ("12.5%", "9.8 MB/s").
    pub current: String,
    /// Peak over the 60-second window, pre-formatted.
    pub peak: String,
    /// Normalised 0..1 values, oldest first; empty when unavailable.
    pub values: Vec<f32>,
    /// Design token name for the line colour.
    pub tone: String,
    pub available: bool,
    /// Fallback explanation shown under the graph.
    pub hint: String,
}

/// A label/value line in a detail column or table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricLine {
    pub label: String,
    pub value: String,
}

/// Everything the Performance tab renders (TM-04..TM-09).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerformanceView {
    pub cards: Vec<GraphCard>,
    pub cpu: Vec<MetricLine>,
    /// Per-core busy share, normalised 0..1 (TM-05).
    pub per_core: Vec<f32>,
    pub memory: Vec<MetricLine>,
    pub disk: Vec<MetricLine>,
    pub network: Vec<MetricLine>,
    pub gpu: Vec<MetricLine>,
}

/// One app row in the Battery tab's energy table (BAT-08, P5-T15).
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryAppRow {
    pub app: String,
    /// Pre-formatted energy for the label ("3.4 Wh").
    pub energy: String,
    /// Share of the top consumer, 0..1.
    pub share: f32,
    /// Level from the energy contract.
    pub level: EnergyImpact,
    pub label: String,
    pub tone: String,
}

/// Everything the Battery tab renders (TM-11, P5-T16 history).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BatteryTabView {
    pub percent: String,
    pub state: String,
    pub power: String,
    pub time_remaining: String,
    pub health: String,
    pub cycles: String,
    pub estimated_cycles: String,
    pub temperature: String,
    pub apps: Vec<BatteryAppRow>,
    /// Normalised health trend, oldest first (BAT-02).
    pub health_trend: Vec<f32>,
    pub history_label: String,
    /// Set when there is no powerd data at all.
    pub empty_reason: Option<String>,
}

/// One sensor row in the Sensors tab (TM-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorRowView {
    pub name: String,
    pub value: String,
    pub tone: String,
}

/// Everything the Sensors tab renders (TM-10; GPU from TM-09).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SensorsTabView {
    pub temperatures: Vec<SensorRowView>,
    pub fans: Vec<SensorRowView>,
    pub gpu: Vec<MetricLine>,
    /// Threshold crossings, worded for display.
    pub alerts: Vec<String>,
    /// Explicit fallbacks for readings this system does not expose.
    pub unavailable: Vec<String>,
}

/// Build the Performance tab from one system sample and its 60 s history.
pub fn performance_view(system: &SystemSample, history: &PerformanceHistory) -> PerformanceView {
    let mem_used = system
        .mem_total_bytes
        .saturating_sub(system.mem_available_bytes);
    let mem_ceiling = system.mem_total_bytes.max(1) as f32;
    let disk_peak = peak(&history.disk);
    let net_peak = peak(&history.network);

    let gpu_available = system.gpu_busy_percent.is_some();
    let cards = vec![
        GraphCard {
            key: "cpu".into(),
            label: "CPU".into(),
            current: format!("{:.1}%", system.cpu_total_percent),
            peak: format!("Peak {:.1}%", peak(&history.cpu)),
            values: normalize(&history.cpu, 100.0),
            tone: "accent".into(),
            available: true,
            hint: match &system.governor {
                Some(governor) => format!("Governor {governor}"),
                None => format!("Governor {NOT_REPORTED}"),
            },
        },
        GraphCard {
            key: "memory".into(),
            label: "Memory".into(),
            current: human_bytes(mem_used),
            peak: format!("Peak {}", human_bytes(peak(&history.memory) as u64)),
            values: normalize(&history.memory, mem_ceiling),
            tone: "accent".into(),
            available: true,
            hint: format!("{} total", human_bytes(system.mem_total_bytes)),
        },
        GraphCard {
            key: "disk".into(),
            label: "Disk".into(),
            current: rate(system.disk_read_bytes_per_sec + system.disk_write_bytes_per_sec),
            peak: format!("Peak {}", rate(f64::from(disk_peak))),
            values: normalize(&history.disk, disk_peak),
            tone: "success".into(),
            available: true,
            hint: format!(
                "{} read · {} write",
                rate(system.disk_read_bytes_per_sec),
                rate(system.disk_write_bytes_per_sec)
            ),
        },
        GraphCard {
            key: "network".into(),
            label: "Network".into(),
            current: rate(system.network_rx_bytes_per_sec + system.network_tx_bytes_per_sec),
            peak: format!("Peak {}", rate(f64::from(net_peak))),
            values: normalize(&history.network, net_peak),
            tone: "warning".into(),
            available: true,
            hint: format!(
                "{} in · {} out",
                rate(system.network_rx_bytes_per_sec),
                rate(system.network_tx_bytes_per_sec)
            ),
        },
        GraphCard {
            key: "gpu".into(),
            label: "GPU".into(),
            current: system
                .gpu_busy_percent
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| NOT_REPORTED.into()),
            peak: if gpu_available {
                format!("Peak {:.0}%", peak(&history.gpu))
            } else {
                NOT_REPORTED.into()
            },
            values: normalize(&history.gpu, 100.0),
            tone: "accent".into(),
            available: gpu_available,
            hint: if gpu_available {
                "Per-process GPU use is Pack D".into()
            } else {
                format!("GPU engine use {NOT_REPORTED} by this driver")
            },
        },
    ];

    let core_count = system.per_core_percent.len();
    let cpu = vec![
        MetricLine {
            label: "Total".into(),
            value: format!("{:.1}%", system.cpu_total_percent),
        },
        MetricLine {
            label: "Cores".into(),
            value: core_count.to_string(),
        },
        MetricLine {
            label: "Clock".into(),
            value: system
                .cpu_mhz
                .map(|mhz| format!("{:.2} GHz", f64::from(mhz) / 1000.0))
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
        MetricLine {
            label: "Governor".into(),
            value: system
                .governor
                .clone()
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
        MetricLine {
            label: "Uptime".into(),
            value: duration(system.uptime_secs),
        },
        MetricLine {
            label: "Processes".into(),
            value: system.process_count.to_string(),
        },
    ];

    let memory = vec![
        MetricLine {
            label: "Used".into(),
            value: human_bytes(mem_used),
        },
        MetricLine {
            label: "Available".into(),
            value: human_bytes(system.mem_available_bytes),
        },
        MetricLine {
            label: "Cached".into(),
            value: human_bytes(system.mem_cached_bytes),
        },
        MetricLine {
            label: "Compressed (zram)".into(),
            value: system
                .zram_used_bytes
                .map(human_bytes)
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
        MetricLine {
            label: "Swap".into(),
            value: format!(
                "{} of {}",
                human_bytes(system.swap_total_bytes.saturating_sub(system.swap_free_bytes)),
                human_bytes(system.swap_total_bytes)
            ),
        },
        MetricLine {
            label: "Pressure".into(),
            value: pressure_label(system.pressure_level).into(),
        },
    ];

    let disk = vec![
        MetricLine {
            label: "Read".into(),
            value: rate(system.disk_read_bytes_per_sec),
        },
        MetricLine {
            label: "Write".into(),
            value: rate(system.disk_write_bytes_per_sec),
        },
    ];

    let network = vec![
        MetricLine {
            label: "Received".into(),
            value: rate(system.network_rx_bytes_per_sec),
        },
        MetricLine {
            label: "Sent".into(),
            value: rate(system.network_tx_bytes_per_sec),
        },
    ];

    let gpu = vec![
        MetricLine {
            label: "Engine".into(),
            value: system
                .gpu_busy_percent
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
        MetricLine {
            label: "Memory".into(),
            value: system
                .gpu_memory_used_bytes
                .map(human_bytes)
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
        MetricLine {
            label: "Per-process".into(),
            value: "Pack D (not installed)".into(),
        },
    ];

    PerformanceView {
        cards,
        cpu,
        per_core: system
            .per_core_percent
            .iter()
            .map(|value| (value / 100.0).clamp(0.0, 1.0))
            .collect(),
        memory,
        disk,
        network,
        gpu,
    }
}

/// Build the Battery tab. `status` is the live powerd status (or sysfs) view;
/// `analytics` is the F14 history store behind the 24-hour energy table.
pub fn battery_view(
    status: Option<&BatteryView>,
    analytics: Option<&BatteryAnalytics>,
    now_epoch_secs: u64,
) -> BatteryTabView {
    let contract = EnergyColumnContract::default();
    let mut view = BatteryTabView {
        percent: NOT_REPORTED.into(),
        state: NOT_REPORTED.into(),
        power: NOT_REPORTED.into(),
        time_remaining: NOT_REPORTED.into(),
        health: NOT_REPORTED.into(),
        cycles: NOT_REPORTED.into(),
        estimated_cycles: NOT_REPORTED.into(),
        temperature: NOT_REPORTED.into(),
        history_label: "Energy by app, last 24 hours (F14 analytics)".into(),
        ..Default::default()
    };

    if let Some(status) = status {
        view.percent = format!("{:.0}%", status.percent);
        view.state = if status.charging {
            "Charging".into()
        } else {
            "Discharging".into()
        };
        view.power = status
            .power_w
            .map(|watts| format!("{watts:.1} W"))
            .unwrap_or_else(|| NOT_REPORTED.into());
        view.time_remaining = status
            .time_remaining_secs
            .map(duration)
            .unwrap_or_else(|| NOT_REPORTED.into());
        view.health = status
            .health_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| NOT_REPORTED.into());
        view.cycles = status
            .cycle_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| NOT_REPORTED.into());
        view.temperature = status
            .temperature_c
            .map(|value| format!("{value:.1} °C"))
            .unwrap_or_else(|| NOT_REPORTED.into());
    }

    if let Some(analytics) = analytics {
        if view.health == NOT_REPORTED {
            if let Some(health) = analytics.health_percent() {
                view.health = format!("{health:.1}%");
            }
        }
        if view.cycles == NOT_REPORTED {
            if let Some(cycles) = analytics.cycle_count() {
                view.cycles = cycles.to_string();
            }
        }
        if let Some(cycles) = analytics.estimated_cycles() {
            view.estimated_cycles = format!("{cycles:.1} (Estimate)");
        }

        let totals = analytics.energy_by_app(now_epoch_secs, 1);
        let max = totals.first().map(|(_, wh)| *wh).unwrap_or(0.0);
        for (app, wh) in totals.iter().take(BATTERY_APP_ROWS) {
            let share = if max > 0.0 { wh / max } else { 0.0 };
            let level = contract.level_for(share, *wh);
            view.apps.push(BatteryAppRow {
                app: app.clone(),
                energy: format!("{wh:.2} Wh"),
                share: share as f32,
                level,
                label: contract.label_for(level),
                tone: contract.tone_for(level),
            });
        }

        view.health_trend = analytics
            .health_trend()
            .into_iter()
            .map(|(_, health)| ((health - 50.0) / 50.0).clamp(0.0, 1.0))
            .collect();
    }

    if status.is_none() && analytics.is_none() {
        view.empty_reason = Some(
            "No battery data: pass --battery-status (powerd status.json) and --power-history (battery-history.json)."
                .into(),
        );
    } else if view.apps.is_empty() {
        view.empty_reason = Some(format!(
            "No per-app energy recorded yet; labels use \"{}\" until history exists.",
            contract.null_label
        ));
    }

    view
}

/// Build the Sensors tab from hwmon/thermal readings and the system sample.
pub fn sensors_view(sensors: &[SensorReading], system: &SystemSample) -> SensorsTabView {
    let mut view = SensorsTabView::default();

    let mut readings: Vec<&SensorReading> = sensors.iter().collect();
    readings.sort_by(|a, b| a.name.cmp(&b.name));
    for sensor in readings {
        match sensor.kind {
            SensorKind::TemperatureC => {
                let tone = if sensor.value >= TEMP_CRIT_C {
                    "danger"
                } else if sensor.value >= TEMP_WARN_C {
                    "warning"
                } else {
                    "success"
                };
                if sensor.value >= TEMP_WARN_C {
                    view.alerts.push(format!(
                        "{} is {:.1} °C (alert threshold {:.0} °C)",
                        sensor.name, sensor.value, TEMP_WARN_C
                    ));
                }
                view.temperatures.push(SensorRowView {
                    name: sensor.name.clone(),
                    value: format!("{:.1} °C", sensor.value),
                    tone: tone.into(),
                });
            }
            SensorKind::FanRpm => view.fans.push(SensorRowView {
                name: sensor.name.clone(),
                value: format!("{:.0} RPM", sensor.value),
                tone: "accent".into(),
            }),
        }
    }

    if view.temperatures.is_empty() {
        view.unavailable
            .push(format!("Temperature {NOT_REPORTED} by this system"));
    }
    if view.fans.is_empty() {
        view.unavailable
            .push("Fan speed is hidden, not shown as zero.".into());
    }

    view.gpu = vec![
        MetricLine {
            label: "Engine".into(),
            value: system
                .gpu_busy_percent
                .map(|value| format!("{value:.0}%"))
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
        MetricLine {
            label: "Memory".into(),
            value: system
                .gpu_memory_used_bytes
                .map(human_bytes)
                .unwrap_or_else(|| NOT_REPORTED.into()),
        },
    ];
    if system.gpu_busy_percent.is_none() {
        view.unavailable
            .push("GPU engine use is not exposed by this driver.".into());
    }

    view
}

/// Text form of the Performance tab, for CLI evidence and D6 logs.
pub fn performance_report(view: &PerformanceView) -> String {
    let mut out = String::from("Performance (60 s window)\n");
    for card in &view.cards {
        out.push_str(&format!(
            "  {:<8} {:>10}  {}  [{} samples]\n",
            card.label,
            card.current,
            card.peak,
            card.values.len()
        ));
        out.push_str(&format!("           {}\n", card.hint));
    }
    for (title, lines) in [
        ("CPU", &view.cpu),
        ("Memory", &view.memory),
        ("Disk", &view.disk),
        ("Network", &view.network),
        ("GPU", &view.gpu),
    ] {
        out.push_str(&format!("\n{title}\n"));
        for line in lines {
            out.push_str(&format!("  {:<20} {}\n", line.label, line.value));
        }
    }
    out
}

/// Text form of the Battery tab, for CLI evidence and D6 logs.
pub fn battery_report(view: &BatteryTabView) -> String {
    let mut out = String::from("Battery\n");
    for (label, value) in [
        ("Charge", &view.percent),
        ("State", &view.state),
        ("Power draw", &view.power),
        ("Time remaining", &view.time_remaining),
        ("Health", &view.health),
        ("Cycle count", &view.cycles),
        ("Estimated cycles", &view.estimated_cycles),
        ("Temperature", &view.temperature),
    ] {
        out.push_str(&format!("  {:<20} {}\n", label, value));
    }
    out.push_str(&format!("\n{}\n", view.history_label));
    for app in &view.apps {
        out.push_str(&format!(
            "  {:<28} {:>10}  {} ({:.0}%)\n",
            app.app,
            app.energy,
            app.label,
            app.share * 100.0
        ));
    }
    if let Some(reason) = &view.empty_reason {
        out.push_str(&format!("  {reason}\n"));
    }
    out
}

/// Text form of the Sensors tab, for CLI evidence and D6 logs.
pub fn sensors_report(view: &SensorsTabView) -> String {
    let mut out = String::from("Sensors\n");
    for row in view.temperatures.iter().chain(view.fans.iter()) {
        out.push_str(&format!("  {:<32} {}\n", row.name, row.value));
    }
    for line in &view.gpu {
        out.push_str(&format!("  GPU {:<28} {}\n", line.label, line.value));
    }
    for alert in &view.alerts {
        out.push_str(&format!("  ALERT {alert}\n"));
    }
    for note in &view.unavailable {
        out.push_str(&format!("  {note}\n"));
    }
    out
}

/// One per-row button in a generic table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowAction {
    /// Action id the caller resolves (a `.desktop` path or `service:<action>:<unit>`).
    pub id: String,
    pub label: String,
}

/// One row in a generic Activity Manager table (TM-12/13/14/16/17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub cells: Vec<String>,
    /// Design token name for the status cell (last column).
    pub tone: String,
    /// Secondary line under the first cell (command, description, exec).
    pub hint: String,
    /// Row actions in display order; read-only rows have none.
    pub actions: Vec<RowAction>,
}

impl TableRow {
    pub fn new(cells: Vec<String>) -> Self {
        Self {
            cells,
            tone: String::new(),
            hint: String::new(),
            actions: Vec::new(),
        }
    }

    pub fn with_tone(mut self, tone: &str) -> Self {
        self.tone = tone.to_string();
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    pub fn with_action(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.actions.push(RowAction {
            id: id.into(),
            label: label.into(),
        });
        self
    }

    pub fn with_actions(mut self, actions: Vec<RowAction>) -> Self {
        self.actions = actions;
        self
    }
}

/// A whole tab rendered as one table (App history, Startup apps, Users,
/// Details, Services).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableView {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<TableRow>,
    pub summary: String,
    /// Honesty note about what the table does and does not show.
    pub note: String,
    pub empty_text: String,
}

impl Default for TableView {
    fn default() -> Self {
        Self {
            title: String::new(),
            headers: Vec::new(),
            rows: Vec::new(),
            summary: String::new(),
            note: String::new(),
            empty_text: "No rows reported.".into(),
        }
    }
}

/// Highest number of rows the generic tables render.
pub const TABLE_ROWS: usize = 25;

/// App history (TM-12): per-app energy for the last 24 hours and 7 days from
/// the F14 analytics store, plus CPU time and network totals from the Pack D
/// contract when it is installed. Missing sources keep the contract's null
/// label instead of showing zero.
pub fn app_history_view(
    analytics: Option<&BatteryAnalytics>,
    cpu: Option<&AppCpuTime>,
    network: Option<&AppNetworkHistory>,
    now_epoch_secs: u64,
) -> TableView {
    let contract = EnergyColumnContract::default();
    let pack_d = PackDContract::published();
    let has_pack_d = cpu.is_some() || network.is_some();
    let mut view = TableView {
        title: "APP HISTORY".into(),
        headers: vec![
            "App".into(),
            "24 hours".into(),
            "7 days".into(),
            "CPU time".into(),
            "Network".into(),
            "Impact".into(),
            "Share".into(),
        ],
        note: if has_pack_d {
            "Energy is measured by powerd (F14). CPU time integrates the 1 s process samples and network sums the Pack D socket-accounting deltas, both rolled up per hour (D10 §5.2)."
                .into()
        } else {
            "Energy is measured by powerd (F14). Per-app CPU time and network need Pack D (advanced monitoring) and read \"Not reported\" until it is installed."
                .into()
        },
        empty_text: if has_pack_d {
            "No per-app history recorded in the last 7 days (no battery history and no Pack D samples).".into()
        } else {
            "No battery history: pass --power-history (powerd battery-history.json).".into()
        },
        ..Default::default()
    };

    let week_start_secs = now_epoch_secs.saturating_sub(7 * 86_400);
    let mut entries: std::collections::BTreeMap<String, HistoryEntry> =
        std::collections::BTreeMap::new();

    if let Some(analytics) = analytics {
        let day = analytics.energy_by_app(now_epoch_secs, 1);
        let week = analytics.energy_by_app(now_epoch_secs, 7);
        let day_by_app: std::collections::BTreeMap<&str, f64> = day
            .iter()
            .map(|(app, wh)| (app.as_str(), *wh))
            .collect();
        for (app, wh) in &week {
            let entry = entries.entry(app.clone()).or_default();
            entry.week_wh = Some(*wh);
            entry.day_wh = Some(day_by_app.get(app.as_str()).copied().unwrap_or(0.0));
        }
    }

    if let Some(cpu) = cpu {
        for app in cpu.apps() {
            entries.entry(app.clone()).or_default().cpu_secs = Some(cpu.total_seconds_for(
                &app,
                week_start_secs * 1_000,
            ));
        }
    }
    if let Some(network) = network {
        for app in network.apps() {
            entries
                .entry(app.clone())
                .or_default()
                .network = Some(network.totals_for(&app, week_start_secs * 1_000));
        }
    }

    if entries.is_empty() {
        if has_pack_d {
            view.empty_text = "No Pack D samples or battery history in the last 7 days.".into();
        }
        return view;
    }

    let max_week = entries
        .values()
        .filter_map(|entry| entry.week_wh)
        .fold(0.0_f64, f64::max);
    let total: f64 = entries.values().filter_map(|entry| entry.week_wh).sum();
    // `f64::sum` starts at -0.0, which would print as "-0.0 Wh" when no energy
    // was measured; report a plain zero instead.
    let total = if total == 0.0 { 0.0 } else { total };

    let mut ordered: Vec<(String, HistoryEntry)> = entries.into_iter().collect();
    ordered.sort_by(|(app_a, a), (app_b, b)| {
        b.week_wh
            .unwrap_or(0.0)
            .partial_cmp(&a.week_wh.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.cpu_secs
                    .unwrap_or(0.0)
                    .partial_cmp(&a.cpu_secs.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| app_a.cmp(app_b))
    });

    view.rows = ordered
        .into_iter()
        .take(TABLE_ROWS)
        .map(|(app, entry)| {
            let week_wh = entry.week_wh.unwrap_or(0.0);
            let share = if max_week > 0.0 { week_wh / max_week } else { 0.0 };
            let level = contract.level_for(share, week_wh);
            let energy_cell = |wh: Option<f64>| match wh {
                Some(wh) => format!("{wh:.2} Wh"),
                None => contract.null_label.clone(),
            };
            let cpu_cell = match entry.cpu_secs {
                Some(secs) if secs > 0.0 && secs < 1.0 => "<1 s".into(),
                Some(secs) => duration(secs.max(0.0) as u64),
                None => pack_d.null_label.clone(),
            };
            let network_cell = match entry.network {
                Some((rx, tx)) => format!(
                    "{} in · {} out",
                    human_bytes(rx),
                    human_bytes(tx)
                ),
                None => pack_d.null_label.clone(),
            };
            TableRow::new(vec![
                app,
                energy_cell(entry.day_wh),
                energy_cell(entry.week_wh),
                cpu_cell,
                network_cell,
                contract.label_for(level),
                if week_wh > 0.0 && max_week > 0.0 {
                    format!("{:.0}%", share * 100.0)
                } else {
                    "—".into()
                },
            ])
            .with_tone(&contract.tone_for(level))
        })
        .collect();
    view.summary = format!(
        "{} apps · {total:.1} Wh in 7 days · top consumer shown at 100% share{}",
        view.rows.len(),
        if has_pack_d {
            " · CPU/network from Pack D"
        } else {
            ""
        }
    );
    view
}

/// One app's 7-day history entry, merged from every available source.
#[derive(Debug, Clone, Default)]
struct HistoryEntry {
    day_wh: Option<f64>,
    week_wh: Option<f64>,
    cpu_secs: Option<f64>,
    network: Option<(u64, u64)>,
}

/// Startup apps (TM-13): autostart entries with an honest impact column and a
/// per-row enable/disable action (the caller persists it).
pub fn startup_view(items: &[crate::model::StartupItem]) -> TableView {
    let mut view = TableView {
        title: "STARTUP APPS".into(),
        headers: vec![
            "Name".into(),
            "Scope".into(),
            "State".into(),
            "Startup impact".into(),
        ],
        note: "Startup impact needs a boot trace and currently reads \"Not measured\" instead of a guess. Enabling or disabling writes Hidden= in the .desktop file (TM-13)."
            .into(),
        empty_text: "No autostart entries found (~/.config/autostart and /etc/xdg/autostart)."
            .into(),
        ..Default::default()
    };
    let enabled = items.iter().filter(|item| item.enabled).count();
    view.rows = items
        .iter()
        .take(TABLE_ROWS)
        .map(|item| {
            TableRow::new(vec![
                item.name.clone(),
                match item.scope {
                    crate::model::StartupScope::User => "User".into(),
                    crate::model::StartupScope::System => "System".into(),
                },
                if item.enabled {
                    "Enabled".into()
                } else {
                    "Disabled".into()
                },
                "Not measured".into(),
            ])
            .with_tone(if item.enabled { "success" } else { "text_muted" })
            .with_hint(&item.exec)
            .with_action(
                &item.desktop_file,
                if item.enabled { "Disable" } else { "Enable" },
            )
        })
        .collect();
    view.summary = format!(
        "{enabled} of {} enabled · user and system autostart",
        items.len()
    );
    view
}

/// Logged-in users (TM-14) from `loginctl list-users`.
pub fn users_view(sessions: &[crate::model::UserSession], current_uid: u32) -> TableView {
    let mut view = TableView {
        title: "USERS".into(),
        headers: vec![
            "User".into(),
            "UID".into(),
            "Seat".into(),
            "Session".into(),
        ],
        note: "From loginctl list-users (systemd-logind). A missing seat means the session is not attached to a graphical seat.".into(),
        empty_text: "No logged-in users reported (is logind running?).".into(),
        ..Default::default()
    };
    view.rows = sessions
        .iter()
        .take(TABLE_ROWS)
        .map(|session| {
            let current = session.uid == current_uid;
            TableRow::new(vec![
                session.user.clone(),
                session.uid.to_string(),
                session.seat.clone().unwrap_or_else(|| "—".into()),
                if current { "This session" } else { "Active" }.into(),
            ])
            .with_tone(if current { "accent" } else { "" })
        })
        .collect();
    view.summary = format!("{} logged-in users", view.rows.len());
    view
}

/// Sandbox level for a process row, derived from its cgroup (TM-16).
pub fn sandbox_level(row: &crate::model::ProcessRow) -> &'static str {
    match row.cgroup.as_deref() {
        Some(cgroup) if crate::manager::app_id_from_scope(cgroup).is_some() => "App scope",
        Some(cgroup) if cgroup.contains("aether") => "Aether system scope",
        Some(_) => "System",
        None => "Unknown",
    }
}

/// Details tab (TM-16): the Windows-style process table.
pub fn details_view(rows: &[crate::model::ProcessRow]) -> TableView {
    let mut view = TableView {
        title: "DETAILS".into(),
        headers: vec![
            "Name".into(),
            "PID".into(),
            "PPID".into(),
            "User".into(),
            "Threads".into(),
            "Files".into(),
            "Sandbox".into(),
        ],
        note: "Private memory uses PSS where the sampler read smaps_rollup. The sandbox column is derived from the cgroup: aether-app-* scopes are sandboxed app scopes, system services are not.".into(),
        empty_text: "No processes reported yet.".into(),
        ..Default::default()
    };
    let mut sorted: Vec<&crate::model::ProcessRow> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    view.rows = sorted
        .into_iter()
        .take(TABLE_ROWS)
        .map(|row| {
            let level = sandbox_level(row);
            TableRow::new(vec![
                row.name.clone(),
                row.pid.to_string(),
                row.ppid.to_string(),
                row.user
                    .map(|uid| uid.to_string())
                    .unwrap_or_else(|| "—".into()),
                row.threads.to_string(),
                row.open_files.to_string(),
                level.into(),
            ])
            .with_tone(if level == "App scope" { "accent" } else { "text_muted" })
            .with_hint(&row.command)
        })
        .collect();
    view.summary = format!("{} processes · sorted by CPU", view.rows.len());
    view
}

/// Services tab (TM-17): systemd units with their status and the Start, Stop
/// and Restart actions. Every action is planned through the permission-prompt
/// contract ([`ServiceActionContract`]); nothing runs from this view.
pub fn services_view(services: &[crate::model::ServiceRow]) -> TableView {
    services_view_with(services, &ServiceActionContract::default())
}

/// Same as [`services_view`] with an explicit contract, so a theme or policy
/// override changes the buttons and the note together.
pub fn services_view_with(
    services: &[crate::model::ServiceRow],
    contract: &ServiceActionContract,
) -> TableView {
    let mut view = TableView {
        title: "SERVICES".into(),
        headers: vec![
            "Unit".into(),
            "Load".into(),
            "Active".into(),
            "Sub".into(),
        ],
        note: format!(
            "Start, Stop and Restart open the administrator permission prompt and show the exact systemctl command before anything runs ({}). Execution is enabled on AetherOS; this build plans the action only.",
            contract.permission
        ),
        empty_text: "systemctl reported no services (this host may not use systemd).".into(),
        ..Default::default()
    };
    let active = services
        .iter()
        .filter(|service| service.active == "active")
        .count();
    view.rows = services
        .iter()
        .take(TABLE_ROWS)
        .map(|service| {
            let tone = if service.active == "active" {
                "success"
            } else if service.active == "failed" {
                "danger"
            } else {
                "text_muted"
            };
            let actions = if service.active == "active" {
                vec![ServiceAction::Stop, ServiceAction::Restart]
            } else {
                vec![ServiceAction::Start, ServiceAction::Restart]
            };
            let actions: Vec<RowAction> = actions
                .into_iter()
                .filter_map(|action| {
                    contract.spec(action).map(|spec| RowAction {
                        id: action_id(&service.unit, action),
                        label: spec.label.clone(),
                    })
                })
                .collect();
            TableRow::new(vec![
                service.unit.clone(),
                service.load.clone(),
                service.active.clone(),
                service.sub.clone(),
            ])
            .with_tone(tone)
            .with_hint(&service.description)
            .with_actions(actions)
        })
        .collect();
    view.summary = format!("{active} active of {}", services.len());
    view
}

/// Text form of a generic table, for the CLI and D6 logs.
pub fn table_report(view: &TableView) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n", view.title));
    if !view.summary.is_empty() {
        out.push_str(&format!("  {}\n", view.summary));
    }
    out.push('\n');
    out.push_str(&format!("  {:<32}", view.headers.first().cloned().unwrap_or_default()));
    for header in view.headers.iter().skip(1) {
        out.push_str(&format!(" {:>14}", header));
    }
    out.push('\n');
    if view.rows.is_empty() {
        out.push_str(&format!("  {}\n", view.empty_text));
    }
    for row in &view.rows {
        let mut cells = row.cells.iter();
        out.push_str(&format!(
            "  {:<32}",
            cells.next().cloned().unwrap_or_default()
        ));
        for cell in cells {
            out.push_str(&format!(" {:>14}", cell));
        }
        out.push('\n');
        if !row.hint.is_empty() {
            out.push_str(&format!("      {}\n", row.hint));
        }
        for action in &row.actions {
            out.push_str(&format!("      [{}] {}\n", action.label, action.id));
        }
    }
    if !view.note.is_empty() {
        out.push_str(&format!("\n  {}\n", view.note));
    }
    out
}

/// Peak value of a history series (0.0 when empty).
pub fn peak(points: &[GraphPoint]) -> f32 {
    points
        .iter()
        .map(|point| point.value)
        .fold(0.0_f32, f32::max)
}

/// Normalise a series into 0..1 against a ceiling.
pub fn normalize(points: &[GraphPoint], ceiling: f32) -> Vec<f32> {
    let ceiling = if ceiling > 0.0 { ceiling } else { 1.0 };
    points
        .iter()
        .map(|point| (point.value / ceiling).clamp(0.0, 1.0))
        .collect()
}

fn rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1e9 {
        format!("{:.1} GB/s", bytes_per_sec / 1e9)
    } else if bytes_per_sec >= 1e6 {
        format!("{:.1} MB/s", bytes_per_sec / 1e6)
    } else if bytes_per_sec >= 1e3 {
        format!("{:.1} KB/s", bytes_per_sec / 1e3)
    } else {
        format!("{bytes_per_sec:.0} B/s")
    }
}

/// Short human duration ("3 h 12 m", "12 m 04 s").
pub fn duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours} h {minutes:02} m")
    } else if minutes > 0 {
        format!("{minutes} m {seconds:02} s")
    } else {
        format!("{seconds} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SensorReading;
    use aether_powerd::analytics::BatteryAnalytics;

    fn history() -> PerformanceHistory {
        let mut history = PerformanceHistory::default();
        let sample = SystemSample {
            cpu_total_percent: 25.0,
            mem_total_bytes: 8_000,
            mem_available_bytes: 3_000,
            disk_read_bytes_per_sec: 100.0,
            disk_write_bytes_per_sec: 50.0,
            network_rx_bytes_per_sec: 10.0,
            network_tx_bytes_per_sec: 5.0,
            gpu_busy_percent: Some(40.0),
            ..Default::default()
        };
        history.push(&sample, 1_000);
        let sample = SystemSample {
            cpu_total_percent: 50.0,
            ..sample
        };
        history.push(&sample, 2_000);
        history
    }

    #[test]
    fn performance_cards_normalise_against_the_window_ceiling() {
        let mut system = SystemSample {
            cpu_total_percent: 50.0,
            per_core_percent: vec![25.0, 75.0],
            mem_total_bytes: 8_000,
            mem_available_bytes: 3_000,
            mem_cached_bytes: 1_000,
            swap_total_bytes: 2_000,
            swap_free_bytes: 1_500,
            disk_read_bytes_per_sec: 100.0,
            disk_write_bytes_per_sec: 50.0,
            network_rx_bytes_per_sec: 10.0,
            network_tx_bytes_per_sec: 5.0,
            gpu_busy_percent: Some(40.0),
            gpu_memory_used_bytes: Some(2_048),
            cpu_mhz: Some(3400),
            governor: Some("powersave".into()),
            uptime_secs: 3_725,
            process_count: 42,
            zram_used_bytes: Some(512),
            ..Default::default()
        };
        system.temperatures_c.insert("coretemp".into(), 55.0);

        let view = performance_view(&system, &history());
        assert_eq!(view.cards.len(), 5);
        assert_eq!(view.cards[0].key, "cpu");
        assert_eq!(view.cards[0].current, "50.0%");
        // 50% busy -> half the graph height, and the history peaked at 50.
        assert_eq!(view.cards[0].values.last().copied(), Some(0.5));
        assert_eq!(view.cards[0].peak, "Peak 50.0%");
        // Memory is drawn against MemTotal, not the window peak.
        assert_eq!(view.cards[1].values.last().copied(), Some(5000.0 / 8000.0));
        // Disk is drawn against its own window peak (150 B/s here).
        assert_eq!(view.cards[2].values.last().copied(), Some(150.0 / 150.0));
        assert!(view.cards[4].available);
        assert_eq!(view.cards[4].values.last().copied(), Some(0.4));

        assert_eq!(view.per_core, vec![0.25, 0.75]);
        let clock = view
            .cpu
            .iter()
            .find(|line| line.label == "Clock")
            .unwrap();
        assert_eq!(clock.value, "3.40 GHz");
        let uptime = view.cpu.iter().find(|line| line.label == "Uptime").unwrap();
        assert_eq!(uptime.value, "1 h 02 m");
        // Swap used = 500 of 2000.
        let swap = view
            .memory
            .iter()
            .find(|line| line.label == "Swap")
            .unwrap();
        assert_eq!(swap.value, "500 B of 2.0 KB");
        assert!(view.disk.iter().any(|line| line.value == "100 B/s"));
    }

    #[test]
    fn performance_reports_missing_gpu_and_governor_instead_of_zero() {
        let system = SystemSample {
            mem_total_bytes: 1_000,
            ..Default::default()
        };
        let view = performance_view(&system, &PerformanceHistory::default());
        let gpu = view.cards.iter().find(|card| card.key == "gpu").unwrap();
        assert!(!gpu.available);
        assert_eq!(gpu.current, NOT_REPORTED);
        assert!(gpu.values.is_empty());
        assert!(gpu.hint.contains(NOT_REPORTED));
        let governor = view
            .cpu
            .iter()
            .find(|line| line.label == "Governor")
            .unwrap();
        assert_eq!(governor.value, NOT_REPORTED);
        assert_eq!(
            view.gpu.iter().find(|line| line.label == "Engine").unwrap().value,
            NOT_REPORTED
        );
    }

    #[test]
    fn battery_tab_uses_history_and_contract_levels() {
        let status = BatteryView {
            percent: 79.0,
            charging: false,
            power_w: Some(9.8),
            time_remaining_secs: Some(3_780),
            cycle_count: Some(120),
            health_percent: Some(88.5),
            temperature_c: Some(30.5),
        };
        let mut analytics = BatteryAnalytics::new(10);
        analytics.record_app_energy(1_000, "org.example.Browser", 18.0);
        analytics.record_app_energy(1_000, "org.example.Mail", 2.0);

        let view = battery_view(Some(&status), Some(&analytics), 1_100);
        assert_eq!(view.percent, "79%");
        assert_eq!(view.state, "Discharging");
        assert_eq!(view.power, "9.8 W");
        assert_eq!(view.time_remaining, "1 h 03 m");
        assert_eq!(view.health, "88.5%");
        assert_eq!(view.cycles, "120");
        assert_eq!(view.temperature, "30.5 °C");
        assert_eq!(view.apps.len(), 2);
        assert_eq!(view.apps[0].app, "org.example.Browser");
        assert_eq!(view.apps[0].level, EnergyImpact::High);
        assert_eq!(view.apps[0].label, "High");
        assert_eq!(view.apps[0].tone, "danger");
        assert_eq!(view.apps[1].level, EnergyImpact::Low);
        assert_eq!(view.apps[0].share, 1.0);
        assert!(view.empty_reason.is_none());
    }

    #[test]
    fn battery_tab_explains_when_history_is_empty() {
        let mut analytics = BatteryAnalytics::new(10);
        analytics.record_app_energy(1_000, "org.example.Browser", 3.0);
        let view = battery_view(None, Some(&analytics), 1_000 + 2 * 86_400);
        // The old entry falls outside the 24-hour window.
        assert!(view.apps.is_empty());
        assert_eq!(view.percent, NOT_REPORTED);
        assert!(view.empty_reason.unwrap().contains("Not reported"));

        let empty = battery_view(None, None, 0);
        assert!(empty.empty_reason.unwrap().contains("No battery data"));
    }

    #[test]
    fn sensors_tab_sorts_and_flags_threshold_crossings() {
        let sensors = vec![
            SensorReading {
                name: "coretemp/Package id 0".into(),
                kind: SensorKind::TemperatureC,
                value: 96.0,
            },
            SensorReading {
                name: "acpitz/temp1".into(),
                kind: SensorKind::TemperatureC,
                value: 40.0,
            },
            SensorReading {
                name: "thinkpad/fan1".into(),
                kind: SensorKind::FanRpm,
                value: 2_800.0,
            },
        ];
        let system = SystemSample {
            gpu_busy_percent: Some(12.0),
            gpu_memory_used_bytes: Some(1_048_576),
            ..Default::default()
        };
        let view = sensors_view(&sensors, &system);
        assert_eq!(view.temperatures.len(), 2);
        // Sorted by name: acpitz before coretemp.
        assert_eq!(view.temperatures[0].name, "acpitz/temp1");
        assert_eq!(view.temperatures[0].tone, "success");
        assert_eq!(view.temperatures[1].tone, "danger");
        assert_eq!(view.temperatures[1].value, "96.0 °C");
        assert_eq!(view.fans[0].value, "2800 RPM");
        assert_eq!(view.alerts.len(), 1);
        assert!(view.alerts[0].contains("coretemp/Package id 0"));
        assert!(view.unavailable.is_empty());
        assert_eq!(view.gpu[0].value, "12%");
    }

    #[test]
    fn sensors_tab_shows_fallbacks_but_not_fake_zeros() {
        let view = sensors_view(&[], &SystemSample::default());
        assert!(view.temperatures.is_empty());
        assert!(view.fans.is_empty());
        assert_eq!(view.unavailable.len(), 3);
        assert!(view
            .unavailable
            .iter()
            .any(|text| text.contains("Fan speed is hidden")));
    }

    #[test]
    fn duration_formats_hours_minutes_and_seconds() {
        assert_eq!(duration(9), "9 s");
        assert_eq!(duration(64), "1 m 04 s");
        assert_eq!(duration(3_725), "1 h 02 m");
    }

    #[test]
    fn app_history_merges_24_hour_and_week_totals() {
        let mut analytics = BatteryAnalytics::new(64);
        let now = 10 * 86_400;
        analytics.record_app_energy(now - 60, "org.aether.Browser", 18.0);
        analytics.record_app_energy(now - 2 * 86_400, "org.aether.Browser", 6.0);
        analytics.record_app_energy(now - 60, "org.aether.Player", 2.0);
        analytics.record_app_energy(now - 0, "org.aether.Player", 0.0);

        let view = app_history_view(Some(&analytics), None, None, now);
        assert_eq!(view.title, "APP HISTORY");
        assert_eq!(view.headers[0], "App");
        assert_eq!(view.headers[3], "CPU time");
        assert_eq!(view.headers[4], "Network");
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].cells[0], "org.aether.Browser");
        assert_eq!(view.rows[0].cells[1], "18.00 Wh");
        assert_eq!(view.rows[0].cells[2], "24.00 Wh");
        // No Pack D on this host: the columns say so instead of showing zero.
        assert_eq!(view.rows[0].cells[3], "Not reported");
        assert_eq!(view.rows[0].cells[4], "Not reported");
        assert_eq!(view.rows[0].cells[5], "High");
        assert_eq!(view.rows[0].tone, "danger");
        assert_eq!(view.rows[1].cells[5], "Low");
        assert!(view.summary.contains("26.0 Wh in 7 days"));
        assert!(view.note.contains("Pack D"));
        assert!(!view.summary.contains("Pack D"));

        // Without history the tab explains what to pass.
        let empty = app_history_view(None, None, None, now);
        assert!(empty.rows.is_empty());
        assert!(empty.empty_text.contains("--power-history"));
        // A store with only stale entries is reported as empty, not as zero.
        let stale = app_history_view(Some(&BatteryAnalytics::new(8)), None, None, now);
        assert!(stale.empty_text.contains("No battery history"));
    }

    #[test]
    fn app_history_adds_pack_d_cpu_and_network_columns() {
        use crate::model::ProcessRow;
        use crate::pack_d::{AppCpuTime, AppNetworkCounters, AppNetworkHistory};

        let now = 10 * 86_400u64;
        let now_ms = now * 1_000;
        let cgroup = "/aether.slice/aether-apps.slice/aether-app-org.aether.Browser-12345678.scope";
        let row = ProcessRow {
            pid: 10,
            ppid: 1,
            name: "browser".into(),
            command: "/usr/bin/browser".into(),
            user: Some(1000),
            threads: 4,
            open_files: 8,
            state: 'R',
            cpu_percent: 50.0,
            memory_bytes: 1024,
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
            cgroup: Some(cgroup.into()),
        };
        let mut cpu = AppCpuTime::default();
        // Prime, then one hour at 50% = 1800 CPU-seconds.
        cpu.observe(std::slice::from_ref(&row), now_ms - 3_600_000);
        cpu.observe(&[row], now_ms);
        assert!((cpu.total_seconds_for("org.aether.Browser", 0) - 1_800.0).abs() < 1e-6);

        let mut network = AppNetworkHistory::default();
        network.update(
            &[AppNetworkCounters {
                app: "org.aether.Browser".into(),
                rx_bytes: 1_000,
                tx_bytes: 500,
            }],
            now_ms - 3_600_000,
        );
        network.update(
            &[AppNetworkCounters {
                app: "org.aether.Browser".into(),
                rx_bytes: 1_000_000,
                tx_bytes: 500_000,
            }],
            now_ms,
        );

        let mut analytics = BatteryAnalytics::new(8);
        analytics.record_app_energy(now - 60, "org.aether.Browser", 18.0);
        let view = app_history_view(Some(&analytics), Some(&cpu), Some(&network), now);
        assert_eq!(view.rows.len(), 1);
        let cells = &view.rows[0].cells;
        assert_eq!(cells[0], "org.aether.Browser");
        assert_eq!(cells[3], "30 m 00 s");
        assert_eq!(cells[4], "975.6 KB in · 487.8 KB out");
        assert_eq!(cells[5], "High");
        assert!(view.summary.contains("CPU/network from Pack D"));
        assert!(view.note.contains("D10"));
        assert!(!view.note.contains("need Pack D"));

        // Pack D apps with no energy still appear, with the null label for the
        // energy columns instead of a fake zero.
        let mut other_cpu = AppCpuTime::default();
        let mut other_row = view_source_row();
        other_row.cpu_percent = 10.0;
        other_row.cgroup = Some(
            "/aether.slice/aether-apps.slice/aether-app-org.aether.Other-87654321.scope".into(),
        );
        other_cpu.observe(&[other_row.clone()], now_ms - 1_000);
        other_cpu.observe(&[other_row], now_ms);
        let view = app_history_view(
            Some(&BatteryAnalytics::new(8)),
            Some(&other_cpu),
            Some(&AppNetworkHistory::default()),
            now,
        );
        let other = view
            .rows
            .iter()
            .find(|row| row.cells[0] == "org.aether.Other")
            .unwrap();
        assert_eq!(other.cells[1], "Not reported");
        assert_eq!(other.cells[3], "<1 s");
        assert_eq!(other.cells[4], "Not reported");
        assert_eq!(other.cells[6], "—");
    }

    fn view_source_row() -> crate::model::ProcessRow {
        crate::model::ProcessRow {
            pid: 11,
            ppid: 1,
            name: "other".into(),
            command: "/usr/bin/other".into(),
            user: Some(1000),
            threads: 1,
            open_files: 2,
            state: 'S',
            cpu_percent: 0.0,
            memory_bytes: 1024,
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
            cgroup: None,
        }
    }

    #[test]
    fn startup_view_shows_state_and_toggle_actions() {
        use crate::model::{StartupItem, StartupScope};
        let items = vec![
            StartupItem {
                name: "Mail".into(),
                exec: "aether-mail".into(),
                desktop_file: "/home/u/.config/autostart/mail.desktop".into(),
                enabled: true,
                scope: StartupScope::User,
            },
            StartupItem {
                name: "Backup".into(),
                exec: "aether-backup".into(),
                desktop_file: "/etc/xdg/autostart/backup.desktop".into(),
                enabled: false,
                scope: StartupScope::System,
            },
        ];
        let view = startup_view(&items);
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].cells, vec!["Mail", "User", "Enabled", "Not measured"]);
        assert_eq!(view.rows[0].actions.len(), 1);
        assert_eq!(view.rows[0].actions[0].label, "Disable");
        assert_eq!(view.rows[0].actions[0].id, "/home/u/.config/autostart/mail.desktop");
        assert_eq!(view.rows[0].tone, "success");
        assert_eq!(view.rows[1].cells[1], "System");
        assert_eq!(view.rows[1].actions[0].label, "Enable");
        assert_eq!(view.rows[1].tone, "text_muted");
        assert!(view.summary.contains("1 of 2 enabled"));
        assert!(view.note.contains("Not measured"));
    }

    #[test]
    fn users_view_marks_the_current_session() {
        use crate::model::UserSession;
        let sessions = vec![
            UserSession {
                user: "anik".into(),
                uid: 1000,
                seat: Some("seat0".into()),
            },
            UserSession {
                user: "builder".into(),
                uid: 1001,
                seat: None,
            },
        ];
        let view = users_view(&sessions, 1000);
        assert_eq!(view.rows[0].cells, vec!["anik", "1000", "seat0", "This session"]);
        assert_eq!(view.rows[0].tone, "accent");
        assert_eq!(view.rows[1].cells[3], "Active");
        assert_eq!(view.rows[1].cells[2], "—");
        assert!(view.summary.contains("2 logged-in users"));
    }

    #[test]
    fn details_view_sorts_by_cpu_and_names_the_sandbox_level() {
        use crate::model::{EnergyImpact, ProcessRow};
        let row = |pid: u32, name: &str, cpu: f32, cgroup: Option<&str>| ProcessRow {
            pid,
            ppid: 1,
            name: name.into(),
            command: format!("/usr/bin/{name} --flag"),
            user: Some(1000),
            threads: 3,
            open_files: 12,
            state: 'S',
            cpu_percent: cpu,
            memory_bytes: 1024,
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
            cgroup: cgroup.map(str::to_string),
        };
        let rows = vec![
            row(10, "idle", 0.5, None),
            row(11, "browser", 40.0, Some("/aether.slice/aether-app-org.aether.Browser-12345678.scope")),
            row(12, "sshd", 1.0, Some("/system.slice/sshd.service")),
        ];
        let view = details_view(&rows);
        assert_eq!(view.rows[0].cells[0], "browser");
        assert_eq!(view.rows[0].cells[6], "App scope");
        assert_eq!(view.rows[0].tone, "accent");
        assert_eq!(view.rows[0].hint, "/usr/bin/browser --flag");
        assert_eq!(view.rows[1].cells[0], "sshd");
        assert_eq!(view.rows[1].cells[6], "System");
        assert_eq!(view.rows[2].cells[6], "Unknown");
        assert_eq!(view.rows[2].cells[3], "1000");
    }

    #[test]
    fn services_view_counts_active_units_and_offers_prompted_actions() {
        use crate::model::ServiceRow;
        let services = vec![
            ServiceRow {
                unit: "aether-powerd.service".into(),
                load: "loaded".into(),
                active: "active".into(),
                sub: "running".into(),
                description: "AetherOS power daemon".into(),
            },
            ServiceRow {
                unit: "aether-lifecycled.service".into(),
                load: "loaded".into(),
                active: "failed".into(),
                sub: "failed".into(),
                description: "AetherOS lifecycle daemon".into(),
            },
        ];
        let view = services_view(&services);
        assert_eq!(view.rows[0].cells[0], "aether-powerd.service");
        assert_eq!(view.rows[0].tone, "success");
        assert_eq!(view.rows[1].tone, "danger");
        assert_eq!(view.rows[1].hint, "AetherOS lifecycle daemon");
        assert!(view.summary.contains("1 active of 2"));
        assert!(view.note.contains("administrator permission prompt"));
        assert!(view.note.contains("admin.authorize"));

        // A running service offers Stop and Restart; a stopped one Start and
        // Restart. Every id carries the unit so the prompt can plan it.
        assert_eq!(
            view.rows[0]
                .actions
                .iter()
                .map(|action| (action.label.as_str(), action.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Stop", "service:stop:aether-powerd.service"),
                ("Restart", "service:restart:aether-powerd.service"),
            ]
        );
        assert_eq!(
            view.rows[1]
                .actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "service:start:aether-lifecycled.service",
                "service:restart:aether-lifecycled.service",
            ]
        );

        // A contract without the Stop action hides that button.
        let mut contract = ServiceActionContract::default();
        contract.actions.retain(|spec| spec.action != ServiceAction::Stop);
        let trimmed = services_view_with(&services, &contract);
        assert_eq!(trimmed.rows[0].actions.len(), 1);
        assert_eq!(trimmed.rows[0].actions[0].id, "service:restart:aether-powerd.service");
    }

    #[test]
    fn table_report_renders_headers_rows_and_notes() {
        use crate::model::UserSession;
        let view = users_view(
            &[UserSession {
                user: "anik".into(),
                uid: 1000,
                seat: Some("seat0".into()),
            }],
            1000,
        );
        let report = table_report(&view);
        assert!(report.contains("USERS"));
        assert!(report.contains("This session"));
        assert!(report.contains("loginctl"));
    }

    #[test]
    fn text_reports_name_every_section() {        let system = SystemSample {
            mem_total_bytes: 2_048,
            mem_available_bytes: 1_024,
            ..Default::default()
        };
        let performance = performance_report(&performance_view(&system, &PerformanceHistory::default()));
        for section in ["Performance (60 s window)", "CPU", "Memory", "Disk", "Network", "GPU"] {
            assert!(performance.contains(section), "missing {section}");
        }

        let battery = battery_report(&battery_view(None, None, 0));
        assert!(battery.contains("No battery data"));

        let sensors = sensors_report(&sensors_view(&[], &SystemSample::default()));
        assert!(sensors.contains("Temperature Not reported"));
        assert!(sensors.contains("Fan speed is hidden"));
    }
}
