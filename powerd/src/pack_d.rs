//! Pack D network accounting recorder (P7-PK-T01, TM-12/TM-15; D10 §2, §5.2).
//!
//! Pack D adds per-app network from kernel socket accounting. The eBPF loader
//! dumps its map to a published snapshot path; this module reads that snapshot,
//! turns cumulative counters into deltas, and rolls the deltas into hourly
//! buckets. [`NetworkRecorder`] is what `aether-powerd` samples on its loop, so
//! the buckets fill whether or not the Activity Manager is open, and
//! [`NetworkRecorder::save`] persists them for the App history window.
//!
//! Attaching the eBPF program and validating the numbers on real hardware are
//! hardware-gated; nothing here needs a kernel beyond reading [`proc`] fixtures,
//! and a missing source is reported, never shown as zero (D10 §1).
//!
//! [`proc`]: std::fs

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where the Pack D loader dumps its socket-accounting map.
pub const NETWORK_SNAPSHOT_PATH: &str = "/run/aether/pack-d/net-accounting";

/// First line of the accounting snapshot; the parser refuses anything else.
pub const NETWORK_SNAPSHOT_HEADER: &str = "aether-net-accounting v1";

/// History retention for the Pack D hourly buckets (8 days covers the 7-day
/// App history window with one hour of slack for the roll-up granularity).
pub const HISTORY_RETENTION_HOURS: u64 = 8 * 24;

/// Refresh cadence from D10 §3 (Pack D row).
pub const PACK_D_REFRESH_VISIBLE_SECONDS: u32 = 2;
pub const PACK_D_REFRESH_HIDDEN_SECONDS: u32 = 30;

/// Format version of the persisted history file.
pub const HISTORY_FILE_VERSION: u32 = 1;

fn hour_of(epoch_ms: u64) -> u64 {
    epoch_ms / 3_600_000
}

/// State of the network source, for the honest "not installed" wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkSourceStatus {
    /// Pack D is installed and the snapshot was read.
    Reported,
    /// Pack D is not installed: the snapshot path is absent (D10 §7).
    NotInstalled,
    /// Pack D is installed but its snapshot could not be read or parsed.
    NotLoaded,
}

impl NetworkSourceStatus {
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
    pub fn status(&self) -> NetworkSourceStatus {
        match self {
            Self::NotInstalled => NetworkSourceStatus::NotInstalled,
            Self::NotLoaded(_) | Self::Malformed(_) => NetworkSourceStatus::NotLoaded,
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

/// One hourly bucket in the persisted history file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkBucketRecord {
    pub app: String,
    /// Hour number since the Unix epoch (`epoch_ms / 3_600_000`).
    pub hour: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// On-disk form of [`AppNetworkHistory`]: hourly buckets only.
///
/// Counter state and session totals are deliberately not stored. Counters are
/// cumulative kernel values, so a restart only costs one priming interval;
/// session totals mean "since this sampler started" and restart with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkHistoryStore {
    pub version: u32,
    pub buckets: Vec<NetworkBucketRecord>,
}

/// Per-app network rates plus the hourly totals behind the App history column.
///
/// Call [`Self::update`] on every Pack D sample (2 s while the Activity Manager
/// is visible, 30 s otherwise, D10 §3). The first call only primes the counters
/// because no interval has been measured yet.
#[derive(Debug, Default, Clone)]
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

    /// Hourly buckets, for the persisted history file.
    pub fn buckets(&self) -> Vec<NetworkBucketRecord> {
        self.buckets
            .iter()
            .map(|((app, hour), (rx, tx))| NetworkBucketRecord {
                app: app.clone(),
                hour: *hour,
                rx_bytes: *rx,
                tx_bytes: *tx,
            })
            .collect()
    }

    /// Replace the buckets from a persisted history file.
    pub fn restore_buckets(&mut self, records: &[NetworkBucketRecord]) {
        for record in records {
            let slot = self
                .buckets
                .entry((record.app.clone(), record.hour))
                .or_insert((0, 0));
            *slot = (record.rx_bytes, record.tx_bytes);
        }
    }

    pub fn to_store(&self) -> NetworkHistoryStore {
        NetworkHistoryStore {
            version: HISTORY_FILE_VERSION,
            buckets: self.buckets(),
        }
    }

    pub fn from_store(store: NetworkHistoryStore) -> Self {
        let mut history = Self::default();
        history.restore_buckets(&store.buckets);
        history
    }

    fn prune(&mut self, at_epoch_ms: u64) {
        let cutoff = hour_of(at_epoch_ms).saturating_sub(HISTORY_RETENTION_HOURS);
        self.buckets.retain(|(_, hour), _| *hour >= cutoff);
    }
}

/// One sample of the network source.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkSample {
    pub status: NetworkSourceStatus,
    pub rates: Vec<AppNetworkRate>,
    /// Why the source is not reporting, when it is not.
    pub message: Option<String>,
}

/// Reads the snapshot on every daemon tick and keeps the hourly buckets.
///
/// `aether-powerd` owns one of these: the buckets therefore fill at the daemon
/// cadence (15 s by default) whether or not the Activity Manager is open, and
/// survive a restart through [`Self::save`]/[`Self::load`].
#[derive(Debug, Clone)]
pub struct NetworkRecorder {
    accounting: NetworkAccounting,
    history: AppNetworkHistory,
    status: NetworkSourceStatus,
    message: Option<String>,
}

impl Default for NetworkRecorder {
    fn default() -> Self {
        Self::system()
    }
}

impl NetworkRecorder {
    pub fn system() -> Self {
        Self::at(NETWORK_SNAPSHOT_PATH)
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            accounting: NetworkAccounting::at(path),
            history: AppNetworkHistory::default(),
            status: NetworkSourceStatus::NotInstalled,
            message: None,
        }
    }

    /// Load the persisted buckets written by an earlier run (best effort).
    pub fn load_history(&mut self, path: &Path) -> io::Result<()> {
        let store = load_store(path)?;
        self.history.restore_buckets(&store.buckets);
        Ok(())
    }

    /// Sample the snapshot and integrate the delta into the hourly buckets.
    ///
    /// A missing or unreadable source changes the status and keeps the history;
    /// it never reports bytes that were not observed.
    pub fn sample(&mut self, at_epoch_ms: u64) -> NetworkSample {
        match self.accounting.snapshot() {
            Ok(counters) => {
                self.status = NetworkSourceStatus::Reported;
                self.message = None;
                NetworkSample {
                    status: self.status,
                    rates: self.history.update(&counters, at_epoch_ms),
                    message: None,
                }
            }
            Err(issue) => {
                self.status = issue.status();
                self.message = Some(issue.message());
                NetworkSample {
                    status: self.status,
                    rates: Vec::new(),
                    message: self.message.clone(),
                }
            }
        }
    }

    pub fn status(&self) -> NetworkSourceStatus {
        self.status
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn history(&self) -> &AppNetworkHistory {
        &self.history
    }

    /// Mutable history, for seeding buckets from a persisted file in tests and
    /// for the Activity Manager's demo data.
    pub fn history_mut(&mut self) -> &mut AppNetworkHistory {
        &mut self.history
    }

    pub fn accounting(&self) -> &NetworkAccounting {
        &self.accounting
    }

    /// Persist the hourly buckets (8-day retention).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let store = self.history.to_store();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&store)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, text)
    }
}

fn load_store(path: &Path) -> io::Result<NetworkHistoryStore> {
    let text = fs::read_to_string(path)?;
    let store: NetworkHistoryStore = serde_json::from_str(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if store.version != HISTORY_FILE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported Pack D history version {}", store.version),
        ));
    }
    Ok(store)
}

/// Load a persisted history file produced by [`NetworkRecorder::save`].
pub fn load_history(path: &Path) -> io::Result<AppNetworkHistory> {
    Ok(AppNetworkHistory::from_store(load_store(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-powerd-pack-d-{tag}-{}-{}-{}",
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

    fn write_snapshot(path: &Path, rows: &[(&str, u64, u64)]) {
        let mut text = format!("{NETWORK_SNAPSHOT_HEADER}\n");
        for (app, rx, tx) in rows {
            text.push_str(&format!("{app} {rx} {tx}\n"));
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
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
            NetworkSourceStatus::NotInstalled
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
    fn recorder_fills_buckets_on_each_sample_and_reports_the_source_honestly() {
        let dir = temp_dir("recorder");
        let snapshot = dir.join("net-accounting");
        let mut recorder = NetworkRecorder::at(&snapshot);

        // No snapshot yet: not installed, and no bytes invented.
        let sample = recorder.sample(0);
        assert_eq!(sample.status, NetworkSourceStatus::NotInstalled);
        assert!(sample.message.unwrap().contains(NETWORK_SNAPSHOT_PATH));
        assert!(recorder.history().apps().is_empty());

        write_snapshot(&snapshot, &[("org.a", 1_000, 200)]);
        let sample = recorder.sample(1_000);
        assert_eq!(sample.status, NetworkSourceStatus::Reported);
        assert_eq!(sample.rates[0].rx_bytes, 0, "the first sample primes");

        write_snapshot(&snapshot, &[("org.a", 4_000, 800)]);
        let sample = recorder.sample(3_000);
        assert_eq!(sample.rates[0].rx_bytes, 3_000);
        assert_eq!(sample.rates[0].tx_bytes, 600);
        assert_eq!(recorder.history().totals_for("org.a", 0), (3_000, 600));

        // A malformed snapshot is "not loaded" and keeps the history intact.
        fs::write(&snapshot, "garbage\n").unwrap();
        let sample = recorder.sample(5_000);
        assert_eq!(sample.status, NetworkSourceStatus::NotLoaded);
        assert!(sample.message.unwrap().contains("malformed"));
        assert_eq!(recorder.history().totals_for("org.a", 0), (3_000, 600));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn persisted_buckets_survive_a_daemon_restart() {
        let dir = temp_dir("persist");
        let snapshot = dir.join("net-accounting");
        let history_path = dir.join("pack-d-network.json");
        let mut recorder = NetworkRecorder::at(&snapshot);
        write_snapshot(&snapshot, &[("org.a", 1_000, 0), ("org.b", 10, 10)]);
        recorder.sample(0);
        write_snapshot(&snapshot, &[("org.a", 3_000, 500), ("org.b", 30, 30)]);
        recorder.sample(2_000);
        recorder.save(&history_path).unwrap();

        // The restarted daemon starts a fresh recorder and restores the buckets.
        let mut restarted = NetworkRecorder::at(&snapshot);
        restarted.load_history(&history_path).unwrap();
        assert_eq!(restarted.history().totals_for("org.a", 0), (2_000, 500));
        assert_eq!(restarted.history().totals_for("org.b", 0), (20, 20));
        // The first sample after a restart primes again: no invented spike.
        let sample = restarted.sample(600_000);
        assert_eq!(sample.rates[0].rx_bytes, 0);

        // The standalone loader reads the same file for the Activity Manager.
        let loaded = load_history(&history_path).unwrap();
        assert_eq!(loaded.totals_for("org.a", 0), (2_000, 500));

        // A future format version is refused, not mis-read.
        fs::write(&history_path, r#"{"version":99,"buckets":[]}"#).unwrap();
        assert!(load_history(&history_path).is_err());

        let _ = fs::remove_dir_all(dir);
    }
}
