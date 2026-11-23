//! Battery telemetry (P5-T11 input, D5 section 2).
//!
//! UPower is the primary source (ADR-014); sysfs is the fallback when UPower
//! is not running (D5 section 10). Both are behind the same parser so the
//! values can be tested without hardware.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::types::{BatteryStatus, BatteryTelemetry, PowerError, PowerSource};

/// Sysfs battery reader over one `BAT*` directory.
#[derive(Debug, Clone)]
pub struct SysfsBattery {
    dir: PathBuf,
    ac_dir: Option<PathBuf>,
}

impl SysfsBattery {
    pub fn system() -> Self {
        Self::at("/sys/class/power_supply/BAT0")
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ac_dir: Some(PathBuf::from("/sys/class/power_supply/AC")),
        }
    }

    pub fn with_ac(mut self, ac_dir: impl Into<PathBuf>) -> Self {
        self.ac_dir = Some(ac_dir.into());
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read_value(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.dir.join(name))
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn read_f32(&self, name: &str) -> Option<f32> {
        self.read_value(name).and_then(|v| v.parse().ok())
    }

    fn read_u32(&self, name: &str) -> Option<u32> {
        self.read_value(name).and_then(|v| v.parse().ok())
    }

    fn ac_online(&self) -> bool {
        self.ac_dir
            .as_ref()
            .and_then(|dir| fs::read_to_string(dir.join("online")).ok())
            .map(|v| v.trim() == "1")
            .unwrap_or(false)
    }
}

impl BatteryTelemetry for SysfsBattery {
    fn read(&self) -> Result<BatteryStatus, PowerError> {
        if !self.dir.exists() {
            return Err(PowerError::Unsupported(format!(
                "no battery at {}",
                self.dir.display()
            )));
        }
        let status = self.read_value("status").unwrap_or_else(|| "Unknown".into());
        let charging = matches!(status.as_str(), "Charging" | "Full");
        let on_ac = self.ac_online() || matches!(status.as_str(), "Charging" | "Full");

        // Capacity: `capacity` is a percentage; otherwise derive from energy.
        let energy_now_wh = power_supply_wh(self, "energy_now", "charge_now");
        let energy_full_wh = power_supply_wh(self, "energy_full", "charge_full");
        let design_wh = power_supply_wh(self, "energy_full_design", "charge_full_design");
        let percent = match self.read_f32("capacity") {
            Some(value) => value,
            None => match (energy_now_wh, energy_full_wh) {
                (Some(now), Some(full)) if full > 0.0 => now / full * 100.0,
                _ => {
                    return Err(PowerError::Unsupported(
                        "battery reports neither capacity nor energy".into(),
                    ))
                }
            },
        };

        // Power: `power_now` (µW) or current × voltage.
        let energy_rate_w = self.read_f32("power_now").map(|uw| uw / 1_000_000.0).or_else(|| {
            match (self.read_f32("current_now"), self.read_f32("voltage_now")) {
                (Some(current_ua), Some(voltage_uv)) => {
                    Some((current_ua * voltage_uv) / 1_000_000_000_000.0)
                }
                _ => None,
            }
        });

        let time_to_empty = energy_rate_w
            .filter(|rate| *rate > 0.1 && !charging)
            .and_then(|rate| energy_now_wh.map(|energy| Duration::from_secs_f32(energy / rate * 3600.0)));
        let time_to_full = energy_rate_w
            .filter(|rate| *rate > 0.1 && charging)
            .and_then(|rate| {
                match (energy_now_wh, energy_full_wh) {
                    (Some(now), Some(full)) if full > now => {
                        Some(Duration::from_secs_f32((full - now) / rate * 3600.0))
                    }
                    _ => None,
                }
            });

        let health_percent = match (energy_full_wh, design_wh) {
            (Some(full), Some(design)) if design > 0.0 => Some(full / design * 100.0),
            _ => self.read_f32("capacity_level").map(|_| 0.0),
        };

        Ok(BatteryStatus {
            percent,
            source: if on_ac {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
            charging,
            energy_rate_w,
            time_to_empty,
            time_to_full,
            health_percent,
            cycle_count: self.read_u32("cycle_count"),
            energy_now_wh,
            energy_full_wh,
            design_wh,
            temperature_c: self.read_f32("temp").map(|tenths| tenths / 10.0),
        })
    }
}

/// `energy_*` is in µWh; `charge_*` is in µAh and needs voltage (volts).
fn power_supply_wh(battery: &SysfsBattery, energy_file: &str, charge_file: &str) -> Option<f32> {
    if let Some(energy) = battery.read_f32(energy_file) {
        return Some(energy / 1_000_000.0);
    }
    let charge = battery.read_f32(charge_file)?;
    let voltage = battery.read_f32("voltage_now")? / 1_000_000.0;
    Some(charge / 1_000_000.0 * voltage)
}

/// UPower command-line probe. UPower itself is the system service; this only
/// reads its public `upower -i` output, so no D-Bus dependency is needed.
#[derive(Debug, Clone)]
pub struct UpowerCli {
    device: String,
    binary: PathBuf,
}

impl UpowerCli {
    pub fn system() -> Self {
        Self {
            device: "battery_BAT0".into(),
            binary: PathBuf::from("upower"),
        }
    }

    pub fn at(device: impl Into<String>, binary: impl Into<PathBuf>) -> Self {
        Self {
            device: device.into(),
            binary: binary.into(),
        }
    }

    pub fn read_text(&self) -> Result<String, PowerError> {
        let output = Command::new(&self.binary)
            .args(["-i", &format!("/org/freedesktop/UPower/devices/{}", self.device)])
            .output()
            .map_err(|e| PowerError::Backend(format!("upower unavailable: {e}")))?;
        if !output.status.success() {
            return Err(PowerError::Backend("upower -i failed".into()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn parse(&self, text: &str) -> Result<BatteryStatus, PowerError> {
        parse_upower(text)
    }
}

impl BatteryTelemetry for UpowerCli {
    fn read(&self) -> Result<BatteryStatus, PowerError> {
        self.parse(&self.read_text()?)
    }
}

/// Parse `upower -i` battery output.
pub fn parse_upower(text: &str) -> Result<BatteryStatus, PowerError> {
    let mut map: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if !key.is_empty() && !value.is_empty() {
                map.push((key, value));
            }
        }
    }
    let get = |name: &str| {
        map.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let percent = get("percentage")
        .and_then(|v| v.trim_end_matches('%').trim().parse::<f32>().ok())
        .ok_or_else(|| PowerError::Backend("upower output has no percentage".into()))?;
    let state = get("state").unwrap_or_else(|| "unknown".into());
    let charging = matches!(state.as_str(), "charging" | "fully-charged");
    let source = if charging { PowerSource::Ac } else { PowerSource::Battery };
    let number = |value: Option<String>| {
        value.and_then(|v| {
            v.split_whitespace()
                .next()
                .map(|n| n.trim_end_matches('%').parse::<f32>().ok())
                .flatten()
        })
    };
    let energy_rate_w = number(get("energy-rate"));
    let energy_now_wh = number(get("energy"));
    let energy_full_wh = number(get("energy-full"));
    let design_wh = number(get("energy-full-design"));
    let health_percent = number(get("capacity"));
    let cycle_count = number(get("charge-cycles"))
        .or_else(|| number(get("cycle-count")))
        .map(|n| n as u32);
    let temperature_c = number(get("temperature"));
    let time_to_empty = get("time to empty").and_then(|v| parse_duration_minutes(&v));
    let time_to_full = get("time to full").and_then(|v| parse_duration_minutes(&v));

    Ok(BatteryStatus {
        percent,
        source,
        charging,
        energy_rate_w,
        time_to_empty,
        time_to_full,
        health_percent,
        cycle_count,
        energy_now_wh,
        energy_full_wh,
        design_wh,
        temperature_c,
    })
}

fn parse_duration_minutes(text: &str) -> Option<Duration> {
    let mut parts = text.split_whitespace();
    let value: f32 = parts.next()?.parse().ok()?;
    let unit = parts.next()?.trim();
    let seconds = if unit.starts_with("minute") {
        value * 60.0
    } else if unit.starts_with("hour") {
        value * 3600.0
    } else if unit.starts_with("second") {
        value
    } else {
        return None;
    };
    Some(Duration::from_secs_f32(seconds))
}

/// A battery device that prefers UPower and falls back to sysfs.
pub struct BatteryReader {
    upower: Option<UpowerCli>,
    sysfs: SysfsBattery,
    last_source: std::sync::Mutex<Option<PowerSource>>,
}

impl BatteryReader {
    pub fn system() -> Self {
        Self {
            upower: Some(UpowerCli::system()),
            sysfs: SysfsBattery::system(),
            last_source: std::sync::Mutex::new(None),
        }
    }

    pub fn sysfs_only(sysfs: SysfsBattery) -> Self {
        Self {
            upower: None,
            sysfs,
            last_source: std::sync::Mutex::new(None),
        }
    }

    pub fn sysfs(&self) -> &SysfsBattery {
        &self.sysfs
    }
}

impl BatteryTelemetry for BatteryReader {
    fn read(&self) -> Result<BatteryStatus, PowerError> {
        let mut status = match &self.upower {
            Some(upower) => upower.read().or_else(|_| self.sysfs.read())?,
            None => self.sysfs.read()?,
        };
        // Keep a stable source when the backends disagree (sysfs is the
        // kernel truth for AC state).
        if let Ok(sysfs) = self.sysfs.read() {
            status.source = sysfs.source;
            if status.energy_rate_w.is_none() {
                status.energy_rate_w = sysfs.energy_rate_w;
            }
            if status.temperature_c.is_none() {
                status.temperature_c = sysfs.temperature_c;
            }
        }
        *self.last_source.lock().unwrap() = Some(status.source);
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-tele-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, value: &str) {
        fs::write(path, value).unwrap();
    }

    #[test]
    fn sysfs_energy_battery_is_read() {
        let dir = temp_dir("energy");
        write(&dir.join("capacity"), "79\n");
        write(&dir.join("status"), "Discharging\n");
        write(&dir.join("energy_now"), "42500000\n");
        write(&dir.join("energy_full"), "53200000\n");
        write(&dir.join("energy_full_design"), "60000000\n");
        write(&dir.join("power_now"), "9800000\n");
        write(&dir.join("cycle_count"), "120\n");
        write(&dir.join("temp"), "305\n");
        let battery = SysfsBattery::at(&dir);
        let status = battery.read().unwrap();
        assert_eq!(status.percent, 79.0);
        assert_eq!(status.source, PowerSource::Battery);
        assert!(!status.charging);
        assert!((status.energy_rate_w.unwrap() - 9.8).abs() < 1e-4);
        assert!((status.energy_now_wh.unwrap() - 42.5).abs() < 1e-3);
        assert!((status.health_percent.unwrap() - 88.6667).abs() < 0.01);
        assert_eq!(status.cycle_count, Some(120));
        assert!((status.temperature_c.unwrap() - 30.5).abs() < 1e-3);
        let time = status.time_to_empty.unwrap();
        assert!((time.as_secs_f32() - 42.5 / 9.8 * 3600.0).abs() < 2.0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sysfs_charge_battery_uses_voltage() {
        let dir = temp_dir("charge");
        write(&dir.join("status"), "Charging\n");
        write(&dir.join("charge_now"), "2000000\n");
        write(&dir.join("charge_full"), "4000000\n");
        write(&dir.join("charge_full_design"), "5000000\n");
        write(&dir.join("voltage_now"), "12000000\n");
        write(&dir.join("current_now"), "500000\n");
        let battery = SysfsBattery::at(&dir);
        let status = battery.read().unwrap();
        assert_eq!(status.percent, 50.0);
        assert_eq!(status.source, PowerSource::Ac);
        assert!(status.charging);
        assert!((status.energy_rate_w.unwrap() - 6.0).abs() < 1e-4);
        assert!((status.energy_now_wh.unwrap() - 24.0).abs() < 1e-4);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_upower_output() {
        let text = r#"  native-path:          BAT0
  power supply:         yes
  updated:              Mon 23 Nov 2026 09:15:00 AM IST (12 seconds ago)
  has history:          yes
  battery
    present:             yes
    state:               discharging
    energy:              42.5 Wh
    energy-full:         53.2 Wh
    energy-full-design:  60 Wh
    energy-rate:         9.8 W
    voltage:             11.4 V
    charge-cycles:       120
    percentage:          79%
    capacity:            88.6667%
    time to empty:       4.3 hours
    temperature:         30.5 degrees C
"#;
        let status = parse_upower(text).unwrap();
        assert_eq!(status.percent, 79.0);
        assert_eq!(status.cycle_count, Some(120));
        assert!((status.energy_rate_w.unwrap() - 9.8).abs() < 1e-3);
        assert!((status.health_percent.unwrap() - 88.6667).abs() < 0.01);
        assert_eq!(
            status.time_to_empty.unwrap().as_secs(),
            (4.3f32 * 3600.0) as u64
        );
    }

    #[test]
    fn missing_upower_output_is_an_error() {
        assert!(parse_upower("nothing here").is_err());
    }

    #[test]
    fn missing_battery_is_unsupported() {
        let battery = SysfsBattery::at("/definitely/not/a/battery");
        assert!(matches!(battery.read(), Err(PowerError::Unsupported(_))));
    }
}
