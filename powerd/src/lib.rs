//! AetherOS power daemon (`aether-powerd`, P5-T09..T17, design D5 and F14).
//!
//! - [`policy`]: profile switching, thermal cap, low-battery state machine
//! - [`estimator`]: time-remaining moving average with spike rejection
//! - [`tuner`]: runtime PM tuner with exclusions and automatic revert
//! - [`charge`]: charge limit where supported
//! - [`sleep`]: suspend / deep / suspend-then-hibernate policy
//! - [`telemetry`]: UPower with a sysfs fallback
//! - [`analytics`]: battery history, sessions, health, energy by app (F14)
//! - [`screentime`]: screen-on time, time per app, pickups, notifications
//! - [`pack_d`]: Pack D network accounting recorder and hourly buckets
//! - [`daemon`]: wiring and the status surface for the control center

pub mod analytics;
pub mod backend;
pub mod charge;
pub mod daemon;
pub mod estimator;
pub mod limits;
pub mod pack_d;
pub mod policy;
pub mod screentime;
pub mod sleep;
pub mod telemetry;
pub mod tuner;
pub mod types;

pub use analytics::{
    BatteryAnalytics, BatterySample, DailyBattery, Session, SessionKind, DAY_SECS,
};
pub use backend::{
    read_background_policy, FileBackgroundSink, PlatformProfile, PowerProfilesCli,
    SystemProfileBackend, SystemdSleep,
};
pub use charge::SysfsChargeLimiter;
pub use daemon::{
    epoch_secs, system_daemon, write_status_file, PowerDaemon, PowerLogEntry, PowerMechanisms,
    PowerStatus, PowerdConfig,
};
pub use estimator::{EstimatorConfig, RateEstimator};
pub use limits::{
    decide as decide_screen_time_limit, remaining_seconds, BlockDecision, DowntimeWindow,
    LimitRule, LimitsConfig, LimitsState,
};
pub use pack_d::{
    AccountingIssue, AppNetworkCounters, AppNetworkHistory, AppNetworkRate, NetworkAccounting,
    NetworkHistoryStore, NetworkRecorder, NetworkSample, NetworkSourceStatus,
    NETWORK_SNAPSHOT_HEADER, NETWORK_SNAPSHOT_PATH,
};
pub use policy::{
    clamp_charge_limit, LowBatteryMachine, LowBatteryState, PowerPolicyConfig, PowerPolicyImpl,
    Threshold,
};
pub use screentime::{DayScreenTime, FocusLogAccuracy, ScreenTimeStore};
pub use sleep::{parse_mem_sleep, SleepAction, SleepConfig, SleepError, SleepManager, SleepMode};
pub use telemetry::{parse_upower, BatteryReader, SysfsBattery, UpowerCli};
pub use tuner::{
    default_rules, DeviceErrorOutcome, SysfsBackend, Tuner, TunerBackend, TunerReport, TunerRule,
};
pub use types::{
    BackgroundPolicy, BatteryStatus, BatteryTelemetry, ChargeLimiter, DeviceTuner, Estimator,
    LowBatteryAction, PowerAction, PowerError, PowerEvent, PowerPolicy, PowerProfile, PowerSource,
    PowerThresholds, ProfileBackend, SleepController, ThermalLevel,
};
