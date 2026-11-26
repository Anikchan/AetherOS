//! Pack D GPU recorder (P7-PK-T01, TM-09; D10 §2, §4, §5.2).
//!
//! Pack D adds per-process GPU detail from the DRM usage fields in
//! `/proc/<pid>/fdinfo/<fd>`: engine busy time and per-region memory. This
//! module parses those keys, turns engine counter deltas into busy shares, and
//! rolls the busy time up per app and per hour so the hourly buckets fill from
//! `aether-lifecycled`'s loop instead of only while a window is open.
//!
//! Validating the numbers on the reference Intel and AMD GPUs is
//! hardware-gated; parsing and aggregation run against fixture trees here, and a
//! process that cannot be attributed to an app scope is reported, never counted
//! as zero.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::app_id_from_scope;

/// Pack D history retention: 8 days of hourly buckets (D10 §5.2).
pub const HISTORY_RETENTION_HOURS: u64 = 8 * 24;

/// Pack D sample cadence while the Activity Manager is closed (D10 §3).
pub const GPU_SAMPLE_INTERVAL_SECONDS: u64 = 30;

/// Format version of the persisted GPU history file.
pub const GPU_HISTORY_FILE_VERSION: u32 = 1;

/// Where the daemon reads the process table from.
pub const PROC_ROOT: &str = "/proc";

fn hour_of(epoch_ms: u64) -> u64 {
    epoch_ms / 3_600_000
}

/// State of the GPU source, for the honest "not loaded" wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpuSourceStatus {
    /// `/proc` was read; the sample lists every process holding a DRM fd.
    Reported,
    /// `/proc` could not be read (no procfs, or no permission).
    NotLoaded,
}

impl GpuSourceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Reported => "Reported",
            Self::NotLoaded => "Not loaded",
        }
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
    proc_root: PathBuf,
    /// (pid, engine) -> (cumulative ns, sampled at ms).
    previous: BTreeMap<(u32, String), (u64, u64)>,
}

impl Default for GpuDetailSampler {
    fn default() -> Self {
        Self::system()
    }
}

impl GpuDetailSampler {
    pub fn system() -> Self {
        Self::at(PROC_ROOT)
    }

    pub fn at(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
            previous: BTreeMap::new(),
        }
    }

    pub fn proc_root(&self) -> &Path {
        &self.proc_root
    }

    pub fn sample(&mut self, at_epoch_ms: u64) -> Vec<GpuProcessDetail> {
        let mut details = Vec::new();
        let mut seen = HashSet::new();
        let Ok(entries) = fs::read_dir(&self.proc_root) else {
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

/// One hour of GPU busy time attributed to one app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuBucketRecord {
    pub app: String,
    /// Hour number since the Unix epoch (`epoch_ms / 3_600_000`).
    pub hour: u64,
    pub busy_ns: u64,
}

/// On-disk form of [`GpuHistory`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuHistoryStore {
    pub version: u32,
    pub buckets: Vec<GpuBucketRecord>,
}

/// Per-app GPU busy time in hourly buckets (TM-09 history, D10 §5.2).
///
/// Only busy time for processes that live in an `aether-app-*` scope is
/// attributed; the rest of the desktop is system work, not app data.
#[derive(Debug, Default, Clone)]
pub struct GpuHistory {
    last_at_ms: Option<u64>,
    buckets: BTreeMap<(String, u64), u64>,
}

impl GpuHistory {
    /// Add the process sample to the current hour's buckets.
    ///
    /// `proc_root` is used only to read the process's cgroup, so a pid that
    /// cannot be attributed is skipped instead of counted as unknown. Busy time
    /// is the engine share times the interval since the previous sample, so a
    /// delayed loop cannot inflate the total.
    pub fn observe(&mut self, details: &[GpuProcessDetail], proc_root: &Path, at_epoch_ms: u64) {
        let hour = hour_of(at_epoch_ms);
        let elapsed_ns = self
            .last_at_ms
            .map(|last| at_epoch_ms.saturating_sub(last).saturating_mul(1_000_000))
            .filter(|ns| *ns > 0)
            .unwrap_or(INTERVAL_NS);
        for detail in details {
            let busy = detail.busy_percent();
            if busy <= 0.0 {
                continue;
            }
            let Some(app) = app_of_pid(proc_root, detail.pid) else {
                continue;
            };
            let busy_ns = (f64::from(busy) / 100.0 * elapsed_ns as f64) as u64;
            let slot = self.buckets.entry((app, hour)).or_insert(0);
            *slot = slot.saturating_add(busy_ns);
        }
        self.last_at_ms = Some(at_epoch_ms);
        self.prune(at_epoch_ms);
    }

    /// GPU busy nanoseconds for an app since `since_epoch_ms` (hourly granularity).
    pub fn total_ns_for(&self, app: &str, since_epoch_ms: u64) -> u64 {
        let since_hour = hour_of(since_epoch_ms);
        self.buckets
            .iter()
            .filter(|((key, hour), _)| key == app && *hour >= since_hour)
            .map(|(_, ns)| *ns)
            .sum()
    }

    pub fn apps(&self) -> BTreeSet<String> {
        self.buckets.keys().map(|(app, _)| app.clone()).collect()
    }

    pub fn buckets(&self) -> Vec<GpuBucketRecord> {
        self.buckets
            .iter()
            .map(|((app, hour), busy_ns)| GpuBucketRecord {
                app: app.clone(),
                hour: *hour,
                busy_ns: *busy_ns,
            })
            .collect()
    }

    pub fn restore_buckets(&mut self, records: &[GpuBucketRecord]) {
        for record in records {
            let slot = self.buckets.entry((record.app.clone(), record.hour)).or_insert(0);
            *slot = record.busy_ns;
        }
    }

    pub fn to_store(&self) -> GpuHistoryStore {
        GpuHistoryStore {
            version: GPU_HISTORY_FILE_VERSION,
            buckets: self.buckets(),
        }
    }

    pub fn from_store(store: GpuHistoryStore) -> Self {
        let mut history = Self::default();
        history.restore_buckets(&store.buckets);
        history
    }

    fn prune(&mut self, at_epoch_ms: u64) {
        let cutoff = hour_of(at_epoch_ms).saturating_sub(HISTORY_RETENTION_HOURS);
        self.buckets.retain(|(_, hour), _| *hour >= cutoff);
    }
}

/// The sampling interval the busy share is a fraction of (D10 §3 hidden rate).
const INTERVAL_NS: u64 = GPU_SAMPLE_INTERVAL_SECONDS * 1_000_000_000;

fn app_of_pid(proc_root: &Path, pid: u32) -> Option<String> {
    let text = fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup")).ok()?;
    text.lines().find_map(|line| {
        // "0::/aether.slice/aether-apps.slice/aether-app-org.a-1a2b3c4d.scope"
        let path = line.rsplit(':').next()?.trim();
        app_id_from_scope(path)
    })
}

/// One sample of the GPU source.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSample {
    pub status: GpuSourceStatus,
    pub processes: Vec<GpuProcessDetail>,
    /// Why the source is not reporting, when it is not.
    pub message: Option<String>,
}

/// Samples fdinfo on every daemon tick and keeps the per-app hourly buckets.
///
/// `aether-lifecycled` owns one of these so GPU busy time is attributed per app
/// even when the Activity Manager is closed; the buckets survive a restart
/// through [`Self::save`]/[`Self::load_history`].
#[derive(Debug)]
pub struct GpuRecorder {
    sampler: GpuDetailSampler,
    history: GpuHistory,
    status: GpuSourceStatus,
    message: Option<String>,
}

impl Default for GpuRecorder {
    fn default() -> Self {
        Self::system()
    }
}

impl GpuRecorder {
    pub fn system() -> Self {
        Self::at(PROC_ROOT)
    }

    pub fn at(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            sampler: GpuDetailSampler::at(proc_root),
            history: GpuHistory::default(),
            status: GpuSourceStatus::NotLoaded,
            message: None,
        }
    }

    /// Load the persisted buckets written by an earlier run (best effort).
    pub fn load_history(&mut self, path: &Path) -> io::Result<()> {
        let store = load_store(path)?;
        self.history.restore_buckets(&store.buckets);
        Ok(())
    }

    /// Sample fdinfo and add the busy time to the hourly buckets.
    pub fn sample(&mut self, at_epoch_ms: u64) -> GpuSample {
        if !self.sampler.proc_root().is_dir() {
            self.status = GpuSourceStatus::NotLoaded;
            self.message = Some(format!(
                "no procfs at {} (Pack D GPU detail is not loaded)",
                self.sampler.proc_root().display()
            ));
            return GpuSample {
                status: self.status,
                processes: Vec::new(),
                message: self.message.clone(),
            };
        }
        let processes = self.sampler.sample(at_epoch_ms);
        self.history
            .observe(&processes, self.sampler.proc_root(), at_epoch_ms);
        self.status = GpuSourceStatus::Reported;
        self.message = None;
        GpuSample {
            status: self.status,
            processes,
            message: None,
        }
    }

    pub fn status(&self) -> GpuSourceStatus {
        self.status
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn history(&self) -> &GpuHistory {
        &self.history
    }

    pub fn sampler(&self) -> &GpuDetailSampler {
        &self.sampler
    }

    /// Persist the hourly buckets (8-day retention).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        save_store(&self.history.to_store(), path)
    }
}

/// Load a persisted history file produced by [`GpuRecorder::save`].
pub fn load_gpu_history(path: &Path) -> io::Result<GpuHistory> {
    Ok(GpuHistory::from_store(load_store(path)?))
}

fn save_store(store: &GpuHistoryStore, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(store)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(path, text)
}

fn load_store(path: &Path) -> io::Result<GpuHistoryStore> {
    let text = fs::read_to_string(path)?;
    let store: GpuHistoryStore = serde_json::from_str(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if store.version != GPU_HISTORY_FILE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported Pack D GPU history version {}", store.version),
        ));
    }
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-gpu-pack-d-{tag}-{}-{}-{}",
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

    #[cfg(unix)]
    fn process_with_drm_fd(root: &Path, pid: u32, app: &str, render_ns: u64) {
        use std::os::unix::fs::symlink;
        let pid_dir = root.join(pid.to_string());
        fs::create_dir_all(pid_dir.join("fd")).unwrap();
        fs::create_dir_all(pid_dir.join("fdinfo")).unwrap();
        // Called twice per test to move the counter: replace the links.
        let _ = fs::remove_file(pid_dir.join("fd/7"));
        let _ = fs::remove_file(pid_dir.join("fd/9"));
        symlink("/dev/dri/renderD128", pid_dir.join("fd/7")).unwrap();
        symlink("/tmp/socket", pid_dir.join("fd/9")).unwrap();
        fs::write(
            pid_dir.join("cgroup"),
            format!(
                "0::/aether.slice/aether-apps.slice/aether-app-{app}-1a2b3c4d.scope\n"
            ),
        )
        .unwrap();
        let mut file = fs::File::create(pid_dir.join("fdinfo/7")).unwrap();
        writeln!(
            file,
            "drm-pdev:\t0000:00:02.0\ndrm-engine-render:\t{render_ns} ns\ndrm-memory-vram:\t2048 KiB"
        )
        .unwrap();
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
        let dir = temp_dir("gpu");
        process_with_drm_fd(&dir, 4321, "org.aether.Player", 1_000_000_000);

        let mut sampler = GpuDetailSampler::at(dir.clone());
        let first = sampler.sample(1_000);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].pid, 4321);
        // First sample primes the counter: no invented busy share.
        assert_eq!(first[0].busy_percent(), 0.0);
        assert_eq!(first[0].memory_total_bytes(), 2_097_152);
        assert_eq!(first[0].pdev.as_deref(), Some("0000:00:02.0"));

        // 500 ms of engine time in a 1000 ms interval = 50%.
        process_with_drm_fd(&dir, 4321, "org.aether.Player", 1_500_000_000);
        let second = sampler.sample(2_000);
        assert!((second[0].busy_percent() - 50.0).abs() < 1e-3);
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn gpu_history_buckets_busy_time_per_app_and_skips_system_pids() {
        let dir = temp_dir("history");
        process_with_drm_fd(&dir, 100, "org.aether.Player", 0);
        // A system process with a DRM fd and no app scope is not app data.
        let system = dir.join("200");
        fs::create_dir_all(system.join("fd")).unwrap();
        fs::create_dir_all(system.join("fdinfo")).unwrap();
        std::os::unix::fs::symlink("/dev/dri/card0", system.join("fd/3")).unwrap();
        fs::write(system.join("cgroup"), "0::/system.slice/sshd.service\n").unwrap();
        fs::write(
            system.join("fdinfo/3"),
            "drm-engine-render:\t0 ns\n",
        )
        .unwrap();

        let mut sampler = GpuDetailSampler::at(dir.clone());
        let mut history = GpuHistory::default();
        let first = sampler.sample(0);
        history.observe(&first, &dir, 0);
        assert!(history.apps().is_empty(), "the primer measures nothing");

        process_with_drm_fd(&dir, 100, "org.aether.Player", 15_000_000_000);
        let second = sampler.sample(GPU_SAMPLE_INTERVAL_SECONDS * 1_000);
        history.observe(&second, &dir, GPU_SAMPLE_INTERVAL_SECONDS * 1_000);
        assert_eq!(history.apps().len(), 1);
        let busy = history.total_ns_for("org.aether.Player", 0);
        // 50% of the 30 s interval = 15 busy seconds.
        assert_eq!(busy, 15 * 1_000_000_000);
        // Outside the window: nothing.
        assert_eq!(history.total_ns_for("org.aether.Player", 7_200_000), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recorder_reports_a_missing_procfs_instead_of_zero() {
        let dir = temp_dir("missing");
        let mut recorder = GpuRecorder::at(dir.join("proc"));
        let sample = recorder.sample(0);
        assert_eq!(sample.status, GpuSourceStatus::NotLoaded);
        assert!(sample.processes.is_empty());
        assert!(sample.message.unwrap().contains("no procfs"));
        assert!(recorder.history().apps().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_gpu_buckets_survive_a_daemon_restart() {
        let dir = temp_dir("persist");
        let path = dir.join("pack-d-gpu.json");
        process_with_drm_fd(&dir, 100, "org.aether.Player", 0);
        let mut recorder = GpuRecorder::at(dir.clone());
        recorder.sample(0);
        process_with_drm_fd(&dir, 100, "org.aether.Player", 15_000_000_000);
        recorder.sample(GPU_SAMPLE_INTERVAL_SECONDS * 1_000);
        assert_eq!(
            recorder.history().total_ns_for("org.aether.Player", 0),
            15 * 1_000_000_000
        );
        recorder.save(&path).unwrap();

        let mut restarted = GpuRecorder::at(dir.clone());
        restarted.load_history(&path).unwrap();
        assert_eq!(
            restarted.history().total_ns_for("org.aether.Player", 0),
            15 * 1_000_000_000
        );
        let loaded = load_gpu_history(&path).unwrap();
        assert_eq!(loaded.apps().len(), 1);

        // A future format version is refused, not mis-read.
        fs::write(&path, r#"{"version":99,"buckets":[]}"#).unwrap();
        assert!(load_gpu_history(&path).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
