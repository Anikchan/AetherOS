//! AetherOS installer and dual-boot tooling (P8, design D4/F5).
//!
//! Detection, pre-checks, planning, boot menu and uninstall planning are pure
//! and tested. Writing to disks requires the live image and is guarded.

pub mod boot;
pub mod disk;
pub mod planner;
pub mod precheck;
pub mod uninstall;

pub use boot::{render_entries, render_loader_conf, render_menu, BootEntry, EntryKind};
pub use disk::{
    detect_system, find_esp_mounts, human_size, inspect_windows_mount, parse_lsblk_json, Disk,
    HardwareReport, OperatingSystem, Partition, PartitionTable, WindowsInfo,
};
pub use planner::{
    plan as plan_install, aetheros_roles, InstallPlan, Operation, OperationKind,
    PartitionAllocation, PlanError, ESP_BYTES, GIB, MIB,
};
pub use precheck::{
    blocking, can_proceed, run as run_prechecks, CheckStatus, InstallMode, PreCheck,
    PreCheckInput, MIN_FREE_BYTES, RECOMMENDED_FREE_BYTES,
};
pub use uninstall::{
    plan as plan_uninstall, UninstallError, UninstallMode, UninstallOperation, UninstallPlan,
};
