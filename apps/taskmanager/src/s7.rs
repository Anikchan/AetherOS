//! Spike S7 — sensor and GPU readings (start of P5-T07).
//!
//! Answers the two questions the Activity Manager's Sensors and GPU sections
//! depend on:
//! 1. Do hwmon/thermal sensors report temperatures and fan speeds?
//! 2. Does the GPU driver expose engine busy time and VRAM use?
//!
//! Findings are recorded as Pass / Partial / Fail, with the fallback the UI
//! uses when a reading is missing. Nothing is invented: absent paths are
//! reported as "Not reported by this system".

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::collect::{read_sensors, ProcFs};
use crate::model::SensorReading;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpikeVerdict {
    Pass,
    Partial,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuProbe {
    pub card: Option<String>,
    pub busy_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub driver: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpikeS7Report {
    pub sys_root: PathBuf,
    pub hwmon_chips: Vec<String>,
    pub thermal_zones: Vec<String>,
    pub sensors: Vec<SensorReading>,
    pub gpu: GpuProbe,
    pub per_process_gpu_source: Option<String>,
    pub verdict: SpikeVerdict,
    pub notes: Vec<String>,
}

/// Run the probe against a sysfs root (`/sys` on AetherOS).
pub fn run(sys_root: impl AsRef<Path>) -> SpikeS7Report {
    let sys_root = sys_root.as_ref();
    let mut notes = Vec::new();

    let mut hwmon_chips = Vec::new();
    if let Ok(entries) = fs::read_dir(sys_root.join("class/hwmon")) {
        for entry in entries.flatten() {
            if let Ok(name) = fs::read_to_string(entry.path().join("name")) {
                hwmon_chips.push(name.trim().to_string());
            }
        }
    }
    hwmon_chips.sort();

    let mut thermal_zones = Vec::new();
    if let Ok(entries) = fs::read_dir(sys_root.join("class/thermal")) {
        for entry in entries.flatten() {
            if let Ok(kind) = fs::read_to_string(entry.path().join("type")) {
                thermal_zones.push(kind.trim().to_string());
            }
        }
    }
    thermal_zones.sort();

    let sensors = read_sensors(sys_root);
    let temperatures = sensors
        .iter()
        .filter(|sensor| sensor.kind == crate::model::SensorKind::TemperatureC)
        .count();
    let fans = sensors
        .iter()
        .filter(|sensor| sensor.kind == crate::model::SensorKind::FanRpm)
        .count();

    let gpu = probe_gpu(sys_root);
    let per_process_gpu_source = if gpu.busy_percent.is_some() {
        Some("/proc/<pid>/fdinfo DRM client usage (Pack D)".to_string())
    } else {
        None
    };

    if temperatures == 0 {
        notes.push(
            "No temperature sensor found: Sensors tab shows 'Not reported by this system' and alerts are disabled."
                .into(),
        );
    }
    if fans == 0 {
        notes.push("No fan sensor found: fan speed is hidden, not shown as zero.".into());
    }
    if gpu.busy_percent.is_none() {
        notes.push(
            "GPU engine busy time not exposed: GPU section shows 'Not reported'; per-process GPU stays in Pack D."
                .into(),
        );
    }
    if gpu.memory_used_bytes.is_none() {
        notes.push("GPU memory usage not exposed by this driver.".into());
    }

    let verdict = match (temperatures > 0, gpu.busy_percent.is_some()) {
        (true, true) => SpikeVerdict::Pass,
        (true, false) | (false, true) => SpikeVerdict::Partial,
        (false, false) => SpikeVerdict::Fail,
    };

    let per_process_gpu_source = if per_process_gpu_source.is_some() && gpu.busy_percent.is_none() {
        None
    } else {
        per_process_gpu_source
    };

    SpikeS7Report {
        sys_root: sys_root.to_path_buf(),
        hwmon_chips,
        thermal_zones,
        sensors,
        gpu,
        per_process_gpu_source,
        verdict,
        notes,
    }
}

fn probe_gpu(sys_root: &Path) -> GpuProbe {
    let mut probe = GpuProbe {
        card: None,
        busy_percent: None,
        memory_used_bytes: None,
        driver: None,
    };
    let Ok(entries) = fs::read_dir(sys_root.join("class/drm")) else {
        return probe;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") {
            continue;
        }
        let device = entry.path().join("device");
        let busy = fs::read_to_string(device.join("gpu_busy_percent"))
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok());
        let memory = fs::read_to_string(device.join("mem_info_vram_used"))
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok());
        if busy.is_some() || memory.is_some() {
            probe.card = Some(name);
            probe.busy_percent = busy;
            probe.memory_used_bytes = memory;
            probe.driver = fs::read_link(device.join("driver"))
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                });
            break;
        }
    }
    probe
}

/// D6-format markdown for the spike report.
pub fn to_markdown(report: &SpikeS7Report) -> String {
    let mut out = String::new();
    out.push_str("# Spike S7 — sensor and GPU readings (P5-T07)\n\n");
    out.push_str(&format!("- Date: {}\n", epoch_day()));
    out.push_str(&format!("- Probe root: `{}`\n", report.sys_root.display()));
    out.push_str(&format!(
        "- Verdict: **{:?}** (Pass = sensors + GPU busy, Partial = one of them, Fail = neither)\n\n",
        report.verdict
    ));
    out.push_str("## Sensors\n\n");
    out.push_str(&format!(
        "- hwmon chips: {}\n- thermal zones: {}\n- readings: {}\n\n",
        if report.hwmon_chips.is_empty() {
            "none".to_string()
        } else {
            report.hwmon_chips.join(", ")
        },
        if report.thermal_zones.is_empty() {
            "none".to_string()
        } else {
            report.thermal_zones.join(", ")
        },
        report.sensors.len()
    ));
    out.push_str("| Sensor | Kind | Value |\n|---|---|---|\n");
    for sensor in &report.sensors {
        out.push_str(&format!(
            "| {} | {:?} | {:.1} |\n",
            sensor.name, sensor.kind, sensor.value
        ));
    }
    out.push_str("\n## GPU\n\n");
    out.push_str(&format!(
        "| Field | Value |\n|---|---|\n| Card | {} |\n| Busy | {} |\n| VRAM used | {} |\n| Driver | {} |\n\n",
        report.gpu.card.clone().unwrap_or_else(|| "Not reported".into()),
        report
            .gpu
            .busy_percent
            .map(|v| format!("{v:.1}%"))
            .unwrap_or_else(|| "Not reported".into()),
        report
            .gpu
            .memory_used_bytes
            .map(|v| format!("{v} bytes"))
            .unwrap_or_else(|| "Not reported".into()),
        report.gpu.driver.clone().unwrap_or_else(|| "unknown".into()),
    ));
    if let Some(source) = &report.per_process_gpu_source {
        out.push_str(&format!("Per-process GPU source: {source}\n\n"));
    }
    out.push_str("## Notes and fallbacks\n\n");
    if report.notes.is_empty() {
        out.push_str("- All readings available.\n");
    } else {
        for note in &report.notes {
            out.push_str(&format!("- {note}\n"));
        }
    }
    out
}

fn epoch_day() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{} (epoch day {})", secs, secs / 86_400)
}

/// Convenience for the CLI: probe the running system.
pub fn run_system() -> SpikeS7Report {
    run(ProcFs::system().sys_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-s7-{tag}-{}-{}-{}",
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

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn full_sensors_and_gpu_pass() {
        let dir = fixture("pass");
        write(&dir.join("class/hwmon/hwmon0/name"), "coretemp\n");
        write(&dir.join("class/hwmon/hwmon0/temp1_input"), "45000\n");
        write(&dir.join("class/thermal/thermal_zone0/type"), "x86_pkg_temp\n");
        write(&dir.join("class/thermal/thermal_zone0/temp"), "47000\n");
        write(&dir.join("class/drm/card0/device/gpu_busy_percent"), "12\n");
        write(&dir.join("class/drm/card0/device/mem_info_vram_used"), "1048576\n");
        let report = run(&dir);
        assert_eq!(report.verdict, SpikeVerdict::Pass);
        assert!(report.gpu.busy_percent.is_some());
        assert!(report.per_process_gpu_source.is_some());
        let markdown = to_markdown(&report);
        assert!(markdown.contains("Verdict: **Pass**"));
        assert!(markdown.contains("coretemp"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sensors_without_gpu_are_partial() {
        let dir = fixture("partial");
        write(&dir.join("class/hwmon/hwmon0/name"), "acpitz\n");
        write(&dir.join("class/hwmon/hwmon0/temp1_input"), "38000\n");
        let report = run(&dir);
        assert_eq!(report.verdict, SpikeVerdict::Partial);
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("GPU engine busy time not exposed")));
        assert!(report.per_process_gpu_source.is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn no_sensors_and_no_gpu_fail() {
        let dir = fixture("fail");
        let report = run(&dir);
        assert_eq!(report.verdict, SpikeVerdict::Fail);
        let markdown = to_markdown(&report);
        assert!(markdown.contains("Not reported"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fan_sensor_is_recognised() {
        let dir = fixture("fan");
        write(&dir.join("class/hwmon/hwmon0/name"), "thinkpad\n");
        write(&dir.join("class/hwmon/hwmon0/fan1_input"), "2800\n");
        let report = run(&dir);
        assert!(report
            .sensors
            .iter()
            .any(|sensor| sensor.kind == crate::model::SensorKind::FanRpm));
        let _ = fs::remove_dir_all(dir);
    }
}
