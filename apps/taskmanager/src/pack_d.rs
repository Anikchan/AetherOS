//! Pack D (advanced monitoring) data contracts (P7-PK-T01, F13 TM-09/TM-12/TM-15).
//!
//! Pack D adds two things the core sampler deliberately leaves out:
//! per-app network from kernel socket accounting, and per-process GPU detail
//! from the DRM usage fields in `/proc/<pid>/fdinfo/<fd>` (D10 §2, §4, §5.2).
//!
//! This module is the sampling and aggregation *model*: it parses the
//! accounting snapshot the Pack D loader produces, integrates CPU time and
//! network bytes into hourly buckets for the App history window, computes
//! per-engine GPU busy time from fdinfo counter deltas, and publishes the
//! contract the window reads. Attaching the eBPF program and validating the
//! numbers on a real Intel/AMD GPU are hardware-gated; nothing here needs a
//! kernel beyond `/proc` fixtures, and missing sources are reported, never
//! shown as zero (D10 §1).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::collect::ProcFs;
use crate::manager::app_id_from_scope;
use crate::model::ProcessRow;

/// Where the Pack D loader dumps its socket-accounting map.
pub const NETWORK_SNAPSHOT_PATH: &str = "/run/aether/pack-d/net-accounting";

/// First line of the accounting snapshot; the parser refuses anything else.
pub const NETWORK_SNAPSHOT_HEADER: &str = "aether-net-accounting v1";

/// History retention for the App history columns (8 days covers the 7-day
/// window with one hour of slack for the roll-up granularity).
pub const HISTORY_RETENTION_HOURS: u64 = 8 * 24;

/// Refresh cadence from D10 §3 (Pack D row).
pub const PACK_D_REFRESH_VISIBLE_SECONDS: u32 = 2;
pub const PACK_D_REFRESH_HIDDEN_SECONDS: u32 = 30;

fn hour_of(epoch_ms: u64) -> u64 {
    epoch_ms / 3_600_000
}

/// State of a Pack D source, for the honest "not installed" wording.
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

/// One app's cumulative socket counters from the kernel accounting program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppNetworkCounters {
    pub app: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Why the accounting snapshot could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountingIssue {
    /// No snapshot at the contract path: the pack is not installed.
    NotInstalled,
    /// The snapshot exists but could not be read (loader not running).
    NotLoaded(String),
    /// The snapshot does not match the published format.
    Malformed(String),
}

impl AccountingIssue {
    pub fn status(&self) -> PackDStatus {
        match self {
            Self::NotInstalled => PackDStatus::NotInstalled,
            Self::NotLoaded(_) | Self::Malformed(_) => PackDStatus::NotLoaded,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::NotInstalled => {
                format!("Pack D is not installed ({NETWORK_SNAPSHOT_PATH} is absent).")
            }
            Self::NotLoaded(reason) => format!("Pack D network accounting is not loaded: {reason}"),
            Self::Malformed(reason) => {
                format!("Pack D network accounting snapshot is malformed: {reason}")
            }
        }
    }
}

/// Parse the accounting snapshot the Pack D loader writes.
///
/// Format (one app per line, decimal bytes, comments with `#`):
///
/// ```text
/// aether-net-accounting v1
/// # app_id rx_bytes tx_bytes
/// org.aether.Browser 182736 99182
/// ```
///
/// The header and every number are required: a counter that was not reported is
/// an error, not a zero. Duplicate apps are refused so a bad dump cannot
/// silently double-count.
pub fn parse_network_snapshot(text: &str) -> Result<Vec<AppNetworkCounters>, AccountingIssue> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        });
    let Some((_, header)) = lines.next() else {
        return Err(AccountingIssue::Malformed("the snapshot is empty".into()));
    };
    if header.trim() != NETWORK_SNAPSHOT_HEADER {
        return Err(AccountingIssue::Malformed(format!(
            "expected the header {NETWORK_SNAPSHOT_HEADER:?}, found {:?}",
            header.trim()
        )));
    }
    let mut seen = BTreeSet::new();
    let mut counters = Vec::new();
    for (index, line) in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(AccountingIssue::Malformed(format!(
                "line {}: expected \"<app> <rx_bytes> <tx_bytes>\"",
                index + 1
            )));
        }
        let parse = |value: &str, field: &str| {
            value.parse::<u64>().map_err(|_| {
                AccountingIssue::Malformed(format!(
                    "line {}: {field} is not a byte count: {value:?}",
                    index + 1
                ))
            })
        };
        let rx_bytes = parse(parts[1], "rx_bytes")?;
        let tx_bytes = parse(parts[2], "tx_bytes")?;
        let app = parts[0].to_string();
        if !seen.insert(app.clone()) {
            return Err(AccountingIssue::Malformed(format!(
                "line {}: duplicate app {app:?}",
                index + 1
            )));
        }
        counters.push(AppNetworkCounters {
            app,
            rx_bytes,
            tx_bytes,
        });
    }
    counters.sort_by(|a, b| a.app.cmp(&b.app));
    Ok(counters)
}

/// Reads the accounting snapshot from the contract path.
#[derive(Debug, Clone)]
pub struct NetworkAccounting {
    pub path: PathBuf,
}

impl NetworkAccounting {
    pub fn system() -> Self {
        Self::at(NETWORK_SNAPSHOT_PATH)
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
        }
    }

    pub fn snapshot(&self) -> Result<Vec<AppNetworkCounters>, AccountingIssue> {
        if !self.path.exists() {
            return Err(AccountingIssue::NotInstalled);
        }
        let text = fs::read_to_string(&self.path).map_err(|error| {
            AccountingIssue::NotLoaded(format!("{}: {error}", self.path.display()))
        })?;
        parse_network_snapshot(&text)
    }
}

/// One app's network rate for the current sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppNetworkRate {
    pub app: String,
    /// Bytes received since the previous sample.
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

/// Per-app network rates plus the hourly totals behind the App history column.
///
/// Call [`Self::update`] on every Pack D sample (2 s while the Activity Manager
/// is visible, 30 s otherwise, D10 §3). The first call only primes the counters
/// because no interval has been measured yet.
#[derive(Debug, Default)]
pub struct AppNetworkHistory {
    previous: BTreeMap<String, (u64, u64)>,
    last_at_ms: Option<u64>,
    /// (app, hour) -> (rx bytes, tx bytes) observed in that hour.
    buckets: BTreeMap<(String, u64), (u64, u64)>,
    session_totals: BTreeMap<String, (u64, u64)>,
}

impl AppNetworkHistory {
    pub fn update(
        &mut self,
        counters: &[AppNetworkCounters],
        at_epoch_ms: u64,
    ) -> Vec<AppNetworkRate> {
        let elapsed = self
            .last_at_ms
            .map(|last| at_epoch_ms.saturating_sub(last) as f64 / 1000.0);
        let hour = hour_of(at_epoch_ms);
        let mut rates = Vec::new();
        for counter in counters {
            let delta = match self.previous.get(&counter.app) {
                // A counter that went backwards means the app restarted; the
                // delta for that interval is unknown, so report none.
                Some((rx, tx)) => (
                    counter.rx_bytes.saturating_sub(*rx),
                    counter.tx_bytes.saturating_sub(*tx),
                ),
                None => (0, 0),
            };
            self.previous
                .insert(counter.app.clone(), (counter.rx_bytes, counter.tx_bytes));

            if delta.0 > 0 || delta.1 > 0 {
                let bucket = self.buckets.entry((counter.app.clone(), hour)).or_default();
                *bucket = (bucket.0 + delta.0, bucket.1 + delta.1);
                let total = self.session_totals.entry(counter.app.clone()).or_default();
                *total = (total.0 + delta.0, total.1 + delta.1);
            }

            let seconds = elapsed.unwrap_or(0.0);
            rates.push(AppNetworkRate {
                app: counter.app.clone(),
                rx_bytes: delta.0,
                tx_bytes: delta.1,
                rx_bytes_per_sec: if seconds > 0.0 {
                    delta.0 as f64 / seconds
                } else {
                    0.0
                },
                tx_bytes_per_sec: if seconds > 0.0 {
                    delta.1 as f64 / seconds
                } else {
                    0.0
                },
            });
        }
        // Apps that disappeared keep their history but drop their counters so a
        // returning app starts a fresh interval.
        let live: HashSet<&str> = counters.iter().map(|counter| counter.app.as_str()).collect();
        self.previous.retain(|app, _| live.contains(app.as_str()));
        self.last_at_ms = Some(at_epoch_ms);
        self.prune(at_epoch_ms);
        rates
    }

    /// Bytes an app moved since `since_epoch_ms` (1-hour granularity).
    pub fn totals_for(&self, app: &str, since_epoch_ms: u64) -> (u64, u64) {
        let since_hour = hour_of(since_epoch_ms);
        self.buckets
            .iter()
            .filter(|((key, hour), _)| key == app && *hour >= since_hour)
            .fold((0, 0), |acc, (_, (rx, tx))| (acc.0 + rx, acc.1 + tx))
    }

    /// Bytes an app moved since this sampler started.
    pub fn session_total(&self, app: &str) -> (u64, u64) {
        self.session_totals
            .get(app)
            .copied()
            .unwrap_or((0, 0))
    }

    /// Apps that have a network history entry.
    pub fn apps(&self) -> BTreeSet<String> {
        self.buckets.keys().map(|(app, _)| app.clone()).collect()
    }

    fn prune(&mut self, at_epoch_ms: u64) {
        let cutoff = hour_of(at_epoch_ms).saturating_sub(HISTORY_RETENTION_HOURS);
        self.buckets.retain(|(_, hour), _| *hour >= cutoff);
    }
}

/// Per-app CPU seconds integrated from the 1 s process samples (TM-12).
///
/// `ProcessRow::cpu_percent` is a share of one core, so CPU seconds for one
/// interval are `cpu_percent / 100 × seconds`. Only rows inside a managed app
/// scope are attributed; system processes are not Pack D data.
#[derive(Debug, Default)]
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

    pub fn apps(&self) -> BTreeSet<String> {
        self.buckets.keys().map(|(app, _)| app.clone()).collect()
    }

    fn prune(&mut self, at_epoch_ms: u64) {
        let cutoff = hour_of(at_epoch_ms).saturating_sub(HISTORY_RETENTION_HOURS);
        self.buckets.retain(|(_, hour), _| *hour >= cutoff);
    }
}

/// DRM usage fields read from one `/proc/<pid>/fdinfo/<fd>` file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FdinfoDrm {
    pub pdev: Option<String>,
    /// Engine class ("render", "compute", "video", …) -> cumulative busy ns.
    pub engines_ns: BTreeMap<String, u64>,
    /// Memory region ("vram", "system", "stolen", …) -> bytes.
    pub memory_bytes: BTreeMap<String, u64>,
}

/// Parse the DRM keys of a fdinfo file. Returns `None` when the file has no DRM
/// usage fields (for example a render node opened but never used).
pub fn parse_fdinfo(text: &str) -> Option<FdinfoDrm> {
    let mut drm = FdinfoDrm::default();
    let mut seen = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key == "drm-pdev" {
            drm.pdev = Some(value.to_string());
            seen = true;
        } else if let Some(engine) = key.strip_prefix("drm-engine-") {
            if let Some(ns) = parse_nanoseconds(value) {
                drm.engines_ns.insert(engine.to_string(), ns);
                seen = true;
            }
        } else if let Some(region) = key.strip_prefix("drm-memory-") {
            if let Some(bytes) = parse_memory_bytes(value) {
                drm.memory_bytes.insert(region.to_string(), bytes);
                seen = true;
            }
        }
    }
    if seen {
        Some(drm)
    } else {
        None
    }
}

fn parse_nanoseconds(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let amount: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        None | Some("ns") => Some(amount),
        // Older kernels print microseconds or milliseconds; convert them so a
        // difference in unit cannot turn into a wrong percentage.
        Some("us") => Some(amount.saturating_mul(1_000)),
        Some("ms") => Some(amount.saturating_mul(1_000_000)),
        Some(_) => None,
    }
}

fn parse_memory_bytes(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let amount: u64 = parts.next()?.parse().ok()?;
    match parts.next().unwrap_or("B") {
        "B" | "bytes" => Some(amount),
        "KiB" | "kB" => Some(amount.saturating_mul(1024)),
        "MiB" | "MB" => Some(amount.saturating_mul(1024 * 1024)),
        "GiB" | "GB" => Some(amount.saturating_mul(1024 * 1024 * 1024)),
        _ => None,
    }
}

/// One engine's busy share for a process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuEngineUsage {
    pub engine: String,
    /// Busy share of the sampling interval, 0..100.
    pub busy_percent: f32,
}

/// Per-process GPU detail (TM-09 Pack D).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuProcessDetail {
    pub pid: u32,
    pub pdev: Option<String>,
    pub engines: Vec<GpuEngineUsage>,
    /// Region -> bytes; regions are merged across the process's DRM fds.
    pub memory_bytes: BTreeMap<String, u64>,
}

impl GpuProcessDetail {
    /// Highest engine share, the value the Processes/GPU column shows.
    pub fn busy_percent(&self) -> f32 {
        self.engines
            .iter()
            .map(|engine| engine.busy_percent)
            .fold(0.0_f32, f32::max)
    }

    pub fn memory_total_bytes(&self) -> u64 {
        self.memory_bytes.values().sum()
    }
}

/// Reads DRM fdinfo for every process holding a DRM file descriptor and turns
/// counter deltas into engine busy shares.
#[derive(Debug)]
pub struct GpuDetailSampler {
    fs: ProcFs,
    /// (pid, engine) -> (cumulative ns, sampled at ms).
    previous: BTreeMap<(u32, String), (u64, u64)>,
}

impl Default for GpuDetailSampler {
    fn default() -> Self {
        Self::new(ProcFs::system())
    }
}

impl GpuDetailSampler {
    pub fn new(fs: ProcFs) -> Self {
        Self {
            fs,
            previous: BTreeMap::new(),
        }
    }

    pub fn at(proc_root: impl Into<PathBuf>) -> Self {
        Self::new(ProcFs::at(proc_root))
    }

    pub fn sample(&mut self, at_epoch_ms: u64) -> Vec<GpuProcessDetail> {
        let mut details = Vec::new();
        let mut seen = HashSet::new();
        let Ok(entries) = fs::read_dir(&self.fs.proc_root) else {
            return details;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let dir = entry.path();
            let mut merged = FdinfoDrm::default();
            let Ok(fds) = fs::read_dir(dir.join("fd")) else {
                continue;
            };
            for fd in fds.flatten() {
                let is_drm = fs::read_link(fd.path())
                    .map(|target| target.to_string_lossy().contains("/dev/dri/"))
                    .unwrap_or(false);
                if !is_drm {
                    continue;
                }
                let Some(fd_name) = fd.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let Ok(text) = fs::read_to_string(dir.join("fdinfo").join(&fd_name)) else {
                    continue;
                };
                let Some(drm) = parse_fdinfo(&text) else {
                    continue;
                };
                if merged.pdev.is_none() {
                    merged.pdev = drm.pdev;
                }
                // A process can hold several fds for the same client; keep the
                // largest counter per engine and region so nothing is counted
                // twice.
                for (engine, ns) in drm.engines_ns {
                    let slot = merged.engines_ns.entry(engine).or_insert(0);
                    *slot = (*slot).max(ns);
                }
                for (region, bytes) in drm.memory_bytes {
                    let slot = merged.memory_bytes.entry(region).or_insert(0);
                    *slot = (*slot).max(bytes);
                }
            }
            if merged.engines_ns.is_empty() && merged.memory_bytes.is_empty() {
                continue;
            }
            seen.insert(pid);
            let mut engines = Vec::new();
            for (engine, ns) in &merged.engines_ns {
                let busy_percent = match self.previous.get(&(pid, engine.clone())) {
                    Some((old_ns, old_at)) => {
                        let elapsed_ns = at_epoch_ms.saturating_sub(*old_at) * 1_000_000;
                        if elapsed_ns == 0 {
                            0.0
                        } else {
                            let delta = ns.saturating_sub(*old_ns);
                            (delta as f64 / elapsed_ns as f64 * 100.0).clamp(0.0, 100.0) as f32
                        }
                    }
                    None => 0.0,
                };
                self.previous
                    .insert((pid, engine.clone()), (*ns, at_epoch_ms));
                engines.push(GpuEngineUsage {
                    engine: engine.clone(),
                    busy_percent,
                });
            }
            engines.sort_by(|a, b| a.engine.cmp(&b.engine));
            details.push(GpuProcessDetail {
                pid,
                pdev: merged.pdev,
                engines,
                memory_bytes: merged.memory_bytes,
            });
        }
        self.previous.retain(|(pid, _), _| seen.contains(pid));
        details.sort_by_key(|detail| detail.pid);
        details
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
    use std::io::Write;

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
    fn snapshot_parser_is_strict_and_never_invents_zero() {
        let text = format!(
            "{NETWORK_SNAPSHOT_HEADER}\n# app_id rx_bytes tx_bytes\norg.b 2048 512\norg.a 1024 256\n"
        );
        let counters = parse_network_snapshot(&text).unwrap();
        assert_eq!(counters.len(), 2);
        // Sorted by app so the table order is stable.
        assert_eq!(counters[0].app, "org.a");
        assert_eq!(counters[0].rx_bytes, 1024);
        assert_eq!(counters[1].tx_bytes, 512);

        assert_eq!(
            parse_network_snapshot(""),
            Err(AccountingIssue::Malformed("the snapshot is empty".into()))
        );
        assert!(matches!(
            parse_network_snapshot("aether-net-accounting v2\norg.a 1 2\n"),
            Err(AccountingIssue::Malformed(reason)) if reason.contains("header")
        ));
        assert!(matches!(
            parse_network_snapshot(&format!("{NETWORK_SNAPSHOT_HEADER}\norg.a 1\n")),
            Err(AccountingIssue::Malformed(reason)) if reason.contains("line 2")
        ));
        assert!(matches!(
            parse_network_snapshot(&format!("{NETWORK_SNAPSHOT_HEADER}\norg.a 1 nope\n")),
            Err(AccountingIssue::Malformed(reason)) if reason.contains("tx_bytes")
        ));
        assert!(matches!(
            parse_network_snapshot(&format!(
                "{NETWORK_SNAPSHOT_HEADER}\norg.a 1 2\norg.a 3 4\n"
            )),
            Err(AccountingIssue::Malformed(reason)) if reason.contains("duplicate")
        ));
    }

    #[test]
    fn missing_snapshot_means_pack_not_installed() {
        let dir = temp_dir("missing");
        let accounting = NetworkAccounting::at(dir.join("net-accounting"));
        assert_eq!(accounting.snapshot(), Err(AccountingIssue::NotInstalled));
        assert_eq!(
            AccountingIssue::NotInstalled.status(),
            PackDStatus::NotInstalled
        );
        assert!(AccountingIssue::NotInstalled
            .message()
            .contains(NETWORK_SNAPSHOT_PATH));

        let path = dir.join("net-accounting");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{NETWORK_SNAPSHOT_HEADER}\norg.a 10 20").unwrap();
        let counters = NetworkAccounting::at(&path).snapshot().unwrap();
        assert_eq!(counters.len(), 1);
        assert_eq!(counters[0].tx_bytes, 20);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn network_rates_primes_then_integrates_into_hourly_totals() {
        let mut history = AppNetworkHistory::default();
        let first = history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 1_000,
                tx_bytes: 500,
            }],
            3_600_000,
        );
        // No interval measured yet: 0 B/s, not a guess.
        assert_eq!(first[0].rx_bytes_per_sec, 0.0);
        assert_eq!(history.session_total("org.a"), (0, 0));

        let second = history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 5_000,
                tx_bytes: 1_500,
            }],
            3_602_000,
        );
        assert_eq!(second[0].rx_bytes, 4_000);
        assert_eq!(second[0].tx_bytes, 1_000);
        assert_eq!(second[0].rx_bytes_per_sec, 2_000.0);
        assert_eq!(second[0].tx_bytes_per_sec, 500.0);
        assert_eq!(history.session_total("org.a"), (4_000, 1_000));
        assert_eq!(history.totals_for("org.a", 0), (4_000, 1_000));
        // Before this hour there is nothing.
        assert_eq!(history.totals_for("org.a", 7_200_000), (0, 0));
        assert_eq!(history.apps().len(), 1);

        // A restarted app resets its counters: no negative delta.
        let reset = history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 10,
                tx_bytes: 0,
            }],
            3_604_000,
        );
        assert_eq!(reset[0].rx_bytes, 0);
        assert_eq!(reset[0].tx_bytes, 0);
        assert_eq!(history.session_total("org.a"), (4_000, 1_000));
    }

    #[test]
    fn network_buckets_expire_after_the_retention_window() {
        let mut history = AppNetworkHistory::default();
        let hour_ms = 3_600_000u64;
        history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 100,
                tx_bytes: 0,
            }],
            0,
        );
        history.update(
            &[AppNetworkCounters {
                app: "org.a".into(),
                rx_bytes: 200,
                tx_bytes: 0,
            }],
            hour_ms,
        );
        assert_eq!(history.totals_for("org.a", 0), (100, 0));
        // Eight days later the old bucket is gone, including from the window.
        let later = (HISTORY_RETENTION_HOURS + 2) * hour_ms;
        history.update(
            &[AppNetworkCounters {
                app: "org.b".into(),
                rx_bytes: 1,
                tx_bytes: 1,
            }],
            later,
        );
        assert_eq!(history.totals_for("org.a", 0), (0, 0));
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
    fn fdinfo_parser_reads_intel_and_amd_keys() {
        let i915 = "\
pos:\t0
flags:\t02100002
drm-driver:\ti915
drm-pdev:\t0000:00:02.0
drm-client-id:\t12
drm-engine-render:\t1234567890 ns
drm-engine-copy:\t0 ns
drm-engine-video:\t500000000 ns
drm-memory-vram:\t4096 KiB
drm-memory-system:\t1024 KiB
";
        let drm = parse_fdinfo(i915).unwrap();
        assert_eq!(drm.pdev.as_deref(), Some("0000:00:02.0"));
        assert_eq!(drm.engines_ns["render"], 1_234_567_890);
        assert_eq!(drm.engines_ns["copy"], 0);
        assert_eq!(drm.engines_ns["video"], 500_000_000);
        assert_eq!(drm.memory_bytes["vram"], 4_194_304);
        assert_eq!(drm.memory_bytes["system"], 1_048_576);

        let amdgpu = "drm-pdev:\t0000:03:00.0\ndrm-engine-gfx:\t1000 ns\ndrm-engine-compute:\t2000 ns\ndrm-memory-vram:\t1 MiB\n";
        let drm = parse_fdinfo(amdgpu).unwrap();
        assert_eq!(drm.engines_ns["gfx"], 1_000);
        assert_eq!(drm.engines_ns["compute"], 2_000);
        assert_eq!(drm.memory_bytes["vram"], 1_048_576);

        // Microsecond fields are converted so percentages stay correct.
        let older = "drm-engine-render:\t1500 us\n";
        assert_eq!(parse_fdinfo(older).unwrap().engines_ns["render"], 1_500_000);

        // No DRM keys: not a DRM fd.
        assert!(parse_fdinfo("pos:\t0\nflags:\t02100002\n").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn gpu_sampler_turns_counter_deltas_into_engine_busy_percent() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir("gpu");
        let pid_dir = dir.join("proc/4321");
        fs::create_dir_all(pid_dir.join("fd")).unwrap();
        fs::create_dir_all(pid_dir.join("fdinfo")).unwrap();
        symlink("/dev/dri/renderD128", pid_dir.join("fd/7")).unwrap();
        // A non-DRM fd must be ignored.
        symlink("/tmp/socket", pid_dir.join("fd/9")).unwrap();
        let fdinfo = pid_dir.join("fdinfo/7");
        let mut file = fs::File::create(&fdinfo).unwrap();
        writeln!(
            file,
            "drm-pdev:\t0000:00:02.0\ndrm-engine-render:\t1000000000 ns\ndrm-memory-vram:\t2048 KiB"
        )
        .unwrap();

        let mut sampler = GpuDetailSampler::at(dir.join("proc"));
        let first = sampler.sample(1_000);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].pid, 4321);
        // First sample primes the counter: no invented busy share.
        assert_eq!(first[0].busy_percent(), 0.0);
        assert_eq!(first[0].memory_total_bytes(), 2_097_152);
        assert_eq!(first[0].pdev.as_deref(), Some("0000:00:02.0"));

        // 500 ms of engine time in a 1000 ms interval = 50%.
        let mut file = fs::File::create(&fdinfo).unwrap();
        writeln!(
            file,
            "drm-pdev:\t0000:00:02.0\ndrm-engine-render:\t1500000000 ns\ndrm-memory-vram:\t2048 KiB"
        )
        .unwrap();
        let second = sampler.sample(2_000);
        assert!((second[0].busy_percent() - 50.0).abs() < 1e-3);
        let _ = fs::remove_dir_all(dir);
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
        let json = serde_json::to_string(&contract).unwrap();
        assert!(json.contains("\"key\":\"pack-d\""));
        assert!(json.contains("\"refresh_visible_seconds\":2"));
    }
}
