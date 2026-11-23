//! Metric collection for the Activity Manager (D10 sources, F13 TM-04..TM-10).
//!
//! Every filesystem root is configurable so parsing is tested against fixture
//! trees on any host. Reads are intentionally cheap: one stat per process and
//! expensive per-process memory detail only where requested (D10 / F13 §6).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use aether_lifecycled::pressure::{PressureLevel, PsiReader};

use crate::model::{BatteryView, ProcessRow, SensorKind, SensorReading, SystemSample};

/// Filesystem view for the collector.
#[derive(Debug, Clone)]
pub struct ProcFs {
    pub proc_root: PathBuf,
    pub sys_root: PathBuf,
}

impl ProcFs {
    pub fn system() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
            sys_root: PathBuf::from("/sys"),
        }
    }

    pub fn at(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
            sys_root: PathBuf::from("/sys"),
        }
    }

    pub fn with_sys(mut self, sys_root: impl Into<PathBuf>) -> Self {
        self.sys_root = sys_root.into();
        self
    }
}

fn ticks_per_second() -> f64 {
    #[cfg(unix)]
    unsafe {
        let value = libc::sysconf(libc::_SC_CLK_TCK);
        if value > 0 {
            return value as f64;
        }
    }
    100.0
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_u64_file(path: &Path) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

/// `pid (comm) state ppid ...` → (comm, state, ppid, utime, stime, threads).
pub fn parse_stat(text: &str) -> Option<(String, char, u32, u64, u64, u32)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    if rest.len() < 18 {
        return None;
    }
    let state = rest[0].chars().next()?;
    let ppid = rest[1].parse().ok()?;
    let utime = rest[11].parse().ok()?;
    let stime = rest[12].parse().ok()?;
    let threads = rest[17].parse().ok()?;
    Some((name, state, ppid, utime, stime, threads))
}

fn parse_status(text: &str) -> (Option<u32>, Option<u64>, u64) {
    let mut uid = None;
    let mut rss = None;
    let mut wakeups = 0u64;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Uid:") {
            uid = value.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(value) = line.strip_prefix("VmRSS:") {
            rss = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|kb| kb * 1024);
        } else if let Some(value) = line.strip_prefix("voluntary_ctxt_switches:") {
            wakeups = value.trim().parse().unwrap_or(0);
        }
    }
    (uid, rss, wakeups)
}

fn parse_smaps_pss(text: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Pss:") {
            return value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|kb| kb * 1024);
        }
    }
    None
}

fn parse_io(text: &str) -> (u64, u64) {
    let mut read = 0;
    let mut write = 0;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("read_bytes:") {
            read = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("write_bytes:") {
            write = value.trim().parse().unwrap_or(0);
        }
    }
    (read, write)
}

fn cgroup_path(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.split_once("::").map(|(_, path)| path.trim().to_string()))
}

/// Per-process sampler that keeps the previous CPU counters.
#[derive(Debug)]
pub struct ProcessSampler {
    pub fs: ProcFs,
    previous: BTreeMap<u32, (u64, Instant)>,
    efficiency: HashSet<u32>,
    pub detailed: bool,
}

impl ProcessSampler {
    pub fn new(fs: ProcFs) -> Self {
        Self {
            fs,
            previous: BTreeMap::new(),
            efficiency: HashSet::new(),
            detailed: false,
        }
    }

    pub fn mark_efficiency(&mut self, pid: u32, enabled: bool) {
        if enabled {
            self.efficiency.insert(pid);
        } else {
            self.efficiency.remove(&pid);
        }
    }

    /// One sampling pass. CPU % is the delta since the previous pass.
    pub fn sample(&mut self) -> io::Result<Vec<ProcessRow>> {
        let now = Instant::now();
        let hz = ticks_per_second();
        let mut rows = Vec::new();
        let mut seen = HashSet::new();

        let entries = fs::read_dir(&self.fs.proc_root)?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(pid) = file_name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let dir = entry.path();
            let Ok(stat) = fs::read_to_string(dir.join("stat")) else {
                continue;
            };
            let Some((name, state, ppid, utime, stime, threads)) = parse_stat(&stat) else {
                continue;
            };
            let total_ticks = utime + stime;
            let cpu_percent = self
                .previous
                .get(&pid)
                .map(|(old_ticks, old_at)| {
                    let elapsed = now.saturating_duration_since(*old_at).as_secs_f64();
                    if elapsed <= 0.0 {
                        0.0
                    } else {
                        ((total_ticks.saturating_sub(*old_ticks)) as f64 / hz / elapsed * 100.0)
                            as f32
                    }
                })
                .unwrap_or(0.0);
            self.previous.insert(pid, (total_ticks, now));
            seen.insert(pid);

            let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
            let (uid, rss, wakeups) = parse_status(&status);
            let memory_bytes = if self.detailed {
                fs::read_to_string(dir.join("smaps_rollup"))
                    .ok()
                    .and_then(|text| parse_smaps_pss(&text))
                    .or(rss)
                    .unwrap_or(0)
            } else {
                rss.unwrap_or(0)
            };
            let (disk_read_bytes, disk_write_bytes) = fs::read_to_string(dir.join("io"))
                .map(|text| parse_io(&text))
                .unwrap_or((0, 0));
            let open_files = fs::read_dir(dir.join("fd"))
                .map(|entries| entries.flatten().count() as u32)
                .unwrap_or(0);
            let command = fs::read(dir.join("cmdline"))
                .map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .replace('\0', " ")
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|_| name.clone());
            let cgroup = fs::read_to_string(dir.join("cgroup"))
                .ok()
                .and_then(|text| cgroup_path(&text));

            rows.push(ProcessRow {
                pid,
                ppid,
                name,
                command,
                user: uid,
                threads,
                open_files,
                state,
                cpu_percent,
                memory_bytes,
                disk_read_bytes,
                disk_write_bytes,
                network_tx_bytes: 0,
                network_rx_bytes: 0,
                gpu_percent: None,
                energy: crate::model::EnergyImpact::classify(cpu_percent, wakeups, None),
                wakeups,
                efficiency: self.efficiency.contains(&pid),
                app_state: None,
                band: None,
                cgroup,
            });
        }

        self.previous.retain(|pid, _| seen.contains(pid));
        Ok(rows)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTicks {
    pub total: u64,
    pub idle: u64,
}

/// System-wide sampler with rate deltas between calls.
#[derive(Debug, Default)]
pub struct SystemSampler {
    last_disk: Option<(u64, u64, Instant)>,
    last_net: Option<(u64, u64, Instant)>,
    last_cpu: Option<(CpuTicks, Vec<CpuTicks>, Instant)>,
}

impl SystemSampler {
    pub fn sample(&mut self, fs: &ProcFs) -> SystemSample {
        let mut sample = SystemSample::default();

        if let Ok(stat) = fs::read_to_string(fs.proc_root.join("stat")) {
            let (total, per_core) = parse_cpu_ticks(&stat);
            let now = Instant::now();
            if let Some((old_total, old_cores, at)) = &self.last_cpu {
                let elapsed = now.saturating_duration_since(*at).as_secs_f64();
                if elapsed > 0.0 {
                    sample.cpu_total_percent = busy_percent(*old_total, total);
                    sample.per_core_percent = per_core
                        .iter()
                        .enumerate()
                        .map(|(index, core)| {
                            old_cores
                                .get(index)
                                .map(|old| busy_percent(*old, *core))
                                .unwrap_or(0.0)
                        })
                        .collect();
                }
            } else {
                sample.per_core_percent = vec![0.0; per_core.len()];
            }
            self.last_cpu = Some((total, per_core, now));
        }

        if let Ok(meminfo) = fs::read_to_string(fs.proc_root.join("meminfo")) {
            let values = parse_meminfo(&meminfo);
            sample.mem_total_bytes = values.get("MemTotal").copied().unwrap_or(0);
            sample.mem_available_bytes = values.get("MemAvailable").copied().unwrap_or(0);
            sample.mem_cached_bytes = values.get("Cached").copied().unwrap_or(0);
            sample.swap_total_bytes = values
                .get("SwapTotal")
                .copied()
                .unwrap_or(0);
            sample.swap_free_bytes = values.get("SwapFree").copied().unwrap_or(0);
        }

        if let Ok(uptime) = fs::read_to_string(fs.proc_root.join("uptime")) {
            sample.uptime_secs = uptime
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v as u64)
                .unwrap_or(0);
        }

        sample.process_count = fs::read_dir(&fs.proc_root)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        e.file_name()
                            .to_str()
                            .map(|name| name.chars().all(|c| c.is_ascii_digit()))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);

        if let Ok(net) = fs::read_to_string(fs.proc_root.join("net/dev")) {
            let (rx, tx) = parse_net_dev(&net);
            let now = Instant::now();
            if let Some((old_rx, old_tx, at)) = self.last_net {
                let elapsed = now.saturating_duration_since(at).as_secs_f64();
                if elapsed > 0.0 {
                    sample.network_rx_bytes_per_sec =
                        rx.saturating_sub(old_rx) as f64 / elapsed;
                    sample.network_tx_bytes_per_sec =
                        tx.saturating_sub(old_tx) as f64 / elapsed;
                }
            }
            self.last_net = Some((rx, tx, now));
        }

        if let Ok((read, write)) = read_disk_totals(&fs.sys_root) {
            let now = Instant::now();
            if let Some((old_read, old_write, at)) = self.last_disk {
                let elapsed = now.saturating_duration_since(at).as_secs_f64();
                if elapsed > 0.0 {
                    sample.disk_read_bytes_per_sec =
                        read.saturating_sub(old_read) as f64 / elapsed;
                    sample.disk_write_bytes_per_sec =
                        write.saturating_sub(old_write) as f64 / elapsed;
                }
            }
            self.last_disk = Some((read, write, now));
        }

        let sensors = read_sensors(&fs.sys_root);
        for sensor in sensors {
            match sensor.kind {
                SensorKind::TemperatureC => {
                    sample.temperatures_c.insert(sensor.name, sensor.value);
                }
                SensorKind::FanRpm => {
                    let rpm = sensor.value.max(0.0) as u32;
                    sample.fans_rpm.insert(sensor.name, rpm);
                }
            }
        }

        sample.gpu_busy_percent = read_gpu_busy(&fs.sys_root);
        sample.gpu_memory_used_bytes = read_gpu_memory(&fs.sys_root);
        sample.battery = read_battery_view(&fs.sys_root);
        sample.zram_used_bytes = read_zram_used(&fs.sys_root);
        sample.cpu_mhz = read_cpu_mhz(&fs.sys_root);
        sample.governor = read_governor(&fs.sys_root);
        sample.pressure_level = PsiReader::at(fs.proc_root.join("pressure/memory"))
            .sample()
            .ok()
            .map(|sample| {
                aether_lifecycled::pressure::PressureThresholds::default().classify(sample)
            });
        sample
    }
}

/// Parse the aggregate line and every per-core line into cumulative ticks.
pub fn parse_cpu_ticks(text: &str) -> (CpuTicks, Vec<CpuTicks>) {
    let mut total = CpuTicks::default();
    let mut per_core = Vec::new();
    for line in text.lines() {
        if !line.starts_with("cpu") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or("");
        let values: Vec<u64> = parts.filter_map(|v| v.parse().ok()).collect();
        if values.len() < 4 {
            continue;
        }
        let ticks = CpuTicks {
            total: values.iter().sum(),
            idle: values[3] + values.get(4).copied().unwrap_or(0),
        };
        if key == "cpu" {
            total = ticks;
        } else {
            per_core.push(ticks);
        }
    }
    (total, per_core)
}

/// Busy percentage between two cumulative samples.
pub fn busy_percent(previous: CpuTicks, current: CpuTicks) -> f32 {
    let total_delta = current.total.saturating_sub(previous.total) as f64;
    let idle_delta = current.idle.saturating_sub(previous.idle) as f64;
    if total_delta <= 0.0 {
        return 0.0;
    }
    (((total_delta - idle_delta) / total_delta) * 100.0).clamp(0.0, 100.0) as f32
}

/// meminfo in bytes.
pub fn parse_meminfo(text: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = value.split_whitespace();
        if let Some(number) = parts.next().and_then(|v| v.parse::<u64>().ok()) {
            let unit = parts.next().unwrap_or("B");
            let bytes = if unit.eq_ignore_ascii_case("kB") {
                number * 1024
            } else {
                number
            };
            map.insert(key.trim().to_string(), bytes);
        }
    }
    map
}

/// Sum rx/tx bytes across physical interfaces.
pub fn parse_net_dev(text: &str) -> (u64, u64) {
    let mut rx = 0;
    let mut tx = 0;
    for line in text.lines().skip(2) {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name == "lo" || name.starts_with("veth") || name.starts_with("docker") {
            continue;
        }
        let fields: Vec<u64> = values
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if fields.len() >= 9 {
            rx += fields[0];
            tx += fields[8];
        }
    }
    (rx, tx)
}

fn read_disk_totals(sys_root: &Path) -> io::Result<(u64, u64)> {
    let mut read = 0u64;
    let mut write = 0u64;
    for entry in fs::read_dir(sys_root.join("block"))?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("sd") || name.starts_with("nvme") || name.starts_with("vd")) {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let fields: Vec<u64> = text
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if fields.len() >= 7 {
            read += fields[2] * 512;
            write += fields[6] * 512;
        }
    }
    Ok((read, write))
}

/// hwmon + thermal sensors (TM-10).
pub fn read_sensors(sys_root: &Path) -> Vec<SensorReading> {
    let mut sensors = Vec::new();
    let hwmon = sys_root.join("class/hwmon");
    if let Ok(entries) = fs::read_dir(&hwmon) {
        for entry in entries.flatten() {
            let dir = entry.path();
            let chip = read_trimmed(&dir.join("name")).unwrap_or_else(|| "hwmon".into());
            if let Ok(files) = fs::read_dir(&dir) {
                for file in files.flatten() {
                    let name = file.file_name().to_string_lossy().into_owned();
                    if let Some(index) = temp_index(&name) {
                        if let Some(raw) = read_u64_file(&file.path()) {
                            let label = read_trimmed(&dir.join(format!("temp{index}_label")))
                                .unwrap_or_else(|| format!("temp{index}"));
                            sensors.push(SensorReading {
                                name: format!("{chip}/{label}"),
                                kind: SensorKind::TemperatureC,
                                value: raw as f32 / 1000.0,
                            });
                        }
                    } else if let Some(index) = fan_index(&name) {
                        if let Some(raw) = read_u64_file(&file.path()) {
                            let label = read_trimmed(&dir.join(format!("fan{index}_label")))
                                .unwrap_or_else(|| chip.clone());
                            sensors.push(SensorReading {
                                name: format!("{chip}/{label}"),
                                kind: SensorKind::FanRpm,
                                value: raw as f32,
                            });
                        }
                    }
                }
            }
        }
    }
    let thermal = sys_root.join("class/thermal");
    if let Ok(entries) = fs::read_dir(thermal) {
        for entry in entries.flatten() {
            let dir = entry.path();
            let Some(kind) = read_trimmed(&dir.join("type")) else {
                continue;
            };
            if let Some(raw) = read_u64_file(&dir.join("temp")) {
                sensors.push(SensorReading {
                    name: format!("thermal/{kind}"),
                    kind: SensorKind::TemperatureC,
                    value: raw as f32 / 1000.0,
                });
            }
        }
    }
    sensors
}

fn temp_index(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("temp")?.strip_suffix("_input")?;
    rest.parse().ok()
}

fn fan_index(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("fan")?.strip_suffix("_input")?;
    rest.parse().ok()
}

fn read_gpu_busy(sys_root: &Path) -> Option<f32> {
    let drm = sys_root.join("class/drm");
    let entries = fs::read_dir(drm).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("device/gpu_busy_percent");
        if let Some(value) = read_u64_file(&path) {
            return Some(value as f32);
        }
    }
    None
}

fn read_gpu_memory(sys_root: &Path) -> Option<u64> {
    let drm = sys_root.join("class/drm");
    for entry in fs::read_dir(drm).ok()?.flatten() {
        if let Some(value) =
            read_u64_file(&entry.path().join("device/mem_info_vram_used"))
        {
            return Some(value);
        }
    }
    None
}

fn read_battery_view(sys_root: &Path) -> Option<BatteryView> {
    let dir = sys_root.join("class/power_supply/BAT0");
    let percent = read_u64_file(&dir.join("capacity"))? as f32;
    let status = read_trimmed(&dir.join("status")).unwrap_or_default();
    let energy_full = read_u64_file(&dir.join("energy_full")).map(|v| v as f32 / 1e6);
    let design = read_u64_file(&dir.join("energy_full_design")).map(|v| v as f32 / 1e6);
    let power_w = read_u64_file(&dir.join("power_now")).map(|v| v as f32 / 1e6);
    Some(BatteryView {
        percent,
        charging: matches!(status.as_str(), "Charging" | "Full"),
        power_w,
        time_remaining_secs: None,
        cycle_count: read_u64_file(&dir.join("cycle_count")).map(|v| v as u32),
        health_percent: match (energy_full, design) {
            (Some(full), Some(design)) if design > 0.0 => Some(full / design * 100.0),
            _ => None,
        },
        temperature_c: read_u64_file(&dir.join("temp")).map(|v| v as f32 / 10.0),
    })
}

fn read_zram_used(sys_root: &Path) -> Option<u64> {
    let text = fs::read_to_string(sys_root.join("block/zram0/mm_stat")).ok()?;
    text.split_whitespace()
        .nth(2)
        .and_then(|v| v.parse().ok())
}

fn read_cpu_mhz(sys_root: &Path) -> Option<u32> {
    let text = fs::read_to_string(
        sys_root.join("devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"),
    )
    .ok()?;
    text.trim().parse::<u32>().ok().map(|khz| khz / 1000)
}

fn read_governor(sys_root: &Path) -> Option<String> {
    read_trimmed(&sys_root.join("devices/system/cpu/cpu0/cpufreq/scaling_governor"))
}

/// Pressure level helper for the UI.
pub fn pressure_label(level: Option<PressureLevel>) -> &'static str {
    match level {
        None => "Not reported",
        Some(PressureLevel::Normal) => "Normal",
        Some(PressureLevel::Elevated) => "Elevated",
        Some(PressureLevel::Critical) => "Critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "aether-tm-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(dir.join("proc/1234")).unwrap();
            fs::create_dir_all(dir.join("sys")).unwrap();
            Self { dir }
        }

        fn proc_root(&self) -> PathBuf {
            self.dir.join("proc")
        }

        fn sys_root(&self) -> PathBuf {
            self.dir.join("sys")
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.dir.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut file = fs::File::create(path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn parses_proc_stat_and_status() {
        let text = "1234 (aether browser) S 1 1234 1234 0 -1 4194560 100 0 0 0 50 25 0 0 20 0 12 0 100 0\n";
        let (name, state, ppid, utime, stime, threads) = parse_stat(text).unwrap();
        assert_eq!(name, "aether browser");
        assert_eq!(state, 'S');
        assert_eq!(ppid, 1);
        assert_eq!(utime, 50);
        assert_eq!(stime, 25);
        assert_eq!(threads, 12);

        let (uid, rss, wakeups) = parse_status(
            "Name:\tbrowser\nUid:\t1000\t1000\t1000\t1000\nVmRSS:\t  2048 kB\nvoluntary_ctxt_switches:\t42\n",
        );
        assert_eq!(uid, Some(1000));
        assert_eq!(rss, Some(2_097_152));
        assert_eq!(wakeups, 42);
    }

    #[test]
    fn samples_a_process_with_cpu_delta() {
        let fixture = Fixture::new();
        fixture.write(
            "proc/1234/stat",
            "1234 (idle) S 1 1234 1234 0 -1 0 0 0 0 0 100 0 0 0 20 0 1 0 0 0\n",
        );
        fixture.write("proc/1234/status", "Uid:\t1000\t1000\t1000\t1000\nVmRSS:\t1024 kB\n");
        fixture.write("proc/1234/cmdline", "idle\0--flag\0");
        fixture.write("proc/1234/io", "read_bytes: 2048\nwrite_bytes: 1024\n");
        let fs_view = ProcFs::at(fixture.proc_root());
        let mut sampler = ProcessSampler::new(fs_view);
        let first = sampler.sample().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name, "idle");
        assert_eq!(first[0].command, "idle --flag");
        assert_eq!(first[0].memory_bytes, 1_048_576);
        assert_eq!(first[0].disk_read_bytes, 2048);
        assert_eq!(first[0].user, Some(1000));
    }

    #[test]
    fn meminfo_is_converted_to_bytes() {
        let values = parse_meminfo("MemTotal:       16384000 kB\nMemAvailable:    8192000 kB\nCached:          2048000 kB\n");
        assert_eq!(values["MemTotal"], 16_384_000 * 1024);
        assert_eq!(values["MemAvailable"], 8_192_000 * 1024);
        assert_eq!(values["Cached"], 2_048_000 * 1024);
    }

    #[test]
    fn net_dev_skips_loopback_and_containers() {
        let text = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: 999 0 0 0 0 0 0 0 999 0 0 0 0 0 0 0\n  eth0: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0\n veth1: 50 0 0 0 0 0 0 0 60 0 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(text), (1000, 2000));
    }

    #[test]
    fn reads_hwmon_thermal_and_fans() {
        let fixture = Fixture::new();
        fixture.write("sys/class/hwmon/hwmon0/name", "coretemp\n");
        fixture.write("sys/class/hwmon/hwmon0/temp1_input", "45000\n");
        fixture.write("sys/class/hwmon/hwmon0/temp1_label", "Package id 0\n");
        fixture.write("sys/class/hwmon/hwmon0/fan1_input", "3200\n");
        fixture.write("sys/class/thermal/thermal_zone0/type", "x86_pkg_temp\n");
        fixture.write("sys/class/thermal/thermal_zone0/temp", "47000\n");
        let sensors = read_sensors(&fixture.sys_root());
        assert!(sensors.iter().any(|s| s.name == "coretemp/Package id 0"
            && (s.value - 45.0).abs() < 1e-4
            && s.kind == SensorKind::TemperatureC));
        assert!(sensors.iter().any(|s| s.kind == SensorKind::FanRpm && s.value == 3200.0));
        assert!(sensors
            .iter()
            .any(|s| s.name == "thermal/x86_pkg_temp" && (s.value - 47.0).abs() < 1e-4));
    }

    #[test]
    fn reads_gpu_busy_and_memory() {
        let fixture = Fixture::new();
        fixture.write("sys/class/drm/card0/device/gpu_busy_percent", "42\n");
        fixture.write("sys/class/drm/card0/device/mem_info_vram_used", "1048576\n");
        assert_eq!(read_gpu_busy(&fixture.sys_root()), Some(42.0));
        assert_eq!(read_gpu_memory(&fixture.sys_root()), Some(1_048_576));
    }

    #[test]
    fn reads_battery_view() {
        let fixture = Fixture::new();
        fixture.write("sys/class/power_supply/BAT0/capacity", "79\n");
        fixture.write("sys/class/power_supply/BAT0/status", "Discharging\n");
        fixture.write("sys/class/power_supply/BAT0/energy_full", "53200000\n");
        fixture.write("sys/class/power_supply/BAT0/energy_full_design", "60000000\n");
        fixture.write("sys/class/power_supply/BAT0/power_now", "9800000\n");
        fixture.write("sys/class/power_supply/BAT0/cycle_count", "120\n");
        fixture.write("sys/class/power_supply/BAT0/temp", "305\n");
        let battery = read_battery_view(&fixture.sys_root()).unwrap();
        assert_eq!(battery.percent, 79.0);
        assert_eq!(battery.cycle_count, Some(120));
        assert!((battery.power_w.unwrap() - 9.8).abs() < 1e-4);
        assert!((battery.health_percent.unwrap() - 88.6667).abs() < 0.01);
        assert!((battery.temperature_c.unwrap() - 30.5).abs() < 1e-3);
    }

    #[test]
    fn cpu_percent_uses_idle_delta() {
        let first = CpuTicks { total: 1000, idle: 800 };
        let second = CpuTicks { total: 1200, idle: 850 };
        // 200 total ticks, 50 idle -> 75% busy.
        assert!((busy_percent(first, second) - 75.0).abs() < 1e-4);
        let (total, cores) = parse_cpu_ticks(
            "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0 0 0\n",
        );
        assert_eq!(total.total, 1000);
        assert_eq!(cores.len(), 2);
        assert_eq!(cores[0].idle, 400);
    }

    #[test]
    fn pressure_label_covers_all_states() {
        assert_eq!(pressure_label(None), "Not reported");
        assert_eq!(pressure_label(Some(PressureLevel::Normal)), "Normal");
        assert_eq!(pressure_label(Some(PressureLevel::Elevated)), "Elevated");
        assert_eq!(pressure_label(Some(PressureLevel::Critical)), "Critical");
    }
}
