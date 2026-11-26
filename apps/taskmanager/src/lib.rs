//! AetherOS Activity Manager core (P5-T07, F13).
//!
//! The Slint window (S3 design system) binds to these rows and actions; all
//! metric collection, process actions, history merge and export are here and
//! tested headlessly.

pub mod collect;
pub mod manager;
pub mod model;
pub mod s7;
pub mod window;

pub use collect::{pressure_label, ProcFs, ProcessSampler, SystemSampler};
pub use manager::{
    app_id_from_scope, parse_loginctl, parse_systemctl, ActivityManager, EndTaskOutcome, Snapshot,
};
pub use window::{demo_rows, ActivityManagerWindow, ProcessRowView};
pub use model::{
    BatteryView, Column, EnergyColumnContract, EnergyImpact, EnergyLevelSpec, EnergySourceSpec,
    ExportFormat, GraphPoint, PerformanceHistory, ProcessRow, SensorKind, SensorReading, ServiceRow,
    StartupItem, StartupScope, SystemSample, Tab, UserSession,
};
pub use s7::{run as run_spike_s7, to_markdown as spike_s7_markdown, SpikeS7Report, SpikeVerdict};
