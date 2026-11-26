//! Pack D (advanced monitoring) data model (P7-PK-T01, F13 TM-09/TM-12/TM-15).
//!
//! Pack D adds two things the core sampler deliberately leaves out: per-app
//! network from kernel socket accounting, and per-process GPU detail from the
//! DRM usage fields in `/proc/<pid>/fdinfo/<fd>` (D10 §2, §4, §5.2).
//!
//! The **recorders live in the daemons**, so the hourly buckets fill without the
//! Activity Manager being open:
//!
//! - [`aether_powerd::pack_d`] reads the socket-accounting snapshot on every
//!   `aether-powerd` tick and keeps [`AppNetworkHistory`];
//! - [`aether_lifecycled::pack_d`] samples DRM fdinfo on the `aether-lifecycled`
//!   loop and keeps [`GpuHistory`].
//!
//! This module re-exports those contracts, owns the CPU-time integrator that
//! runs on the 1 s process samples, applies Pack D values to [`ProcessRow`]s and
//! publishes [`PackDContract`], which pins the paths, refresh rates, units and
//! the parts that still need AetherOS hardware.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Network accounting types and recorder (powerd owns the loop).
pub use aether_powerd::pack_d::{
    AppNetworkCounters, AppNetworkHistory, AppNetworkRate, NetworkAccounting, NetworkHistoryStore,
    NetworkRecorder, NetworkSample, NetworkSourceStatus, NETWORK_SNAPSHOT_HEADER,
    NETWORK_SNAPSHOT_PATH,
};
/// GPU fdinfo types and recorder (lifecycled owns the loop).
pub use aether_lifecycled::pack_d::{
    GpuBucketRecord, GpuDetailSampler, GpuEngineUsage, GpuHistory, GpuHistoryStore,
    GpuProcessDetail, GpuRecorder, GpuSample, GpuSourceStatus, GPU_SAMPLE_INTERVAL_SECONDS,
};

use crate::manager::app_id_from_scope;
use crate::model::ProcessRow;

/// History retention for the App history columns (8 days covers the 7-day
/// window with one hour of slack for the roll-up granularity).
pub const HISTORY_RETENTION_HOURS: u64 = aether_powerd::pack_d::HISTORY_RETENTION_HOURS;

/// Refresh cadence from D10 §3 (Pack D row).
pub const PACK_D_REFRESH_VISIBLE_SECONDS: u32 = aether_powerd::pack_d::PACK_D_REFRESH_VISIBLE_SECONDS;
pub const PACK_D_REFRESH_HIDDEN_SECONDS: u32 = aether_powerd::pack_d::PACK_D_REFRESH_HIDDEN_SECONDS;

fn hour_of(epoch_ms: u64) -> u64 {
    epoch_ms / 3_600_000
}

/// State of a Pack D source, for the honest "not installed" wording.
///
/// This is the view-side union of the two daemon states: the network recorder
/// can tell "not installed" from "not loaded", the GPU recorder can only say
/// whether procfs was readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackDStatus {
    /// Pack D is installed and the source produced data.
    Reported,
    /// Pack D is not installed: samplers are not loaded or running (D10 §7).
    NotInstalled,
    /// Pack D is installed but its counter source could not be read.
    NotLoaded,
}

impl PackDStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Reported => "Reported",
            Self::NotInstalled => "Not installed",
            Self::NotLoaded => "Not loaded",
        }
    }
}

impl From<NetworkSourceStatus> for PackDStatus {
    fn from(status: NetworkSourceStatus) -> Self {
        match status {
            NetworkSourceStatus::Reported => Self::Reported,
            NetworkSourceStatus::NotInstalled => Self::NotInstalled,
            NetworkSourceStatus::NotLoaded => Self::NotLoaded,
        }
    }
}

impl From<GpuSourceStatus> for PackDStatus {
    fn from(status: GpuSourceStatus) -> Self {
        match status {
            GpuSourceStatus::Reported => Self::Reported,
            GpuSourceStatus::NotLoaded => Self::NotLoaded,
        }
    }
}

/// Per-app CPU seconds integrated from the 1 s process samples (TM-12).
///
/// `ProcessRow::cpu_percent` is a share of one core, so CPU seconds for one
/// interval are `cpu_percent / 100 × seconds`. Only rows inside a managed app
/// scope are attributed; system processes are not Pack D data.
#[derive(Debug, Default, Clone)]
pub struct AppCpuTime {
    last_at_ms: Option<u64>,
    buckets: BTreeMap<(String, u64), f64>,
}

impl AppCpuTime {
    /// Integrate one process sample. The first call only starts the clock.
    pub fn observe(&mut self, rows: &[ProcessRow], at_epoch_ms: u64) {
        let Some(last) = self.last_at_ms else {
            self.last_at_ms = Some(at_epoch_ms);
            return;
        };
        let elapsed = at_epoch_ms.saturating_sub(last) as f64 / 1000.0;
        if elapsed <= 0.0 {
            return;
        }
        let hour = hour_of(at_epoch_ms);
        for row in rows {
            let Some(app) = row.cgroup.as_deref().and_then(app_id_from_scope) else {
                continue;
            };
            if row.cpu_percent <= 0.0 {
                continue;
            }
            let seconds = f64::from(row.cpu_percent) / 100.0 * elapsed;
            *self.buckets.entry((app, hour)).or_default() += seconds;
        }
        self.last_at_ms = Some(at_epoch_ms);
        self.prune(at_epoch_ms);
    }

    /// CPU seconds an app used since `since_epoch_ms` (1-hour granularity).
    pub fn total_seconds_for(&self, app: &str, since_epoch_ms: u64) -> f64 {
        let since_hour = hour_of(since_epoch_ms);
        self.buckets
            .iter()
            .filter(|((key, hour), _)| key == app && *hour >= since_hour)
            .map(|(_, seconds)| *seconds)
            .sum()
    }

    pub fn apps(&self) -> std::collections::BTreeSet<String> {
        self.buckets.keys().map(|(app, _)| app.clone()).collect()
    }

    fn prune(&mut self, at_epoch_ms: u64) {
        let cutoff = hour_of(at_epoch_ms).saturating_sub(HISTORY_RETENTION_HOURS);
        self.buckets.retain(|(_, hour), _| *hour >= cutoff);
    }
}

/// Apply Pack D GPU detail to the process rows (TM-09 per-process value).
pub fn apply_gpu_detail(rows: &mut [ProcessRow], details: &[GpuProcessDetail]) {
    let by_pid: BTreeMap<u32, &GpuProcessDetail> =
        details.iter().map(|detail| (detail.pid, detail)).collect();
    for row in rows.iter_mut() {
        if let Some(detail) = by_pid.get(&row.pid) {
            if !detail.engines.is_empty() {
                row.gpu_percent = Some(detail.busy_percent());
            }
        }
    }
}

/// Apply session network totals to the process rows (TM-15, Processes tab).
pub fn apply_network_totals(rows: &mut [ProcessRow], history: &AppNetworkHistory) {
    for row in rows.iter_mut() {
        let Some(app) = row.cgroup.as_deref().and_then(app_id_from_scope) else {
            continue;
        };
        let (rx, tx) = history.session_total(&app);
        if rx > 0 || tx > 0 {
            row.network_rx_bytes = rx;
            row.network_tx_bytes = tx;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackDNetworkContract {
    /// Kernel mechanism, as recorded in D10 §2.
    pub accounting: String,
    pub snapshot_path: String,
    pub snapshot_header: String,
    pub rx_header: String,
    pub tx_header: String,
    pub unit: String,
    pub null_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackDGpuContract {
    pub source: String,
    pub fdinfo_path: String,
    pub engines: Vec<String>,
    pub memory_regions: Vec<String>,
    pub unit: String,
    pub null_label: String,
}

/// Contract for the Pack D columns (TM-09/TM-12/TM-15, D10 §5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackDContract {
    pub key: String,
    pub pack: String,
    pub refresh_visible_seconds: u32,
    pub refresh_hidden_seconds: u32,
    /// D10 §3 overhead target for the pack.
    pub overhead_percent_of_one_core: f64,
    pub history_retention_hours: u64,
    pub history_columns: Vec<String>,
    pub network: PackDNetworkContract,
    pub gpu: PackDGpuContract,
    pub null_label: String,
    /// Which daemon records each source, so the wiring is published, not implied.
    pub recorders: Vec<String>,
    /// Parts that cannot be validated on this host and need AetherOS hardware.
    pub hardware_gated: Vec<String>,
}

impl Default for PackDContract {
    fn default() -> Self {
        Self {
            key: "pack-d".into(),
            pack: "D. Advanced monitoring".into(),
            refresh_visible_seconds: PACK_D_REFRESH_VISIBLE_SECONDS,
            refresh_hidden_seconds: PACK_D_REFRESH_HIDDEN_SECONDS,
            overhead_percent_of_one_core: 0.5,
            history_retention_hours: HISTORY_RETENTION_HOURS,
            history_columns: vec!["CPU time".into(), "Network".into()],
            network: PackDNetworkContract {
                accounting: "kernel socket accounting through an eBPF program attached to app cgroups"
                    .into(),
                snapshot_path: NETWORK_SNAPSHOT_PATH.into(),
                snapshot_header: NETWORK_SNAPSHOT_HEADER.into(),
                rx_header: "Received".into(),
                tx_header: "Sent".into(),
                unit: "bytes".into(),
                null_label: "Not reported".into(),
            },
            gpu: PackDGpuContract {
                source: "DRM usage fields in /proc/<pid>/fdinfo/<fd>".into(),
                fdinfo_path: "/proc/<pid>/fdinfo/<fd>".into(),
                engines: vec![
                    "render".into(),
                    "compute".into(),
                    "video".into(),
                    "video-enhance".into(),
                    "copy".into(),
                    "gfx".into(),
                ],
                memory_regions: vec![
                    "vram".into(),
                    "gtt".into(),
                    "system".into(),
                    "stolen".into(),
                ],
                unit: "percent".into(),
                null_label: "Not reported".into(),
            },
            null_label: "Not reported".into(),
            recorders: vec![
                format!(
                    "network: aether-powerd samples {NETWORK_SNAPSHOT_PATH} every {PACK_D_REFRESH_HIDDEN_SECONDS} s and writes hourly buckets"
                ),
                format!(
                    "gpu: aether-lifecycled samples DRM fdinfo every {GPU_SAMPLE_INTERVAL_SECONDS} s and writes hourly buckets"
                ),
            ],
            hardware_gated: vec![
                "eBPF cgroup/sock accounting program and its map dump to the snapshot path".into(),
                "DRM fdinfo validation on the reference Intel and AMD GPUs".into(),
                "G-TM1 overhead measurement with Pack D installed".into(),
            ],
        }
    }
}

impl PackDContract {
    /// The published contract, used by the window and the CLI.
    pub fn published() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-pack-d-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn app_row(pid: u32, app: &str, cpu_percent: f32) -> ProcessRow {
        ProcessRow {
            pid,
            ppid: 1,
            name: app.rsplit('.').next().unwrap_or(app).to_lowercase(),
            command: format!("/usr/bin/{app}"),
            user: Some(1000),
            threads: 2,
            open_files: 4,
            state: 'S',
            cpu_percent,
            memory_bytes: 1024,
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            network_tx_bytes: 0,
            network_rx_bytes: 0,
            gpu_percent: None,
            energy: crate::model::EnergyImpact::Low,
            wakeups: 0,
            efficiency: false,
            app_state: None,
            band: None,
            cgroup: Some(format!(
                "/aether.slice/aether-apps.slice/aether-app-{app}-1a2b3c4d.scope"
            )),
        }
    }

    #[test]
    fn cpu_time_is_integrated_from_cpu_share_and_elapsed_time() {
        let mut cpu = AppCpuTime::default();
        let rows = vec![
            app_row(10, "org.a", 50.0),
            app_row(11, "org.a", 25.0),
            app_row(12, "org.b", 100.0),
        ];
        // First sample only starts the clock.
        cpu.observe(&rows, 0);
        assert_eq!(cpu.total_seconds_for("org.a", 0), 0.0);

        // One second at 50% + 25% = 0.75 CPU-seconds for org.a.
        cpu.observe(&rows, 1_000);
        assert!((cpu.total_seconds_for("org.a", 0) - 0.75).abs() < 1e-6);
        assert!((cpu.total_seconds_for("org.b", 0) - 1.0).abs() < 1e-6);

        // Two more seconds of the same load.
        cpu.observe(&rows, 3_000);
        assert!((cpu.total_seconds_for("org.a", 0) - 2.25).abs() < 1e-6);
        // The window starts in the next hour.
        assert_eq!(cpu.total_seconds_for("org.a", 7_200_000), 0.0);
        assert_eq!(cpu.apps().len(), 2);
    }

    #[test]
    fn cpu_time_ignores_processes_outside_app_scopes() {
        let mut cpu = AppCpuTime::default();
        let mut system = app_row(20, "org.a", 90.0);
        system.cgroup = Some("/system.slice/sshd.service".into());
        cpu.observe(&[system.clone()], 0);
        cpu.observe(&[system], 1_000);
        assert!(cpu.apps().is_empty());
    }

    #[test]
    fn gpu_detail_and_network_totals_reach_the_process_rows() {
        let mut rows = vec![app_row(10, "org.a", 1.0), app_row(11, "org.b", 1.0)];
        let mut history = AppNetworkHistory::default();
        history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 100,
                tx_bytes: 50,
            }],
            0,
        );
        history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 600,
                tx_bytes: 150,
            }],
            1_000,
        );
        apply_network_totals(&mut rows, &history);
        assert_eq!(rows[0].network_rx_bytes, 500);
        assert_eq!(rows[0].network_tx_bytes, 100);
        // No counters for org.b: zeros stay zeros because nothing was reported.
        assert_eq!(rows[1].network_rx_bytes, 0);

        let details = vec![GpuProcessDetail {
            pid: 11,
            pdev: Some("0000:00:02.0".into()),
            engines: vec![
                GpuEngineUsage {
                    engine: "render".into(),
                    busy_percent: 12.0,
                },
                GpuEngineUsage {
                    engine: "video".into(),
                    busy_percent: 30.5,
                },
            ],
            memory_bytes: BTreeMap::from([("vram".into(), 1_048_576)]),
        }];
        apply_gpu_detail(&mut rows, &details);
        // Highest engine wins, and rows without detail stay unreported.
        assert_eq!(rows[1].gpu_percent, Some(30.5));
        assert_eq!(rows[0].gpu_percent, None);
    }

    #[test]
    fn daemon_statuses_map_to_the_view_status() {
        assert_eq!(
            PackDStatus::from(NetworkSourceStatus::Reported),
            PackDStatus::Reported
        );
        assert_eq!(
            PackDStatus::from(NetworkSourceStatus::NotInstalled),
            PackDStatus::NotInstalled
        );
        assert_eq!(
            PackDStatus::from(NetworkSourceStatus::NotLoaded),
            PackDStatus::NotLoaded
        );
        assert_eq!(
            PackDStatus::from(GpuSourceStatus::NotLoaded),
            PackDStatus::NotLoaded
        );
        assert_eq!(PackDStatus::NotInstalled.label(), "Not installed");
    }

    #[test]
    fn app_history_reads_the_buckets_the_daemon_persisted() {
        // Evidence for the wiring: the App history view consumes the file that
        // aether-powerd writes, instead of only its own in-process samples.
        let dir = temp_dir("history-file");
        let path = dir.join("pack-d-network.json");
        let mut recorder = NetworkRecorder::at(dir.join("net-accounting"));
        recorder.history_mut().restore_buckets(
            &[aether_powerd::pack_d::NetworkBucketRecord {
                app: "org.aether.Browser".into(),
                hour: 0,
                rx_bytes: 4_096,
                tx_bytes: 1_024,
            }],
        );
        recorder.save(&path).unwrap();

        let history = aether_powerd::pack_d::load_history(&path).unwrap();
        let view = crate::tabs::app_history_view(None, None, Some(&history), 60);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].cells[0], "org.aether.Browser");
        assert_eq!(view.rows[0].cells[4], "4.0 KB in · 1.0 KB out");
        assert_eq!(view.rows[0].cells[3], "Not reported");
        assert!(view.note.contains("Pack D"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn contract_pins_the_pack_d_binding() {
        let contract = PackDContract::published();
        assert_eq!(contract.key, "pack-d");
        assert_eq!(contract.refresh_visible_seconds, 2);
        assert_eq!(contract.refresh_hidden_seconds, 30);
        assert!((contract.overhead_percent_of_one_core - 0.5).abs() < 1e-9);
        assert_eq!(contract.history_retention_hours, 8 * 24);
        assert_eq!(contract.network.snapshot_path, NETWORK_SNAPSHOT_PATH);
        assert_eq!(contract.network.snapshot_header, NETWORK_SNAPSHOT_HEADER);
        assert_eq!(contract.network.null_label, "Not reported");
        assert!(contract.gpu.engines.contains(&"render".to_string()));
        assert!(contract.hardware_gated.iter().any(|item| item.contains("eBPF")));
        // The daemons that record Pack D are part of the contract.
        assert_eq!(contract.recorders.len(), 2);
        assert!(contract.recorders[0].contains("aether-powerd"));
        assert!(contract.recorders[1].contains("aether-lifecycled"));
        let json = serde_json::to_string(&contract).unwrap();
        assert!(json.contains("\"key\":\"pack-d\""));
        assert!(json.contains("\"refresh_visible_seconds\":2"));
    }
}
