//! Contracts for `aether-powerd` (P5-T09..T17, design D5 section 4).
//!
//! The policy has no I/O; every mechanism is behind a trait so the rules can
//! be tested on any host.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerSource {
    Ac,
    Battery,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerProfile {
    Performance,
    Balanced,
    Saver,
}

impl PowerProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Balanced => "balanced",
            Self::Saver => "power-saver",
        }
    }

    /// Display name used by the control center (D5 open question: Windows-like
    /// plan names).
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Balanced => "Balanced",
            Self::Saver => "Saver",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "performance" => Some(Self::Performance),
            "balanced" => Some(Self::Balanced),
            "power-saver" | "saver" | "power-saver-mode" => Some(Self::Saver),
            _ => None,
        }
    }
}

/// Battery snapshot. Fields beyond the D5 minimum are optional and additive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryStatus {
    pub percent: f32,
    pub source: PowerSource,
    pub charging: bool,
    pub energy_rate_w: Option<f32>,
    pub time_to_empty: Option<Duration>,
    pub time_to_full: Option<Duration>,
    pub health_percent: Option<f32>,
    pub cycle_count: Option<u32>,
    /// Remaining energy in Wh (used by the estimator).
    #[serde(default)]
    pub energy_now_wh: Option<f32>,
    /// Last full capacity in Wh.
    #[serde(default)]
    pub energy_full_wh: Option<f32>,
    /// Design capacity in Wh.
    #[serde(default)]
    pub design_wh: Option<f32>,
    #[serde(default)]
    pub temperature_c: Option<f32>,
}

impl Default for BatteryStatus {
    fn default() -> Self {
        Self {
            percent: 0.0,
            source: PowerSource::Unknown,
            charging: false,
            energy_rate_w: None,
            time_to_empty: None,
            time_to_full: None,
            health_percent: None,
            cycle_count: None,
            energy_now_wh: None,
            energy_full_wh: None,
            design_wh: None,
            temperature_c: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThermalLevel {
    Normal,
    Warm,
    Hot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LowBatteryAction {
    Suspend,
    Hibernate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundPolicy {
    pub grace_period_secs: u64,
    pub hibernate_after_secs: u64,
    pub cpu_weight: u32,
}

impl BackgroundPolicy {
    pub fn ac() -> Self {
        Self {
            grace_period_secs: 30,
            hibernate_after_secs: 600,
            cpu_weight: 100,
        }
    }

    pub fn battery() -> Self {
        Self {
            grace_period_secs: 10,
            hibernate_after_secs: 300,
            cpu_weight: 50,
        }
    }
}

/// Events from UPower, logind and the user (D5 section 4).
#[derive(Debug, Clone, PartialEq)]
pub enum PowerEvent {
    SourceChanged(PowerSource),
    BatteryUpdated(BatteryStatus),
    LidChanged { closed: bool },
    UserProfileRequest(PowerProfile),
    ThermalChanged(ThermalLevel),
    UserIdle(bool),
    DeviceError { device: String },
    /// User changed the charge limit in settings.
    ChargeLimitRequest(u8),
}

/// Actions the daemon turns into mechanism calls (D5 section 4).
#[derive(Debug, Clone, PartialEq)]
pub enum PowerAction {
    SetProfile(PowerProfile),
    SetDevicePowerSaving { device: String, enabled: bool },
    SetChargeLimit(u8),
    SetBackgroundPolicy(BackgroundPolicy),
    SetLowBatteryAction(LowBatteryAction),
    Notify(String),
    Suspend,
    Hibernate,
    Log(String),
}

#[derive(Debug, Error)]
pub enum PowerError {
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("backend error: {0}")]
    Backend(String),
}

pub trait PowerPolicy {
    fn on_event(&mut self, event: PowerEvent) -> Vec<PowerAction>;
}

pub trait BatteryTelemetry {
    fn read(&self) -> Result<BatteryStatus, PowerError>;
}

pub trait ProfileBackend {
    fn apply(&self, profile: PowerProfile) -> Result<(), PowerError>;
    fn current(&self) -> Result<Option<PowerProfile>, PowerError>;
}

pub trait DeviceTuner {
    fn apply(&self, device: &str, enabled: bool) -> std::io::Result<()>;
    fn revert_all(&self) -> std::io::Result<()>;
}

pub trait ChargeLimiter {
    fn supported(&self) -> bool;
    fn set_limit(&self, percent: u8) -> std::io::Result<()>;
    fn limit(&self) -> std::io::Result<Option<u8>>;
}

pub trait Estimator {
    fn update(&mut self, sample: &BatteryStatus, at: Instant);
    fn time_to_empty(&self) -> Option<Duration>;
}

/// Daemon-side actions that touch the system (kept out of the policy).
pub trait SleepController {
    fn suspend(&self) -> Result<(), PowerError>;
    fn hibernate(&self) -> Result<(), PowerError>;
}

/// Forwards background limits to `aether-lifecycled` (D5 section 3).
pub trait BackgroundPolicySink {
    fn set(&self, policy: BackgroundPolicy) -> Result<(), PowerError>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerThresholds {
    pub warn: f32,
    pub saver: f32,
    pub action: f32,
    pub critical: f32,
}

impl Default for PowerThresholds {
    fn default() -> Self {
        Self {
            warn: 20.0,
            saver: 10.0,
            action: 5.0,
            critical: 3.0,
        }
    }
}
