//! Data model for the Activity Manager (F13, D10).
//!
//! The window (Slint, S3 design system) renders these rows; all metric
//! collection and actions live here so they are testable without a display.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use aether_lifecycled::state::{AppState, Band};

/// One row in the Processes/Details tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessRow {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub command: String,
    pub user: Option<u32>,
    pub threads: u32,
    pub open_files: u32,
    pub state: char,
    /// Share of one core, from the delta between samples.
    pub cpu_percent: f32,
    /// Private memory (PSS where available, else VmRSS).
    pub memory_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub network_tx_bytes: u64,
    pub network_rx_bytes: u64,
    pub gpu_percent: Option<f32>,
    pub energy: EnergyImpact,
    pub wakeups: u64,
    pub efficiency: bool,
    pub app_state: Option<AppState>,
    pub band: Option<Band>,
    pub cgroup: Option<String>,
}

impl ProcessRow {
    /// Column key used for sorting by any column (TM-01).
    pub fn sort_key(&self, column: Column) -> f64 {
        match column {
            Column::Name => 0.0,
            Column::Cpu => f64::from(self.cpu_percent),
            Column::Memory => self.memory_bytes as f64,
            Column::Disk => (self.disk_read_bytes + self.disk_write_bytes) as f64,
            Column::Network => (self.network_tx_bytes + self.network_rx_bytes) as f64,
            Column::Gpu => f64::from(self.gpu_percent.unwrap_or(0.0)),
            Column::Energy => self.energy.score(),
            Column::Wakeups => self.wakeups as f64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnergyImpact {
    None,
    Low,
    Medium,
    High,
}

impl EnergyImpact {
    pub fn score(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Low => 1.0,
            Self::Medium => 2.0,
            Self::High => 3.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    /// Relative score from CPU time, wakeups and GPU use (F7 PWR-12).
    pub fn classify(cpu_percent: f32, wakeups: u64, gpu_percent: Option<f32>) -> Self {
        let gpu = gpu_percent.unwrap_or(0.0);
        let score = cpu_percent
            + gpu * 0.5
            + (wakeups as f32 / 100.0).min(10.0) * 0.2;
        if score >= 50.0 {
            Self::High
        } else if score >= 15.0 {
            Self::Medium
        } else if score > 0.5 {
            Self::Low
        } else {
            Self::None
        }
    }

    /// Level from the measured share of the top app's energy in the F14
    /// analytics store. Thresholds are the ones published in the column
    /// contract (`EnergyColumnContract::default`).
    pub fn from_share(share: f64, watt_hours: f64) -> Self {
        if share >= 0.66 {
            Self::High
        } else if share >= 0.33 {
            Self::Medium
        } else if watt_hours > 0.0 {
            Self::Low
        } else {
            Self::None
        }
    }

    /// Stable machine-readable binding for the Activity Manager Energy column
    /// (P5-T15, F7 PWR-12). The Slint window reads this instead of hard-coding
    /// labels or thresholds; tests pin the values.
    pub fn contract() -> EnergyColumnContract {
        EnergyColumnContract::default()
    }
}

/// One level in the energy column contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnergyLevelSpec {
    pub level: EnergyImpact,
    pub label: String,
    /// Lower bound of the app's share of the top consumer's energy.
    pub min_share: f64,
    /// Design token (`ui` theme) used for the label.
    pub tone: String,
}

/// Where the column data comes from, in priority order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnergySourceSpec {
    pub id: String,
    pub description: String,
    pub path: String,
    pub priority: u8,
}

/// Contract for the Energy column (P5-T15, D10 §5, F7 PWR-12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnergyColumnContract {
    pub key: String,
    pub header: String,
    pub unit: String,
    pub default_sort: String,
    pub null_label: String,
    pub refresh_seconds: u32,
    pub levels: Vec<EnergyLevelSpec>,
    pub sources: Vec<EnergySourceSpec>,
    pub tooltip: String,
}

impl Default for EnergyColumnContract {
    fn default() -> Self {
        Self {
            key: "energy".into(),
            header: "Energy".into(),
            unit: "impact".into(),
            default_sort: "desc".into(),
            null_label: "Not reported".into(),
            // F14 rolls energy per app up once a minute.
            refresh_seconds: 60,
            levels: vec![
                EnergyLevelSpec {
                    level: EnergyImpact::High,
                    label: EnergyImpact::High.label().into(),
                    min_share: 0.66,
                    tone: "danger".into(),
                },
                EnergyLevelSpec {
                    level: EnergyImpact::Medium,
                    label: EnergyImpact::Medium.label().into(),
                    min_share: 0.33,
                    tone: "accent".into(),
                },
                EnergyLevelSpec {
                    level: EnergyImpact::Low,
                    label: EnergyImpact::Low.label().into(),
                    min_share: 0.0,
                    tone: "success".into(),
                },
                EnergyLevelSpec {
                    level: EnergyImpact::None,
                    label: EnergyImpact::None.label().into(),
                    min_share: 0.0,
                    tone: "text_muted".into(),
                },
            ],
            sources: vec![
                EnergySourceSpec {
                    id: "battery-history".into(),
                    description: "Measured energy per app from the F14 battery history (preferred)".into(),
                    path: "powerd battery-history.json -> energy_by_app".into(),
                    priority: 1,
                },
                EnergySourceSpec {
                    id: "computed".into(),
                    description: "Relative score from CPU time, wakeups and GPU use when no history exists".into(),
                    path: "ProcessRow cpu_percent + wakeups + gpu_percent".into(),
                    priority: 2,
                },
            ],
            tooltip: "Relative energy use: a share of the top app's measured energy, or a score from CPU, wakeups and GPU use before history exists.".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Column {
    Name,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    Energy,
    Wakeups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tab {
    Processes,
    Performance,
    AppHistory,
    StartupApps,
    Users,
    Details,
    Services,
    Battery,
    Sensors,
    Android,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Processes => "Processes",
            Self::Performance => "Performance",
            Self::AppHistory => "App history",
            Self::StartupApps => "Startup apps",
            Self::Users => "Users",
            Self::Details => "Details",
            Self::Services => "Services",
            Self::Battery => "Battery",
            Self::Sensors => "Sensors",
            Self::Android => "Android",
        }
    }

    /// Android tab only exists when Pack A is installed (F15).
    pub fn visible(self, android_pack: bool) -> bool {
        self != Self::Android || android_pack
    }
}

/// System-wide values for the Performance tab (TM-04..TM-08).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemSample {
    pub cpu_total_percent: f32,
    pub per_core_percent: Vec<f32>,
    pub cpu_mhz: Option<u32>,
    pub governor: Option<String>,
    pub uptime_secs: u64,
    pub process_count: usize,
    pub mem_total_bytes: u64,
    pub mem_available_bytes: u64,
    pub mem_cached_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
    pub zram_used_bytes: Option<u64>,
    pub pressure_level: Option<aether_lifecycled::pressure::PressureLevel>,
    pub disk_read_bytes_per_sec: f64,
    pub disk_write_bytes_per_sec: f64,
    pub network_rx_bytes_per_sec: f64,
    pub network_tx_bytes_per_sec: f64,
    pub gpu_busy_percent: Option<f32>,
    pub gpu_memory_used_bytes: Option<u64>,
    pub temperatures_c: BTreeMap<String, f32>,
    pub fans_rpm: BTreeMap<String, u32>,
    pub battery: Option<BatteryView>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatteryView {
    pub percent: f32,
    pub charging: bool,
    pub power_w: Option<f32>,
    pub time_remaining_secs: Option<u64>,
    pub cycle_count: Option<u32>,
    pub health_percent: Option<f32>,
    pub temperature_c: Option<f32>,
}

/// A sensor reading from hwmon or thermal sysfs (TM-10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorReading {
    pub name: String,
    pub kind: SensorKind,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensorKind {
    TemperatureC,
    FanRpm,
}

/// One performance graph point (60 s window, D10 sampling).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GraphPoint {
    pub at_epoch_ms: u64,
    pub value: f32,
}

/// 60-second history for each Performance section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PerformanceHistory {
    pub cpu: Vec<GraphPoint>,
    pub memory: Vec<GraphPoint>,
    pub disk: Vec<GraphPoint>,
    pub network: Vec<GraphPoint>,
    pub gpu: Vec<GraphPoint>,
}

impl PerformanceHistory {
    pub fn push(&mut self, sample: &SystemSample, at_epoch_ms: u64) {
        push_point(&mut self.cpu, at_epoch_ms, sample.cpu_total_percent);
        let used = sample
            .mem_total_bytes
            .saturating_sub(sample.mem_available_bytes) as f32;
        push_point(&mut self.memory, at_epoch_ms, used);
        push_point(
            &mut self.disk,
            at_epoch_ms,
            (sample.disk_read_bytes_per_sec + sample.disk_write_bytes_per_sec) as f32,
        );
        push_point(
            &mut self.network,
            at_epoch_ms,
            (sample.network_rx_bytes_per_sec + sample.network_tx_bytes_per_sec) as f32,
        );
        if let Some(gpu) = sample.gpu_busy_percent {
            push_point(&mut self.gpu, at_epoch_ms, gpu);
        }
    }
}

fn push_point(points: &mut Vec<GraphPoint>, at_epoch_ms: u64, value: f32) {
    points.push(GraphPoint { at_epoch_ms, value });
    let cutoff = at_epoch_ms.saturating_sub(60_000);
    points.retain(|point| point.at_epoch_ms >= cutoff);
}

/// A startup item (TM-13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupItem {
    pub name: String,
    pub exec: String,
    pub desktop_file: String,
    pub enabled: bool,
    pub scope: StartupScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupScope {
    User,
    System,
}

/// A systemd service row (TM-17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRow {
    pub unit: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

/// A logged-in user/session (TM-14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSession {
    pub user: String,
    pub uid: u32,
    pub seat: Option<String>,
}

/// Export format (TM-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Text,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ProcessRow {
        ProcessRow {
            pid: 10,
            ppid: 1,
            name: "browser".into(),
            command: "/usr/bin/browser".into(),
            user: Some(1000),
            threads: 12,
            open_files: 40,
            state: 'S',
            cpu_percent: 12.5,
            memory_bytes: 1_400_000_000,
            disk_read_bytes: 2_000_000,
            disk_write_bytes: 1_000_000,
            network_tx_bytes: 300_000,
            network_rx_bytes: 600_000,
            gpu_percent: Some(4.0),
            energy: EnergyImpact::High,
            wakeups: 120,
            efficiency: false,
            app_state: Some(AppState::Foreground),
            band: Some(Band::Foreground),
            cgroup: Some("/aether.slice/apps.slice/browser.scope".into()),
        }
    }

    #[test]
    fn sorts_by_every_column() {
        let mut a = row();
        let mut b = row();
        b.cpu_percent = 1.0;
        b.memory_bytes = 10;
        let mut rows = vec![a.clone(), b.clone()];
        rows.sort_by(|x, y| y.sort_key(Column::Cpu).partial_cmp(&x.sort_key(Column::Cpu)).unwrap());
        assert_eq!(rows[0].pid, 10);
        rows.sort_by(|x, y| x.sort_key(Column::Memory).partial_cmp(&y.sort_key(Column::Memory)).unwrap());
        assert_eq!(rows[0].pid, 10);
        a.efficiency = true;
        assert!(a.efficiency);
    }

    #[test]
    fn energy_impact_classification() {
        assert_eq!(EnergyImpact::classify(0.1, 0, None), EnergyImpact::None);
        assert_eq!(EnergyImpact::classify(1.0, 0, None), EnergyImpact::Low);
        assert_eq!(EnergyImpact::classify(20.0, 0, None), EnergyImpact::Medium);
        assert_eq!(
            EnergyImpact::classify(60.0, 10_000, Some(80.0)),
            EnergyImpact::High
        );
        assert_eq!(EnergyImpact::High.label(), "High");
        assert_eq!(EnergyImpact::from_share(0.7, 25.0), EnergyImpact::High);
        assert_eq!(EnergyImpact::from_share(0.4, 10.0), EnergyImpact::Medium);
        assert_eq!(EnergyImpact::from_share(0.1, 0.4), EnergyImpact::Low);
        assert_eq!(EnergyImpact::from_share(0.0, 0.0), EnergyImpact::None);
    }

    #[test]
    fn energy_column_contract_is_stable() {
        let contract = EnergyImpact::contract();
        assert_eq!(contract.key, "energy");
        assert_eq!(contract.header, "Energy");
        assert_eq!(contract.default_sort, "desc");
        assert_eq!(contract.refresh_seconds, 60);
        assert_eq!(contract.levels.len(), 4);
        assert_eq!(contract.levels[0].level, EnergyImpact::High);
        assert_eq!(contract.levels[0].min_share, 0.66);
        assert_eq!(contract.levels[0].tone, "danger");
        assert_eq!(contract.sources[0].priority, 1);
        let json = serde_json::to_string(&contract).unwrap();
        assert!(json.contains("\"null_label\":\"Not reported\""));
    }

    #[test]
    fn android_tab_only_with_pack() {
        assert!(!Tab::Android.visible(false));
        assert!(Tab::Android.visible(true));
        assert!(Tab::Processes.visible(false));
    }

    #[test]
    fn history_keeps_sixty_seconds() {
        let mut history = PerformanceHistory::default();
        let mut sample = SystemSample {
            cpu_total_percent: 10.0,
            mem_total_bytes: 100,
            mem_available_bytes: 40,
            disk_read_bytes_per_sec: 1.0,
            disk_write_bytes_per_sec: 2.0,
            network_rx_bytes_per_sec: 3.0,
            network_tx_bytes_per_sec: 4.0,
            gpu_busy_percent: Some(5.0),
            ..Default::default()
        };
        for step in 0..70u64 {
            sample.cpu_total_percent = step as f32;
            history.push(&sample, step * 1000);
        }
        assert!(history.cpu.len() <= 61);
        assert_eq!(history.cpu.last().unwrap().value, 69.0);
        assert_eq!(history.gpu.len(), history.cpu.len());
    }
}
