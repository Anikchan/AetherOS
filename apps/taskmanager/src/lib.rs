//! AetherOS Activity Manager core (P5-T07, F13).
//!
//! The Slint window (S3 design system) binds to these rows and actions; all
//! metric collection, process actions, history merge and export are here and
//! tested headlessly.

pub mod collect;
pub mod manager;
pub mod model;
pub mod s7;
pub mod shortcut;
pub mod tabs;
pub mod window;

pub use collect::{pressure_label, ProcFs, ProcessSampler, SystemSampler};
pub use manager::{
    app_id_from_scope, parse_loginctl, parse_systemctl, ActivityManager, ConfirmationDialog,
    EndTaskOutcome, EndTaskRequest, Snapshot,
};
pub use shortcut::{KeyChord, KeyName, ShortcutBinding};
pub use tabs::{
    app_history_view, battery_report, battery_view, details_view, performance_report,
    performance_view, sandbox_level, sensors_report, sensors_view, services_view, startup_view,
    table_report, users_view, BatteryAppRow, BatteryTabView, GraphCard, MetricLine,
    PerformanceView, SensorRowView, SensorsTabView, TableRow, TableView,
};
pub use window::{
    demo_history, demo_rows, demo_sensors, demo_services, demo_startup, demo_system,
    demo_users, ActivityManagerWindow, ProcessRowView,
};
pub use model::{
    BatteryView, Column, EnergyColumnContract, EnergyImpact, EnergyLevelSpec, EnergySourceSpec,
    ExportFormat, GraphPoint, PerformanceHistory, ProcessRow, SensorKind, SensorReading, ServiceRow,
    StartupItem, StartupScope, SystemSample, Tab, UserSession,
};
pub use s7::{run as run_spike_s7, to_markdown as spike_s7_markdown, SpikeS7Report, SpikeVerdict};
