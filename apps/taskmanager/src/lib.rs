//! AetherOS Activity Manager core (P5-T07, F13).
//!
//! The Slint window (S3 design system) binds to these rows and actions; all
//! metric collection, process actions, history merge and export are here and
//! tested headlessly.

pub mod collect;
pub mod launch;
pub mod manager;
pub mod model;
pub mod pack_d;
pub mod s7;
pub mod service_actions;
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
    performance_view, sandbox_level, sensors_report, sensors_view, services_view,
    services_view_with, startup_view, table_report, users_view, BatteryAppRow, BatteryTabView,
    GraphCard, MetricLine, PerformanceView, RowAction, SensorRowView, SensorsTabView, TableRow,
    TableView,
};
pub use window::{
    demo_history, demo_pack_d, demo_rows, demo_sensors, demo_services, demo_startup, demo_system,
    demo_users, ActivityManagerWindow, ProcessRefresh, ProcessRowView,
};
pub use model::{
    BatteryView, Column, EnergyColumnContract, EnergyImpact, EnergyLevelSpec, EnergySourceSpec,
    ExportFormat, GraphPoint, PerformanceHistory, ProcessRow, SensorKind, SensorReading, ServiceRow,
    StartupItem, StartupScope, SystemSample, Tab, UserSession,
};
pub use pack_d::{apply_gpu_detail, apply_network_totals, AppCpuTime, PackDContract, PackDStatus};
pub use aether_powerd::pack_d::{
    parse_network_snapshot, AccountingIssue, AppNetworkCounters, AppNetworkHistory,
    AppNetworkRate, NetworkAccounting, NETWORK_SNAPSHOT_HEADER, NETWORK_SNAPSHOT_PATH,
};
pub use aether_lifecycled::pack_d::{
    parse_fdinfo, FdinfoDrm, GpuDetailSampler, GpuEngineUsage, GpuProcessDetail,
};
pub use service_actions::{
    action_id, parse_action_id, polkit_policy_xml, systemd_argv, ProtectedUnit, ServiceAction,
    ServiceActionContract, ServiceActionOutcome, ServiceActionPlan, ServiceActionPlanner,
    ServiceActionPrompt, ServiceActionRequest, ServiceActionSpec, ServiceExecutor, ServicePrompt,
    ServicePromptState, SystemdDbusExecutor, POLKIT_ACTION_ID, POLKIT_POLICY_FILE,
    POLKIT_POLICY_PATH,
};
pub use s7::{run as run_spike_s7, to_markdown as spike_s7_markdown, SpikeS7Report, SpikeVerdict};
