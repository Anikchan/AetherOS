//! AetherOS lifecycle daemon (`aether-lifecycled`, P5, design D3).
//!
//! Modules:
//! - [`identity`]: app identity and one cgroup v2 scope per app (P5-T01)
//! - [`state`]: lifecycle state machine, every D3 §3 transition encoded once
//!   (P5-T02)
//! - [`protocol`]: app-facing will-suspend / did-resume protocol (P5-T03)
//! - [`cgroup`]: freeze, `memory.reclaim`, zram tuning, kill (P5-T04)
//! - [`pressure`]: PSI reader with a reclaim-only fallback (P5-T05)
//! - [`budget`]: background CPU-time budget (P5-T06)
//! - [`pack_d`]: Pack D GPU fdinfo sampler and per-app hourly buckets
//! - [`policy`]: pure decision layer (no I/O)
//! - [`daemon`]: wiring, timers, persistence, metrics
//!
//! Design contracts are in `docs/2026/11/16/os-project/design/process-manager-design.md`.

pub mod budget;
pub mod cgroup;
pub mod daemon;
pub mod identity;
pub mod pack_d;
pub mod policy;
pub mod pressure;
pub mod protocol;
pub mod state;

pub use budget::{BackgroundBudget, BackgroundBudgetConfig};
pub use cgroup::{CgroupFs, Freezer, Killer, MemoryReader, Reclaimer, ZramStatus, ZramTuner};
pub use daemon::{
    epoch_millis, system_daemon, AppRow, Daemon, DaemonError, KillRecord, LogEntry, Mechanisms,
    MetricsSnapshot, RuntimeConfig,
};
pub use identity::{
    app_id_from_scope, scope_unit_name, AppId, AppScope, CgroupScopeManager, IdentityError,
    MemoryScopeManager, ScopeManager, APP_SLICE, ROOT_SLICE, SCOPE_PREFIX,
};
pub use pack_d::{
    load_gpu_history, parse_fdinfo, FdinfoDrm, GpuBucketRecord, GpuDetailSampler, GpuEngineUsage,
    GpuHistory, GpuHistoryStore, GpuProcessDetail, GpuRecorder, GpuSample, GpuSourceStatus,
    GPU_SAMPLE_INTERVAL_SECONDS, HISTORY_RETENTION_HOURS, PROC_ROOT,
};
pub use policy::{
    select_kill_candidate, Action, Event, KillReason, LifecyclePolicy, Policy, PolicyConfig,
    PowerSourceKind, StateReason, TimerKind,
};
pub use pressure::{PressureLevel, PressureThresholds, PsiReader, PsiSample};
pub use protocol::{
    socket_name, Ack, LifecycleMessage, LifecycleNotifier, LifecycleServer, NotifyError,
    SocketNotifier, SuspendHandler,
};
pub use state::{
    can_transition, validate_transition, AppProcess, AppState, Band, ProtectReason,
    TransitionError,
};
