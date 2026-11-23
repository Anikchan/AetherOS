//! Battery analytics store (P5-T16, F14 section 2.1, BAT-01..BAT-05/08/09).
//!
//! Everything is local. Missing hardware values are reported as
//! "Not reported by this battery"; derived values are labelled "Estimate"
//! (F14 section 4 honesty rules).

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::{BatteryStatus, PowerSource};

/// Seconds in one local day bucket (UTC day; the UI may offset for display).
pub const DAY_SECS: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatterySample {
    pub at: u64,
    pub percent: f32,
    pub charging: bool,
    pub source: PowerSource,
    pub rate_w: Option<f32>,
    pub temp_c: Option<f32>,
    pub health_pct: Option<f32>,
    pub cycle_count: Option<u32>,
    pub energy_now_wh: Option<f32>,
    pub energy_full_wh: Option<f32>,
}

impl BatterySample {
    pub fn from_status(status: &BatteryStatus, at: u64) -> Self {
        Self {
            at,
            percent: status.percent,
            charging: status.charging,
            source: status.source,
            rate_w: status.energy_rate_w,
            temp_c: status.temperature_c,
            health_pct: status.health_percent,
            cycle_count: status.cycle_count,
            energy_now_wh: status.energy_now_wh,
            energy_full_wh: status.energy_full_wh,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Discharge,
    Charge,
}

/// One charge or discharge session (BAT-04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub kind: SessionKind,
    pub start: u64,
    pub end: u64,
    pub start_pct: f32,
    pub end_pct: f32,
    pub energy_wh: f64,
    pub avg_power_w: f64,
}

/// Daily aggregation (BAT-02, BAT-05, BAT-06, BAT-09).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DailyBattery {
    pub day: i64,
    pub health_pct: Option<f32>,
    pub temp_min: Option<f32>,
    pub temp_max: Option<f32>,
    pub temp_sum: f32,
    pub temp_count: u32,
    pub standby_wh: f64,
    pub standby_percent: f32,
    pub minutes_above_80: u32,
    pub charged_to_full: u32,
}

impl DailyBattery {
    pub fn temp_avg(&self) -> Option<f32> {
        if self.temp_count == 0 {
            None
        } else {
            Some(self.temp_sum / self.temp_count as f32)
        }
    }
}

/// Per-app energy entry (BAT-08).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppEnergy {
    pub at: u64,
    pub app: String,
    pub energy_wh: f64,
}

/// Battery history store, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatteryAnalytics {
    pub samples: Vec<BatterySample>,
    pub sessions: Vec<Session>,
    pub daily: BTreeMap<i64, DailyBattery>,
    #[serde(default)]
    pub app_energy: VecDeque<AppEnergy>,
    #[serde(default)]
    discharged_wh_total: f64,
    #[serde(skip)]
    last: Option<BatterySample>,
    #[serde(skip)]
    open_session: Option<Session>,
    #[serde(default)]
    max_samples: usize,
}

impl BatteryAnalytics {
    pub fn new(max_samples: usize) -> Self {
        Self {
            max_samples: max_samples.max(1),
            ..Default::default()
        }
    }

    /// Record one sample plus the screen state (for standby drain).
    pub fn observe(&mut self, sample: BatterySample, screen_on: bool) {
        let day = (sample.at / DAY_SECS) as i64;

        // Session and energy accounting first: it needs `&mut self` but not
        // the daily map, so the two borrows never overlap.
        let mut minutes_above_80 = 0u32;
        let mut standby_wh = 0.0f64;
        let mut standby_percent = 0.0f32;
        let mut charged_to_full = 0u32;

        if let Some(previous) = self.last.clone() {
            let dt = sample.at.saturating_sub(previous.at).min(300) as f64;
            if dt > 0.0 {
                if sample.percent > 80.0 {
                    minutes_above_80 = (dt / 60.0).round() as u32;
                }
                if sample.source == PowerSource::Battery && !sample.charging {
                    let power = sample
                        .rate_w
                        .map(|p| p as f64)
                        .or_else(|| match (previous.energy_now_wh, sample.energy_now_wh) {
                            (Some(before), Some(after)) if after < before => {
                                Some((before - after) as f64 / (dt / 3600.0))
                            }
                            _ => None,
                        })
                        .unwrap_or(0.0);
                    let energy = power * dt / 3600.0;
                    self.discharged_wh_total += energy;
                    self.accumulate_session(&previous, &sample, energy);
                    if !screen_on {
                        standby_wh = energy;
                        standby_percent = (previous.percent - sample.percent).max(0.0);
                    }
                } else if sample.charging {
                    self.accumulate_session(&previous, &sample, 0.0);
                    if sample.percent >= 99.5 && previous.percent < 99.5 {
                        charged_to_full = 1;
                    }
                } else {
                    self.close_session(sample.at);
                }
            }
        }

        let daily = self.daily.entry(day).or_insert_with(|| DailyBattery {
            day,
            ..Default::default()
        });
        if let Some(health) = sample.health_pct {
            // One health point per day (BAT-02).
            daily.health_pct.get_or_insert(health);
        }
        if let Some(temp) = sample.temp_c {
            daily.temp_min = Some(daily.temp_min.map_or(temp, |v| v.min(temp)));
            daily.temp_max = Some(daily.temp_max.map_or(temp, |v| v.max(temp)));
            daily.temp_sum += temp;
            daily.temp_count += 1;
        }
        daily.minutes_above_80 += minutes_above_80;
        daily.standby_wh += standby_wh;
        daily.standby_percent += standby_percent;
        daily.charged_to_full += charged_to_full;

        self.last = Some(sample.clone());
        self.samples.push(sample);
        if self.samples.len() > self.max_samples {
            let excess = self.samples.len() - self.max_samples;
            self.samples.drain(0..excess);
        }
    }

    fn accumulate_session(&mut self, previous: &BatterySample, sample: &BatterySample, energy_wh: f64) {
        let kind = if sample.charging {
            SessionKind::Charge
        } else {
            SessionKind::Discharge
        };
        match self.open_session.as_mut() {
            Some(session) if session.kind == kind => {
                session.end = sample.at;
                session.end_pct = sample.percent;
                session.energy_wh += energy_wh;
                let seconds = session.end.saturating_sub(session.start).max(1) as f64;
                session.avg_power_w = session.energy_wh / (seconds / 3600.0);
            }
            _ => {
                self.close_session(previous.at);
                self.open_session = Some(Session {
                    kind,
                    start: previous.at,
                    end: sample.at,
                    start_pct: previous.percent,
                    end_pct: sample.percent,
                    energy_wh,
                    avg_power_w: 0.0,
                });
            }
        }
    }

    fn close_session(&mut self, at: u64) {
        if let Some(mut session) = self.open_session.take() {
            session.end = at;
            let seconds = session.end.saturating_sub(session.start).max(1) as f64;
            session.avg_power_w = session.energy_wh / (seconds / 3600.0);
            if session.end > session.start {
                self.sessions.push(session);
            }
        }
    }

    /// Close an open session (daemon shutdown or display off).
    pub fn finish(&mut self) {
        if let Some(last) = &self.last {
            let at = last.at;
            self.close_session(at);
        }
    }

    /// Add energy used by one app (D10 sampling; 1-minute to 1-day rollup).
    pub fn record_app_energy(&mut self, at: u64, app: impl Into<String>, energy_wh: f64) {
        self.app_energy.push_back(AppEnergy {
            at,
            app: app.into(),
            energy_wh,
        });
        let cutoff = at.saturating_sub(7 * DAY_SECS);
        while self
            .app_energy
            .front()
            .map(|entry| entry.at < cutoff)
            .unwrap_or(false)
        {
            self.app_energy.pop_front();
        }
    }

    pub fn cycle_count(&self) -> Option<u32> {
        self.samples.iter().rev().find_map(|s| s.cycle_count)
    }

    pub fn health_percent(&self) -> Option<f32> {
        self.samples.iter().rev().find_map(|s| s.health_pct)
    }

    /// BAT-03: total discharged energy divided by full capacity, labelled
    /// "Estimate" by the caller.
    pub fn estimated_cycles(&self) -> Option<f64> {
        let full = self
            .samples
            .iter()
            .rev()
            .find_map(|s| s.energy_full_wh)
            .filter(|full| *full > 0.0)?;
        Some(self.discharged_wh_total / full as f64)
    }

    /// One health point per day (BAT-02), oldest first.
    pub fn health_trend(&self) -> Vec<(i64, f32)> {
        self.daily
            .values()
            .filter_map(|d| d.health_pct.map(|h| (d.day, h)))
            .collect()
    }

    pub fn temperature_history(&self) -> Vec<(i64, f32, f32, f32)> {
        self.daily
            .values()
            .filter_map(|d| {
                Some((
                    d.day,
                    d.temp_min?,
                    d.temp_max?,
                    d.temp_avg()?,
                ))
            })
            .collect()
    }

    pub fn standby_drain(&self, day: i64) -> Option<&DailyBattery> {
        self.daily.get(&day)
    }

    /// App energy over the last `days`, highest first (BAT-08).
    pub fn energy_by_app(&self, now: u64, days: u64) -> Vec<(String, f64)> {
        let cutoff = now.saturating_sub(days * DAY_SECS);
        let mut totals: BTreeMap<String, f64> = BTreeMap::new();
        for entry in &self.app_energy {
            if entry.at >= cutoff {
                *totals.entry(entry.app.clone()).or_insert(0.0) += entry.energy_wh;
            }
        }
        let mut rows: Vec<(String, f64)> = totals.into_iter().collect();
        rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        rows
    }

    /// Text report for export (BAT-10 style); missing values are explicit.
    pub fn report(&self) -> String {
        let mut out = String::from("AetherOS battery report\n======================\n");
        match self.cycle_count() {
            Some(cycles) => out.push_str(&format!("Cycle count: {cycles} (reported by battery)\n")),
            None => out.push_str("Cycle count: Not reported by this battery\n"),
        }
        if let Some(cycles) = self.estimated_cycles() {
            out.push_str(&format!("Estimated cycles: {cycles:.1} (Estimate)\n"));
        }
        match self.health_percent() {
            Some(health) => out.push_str(&format!("Battery health: {health:.1}%\n")),
            None => out.push_str("Battery health: Not reported by this battery\n"),
        }
        out.push_str(&format!("Recorded sessions: {}\n", self.sessions.len()));
        out.push_str(&format!("Days of history: {}\n", self.daily.len()));
        let top = self.energy_by_app(
            self.samples.last().map(|s| s.at).unwrap_or(0),
            7,
        );
        if top.is_empty() {
            out.push_str("Energy by app: no samples yet\n");
        } else {
            out.push_str("Energy by app (last 7 days):\n");
            for (app, wh) in top.iter().take(10) {
                out.push_str(&format!("  {app}: {wh:.2} Wh\n"));
            }
        }
        out
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: u64, percent: f32, charging: bool, rate: f32, energy: f32) -> BatterySample {
        BatterySample {
            at,
            percent,
            charging,
            source: if charging {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
            rate_w: Some(rate),
            temp_c: Some(30.0),
            health_pct: Some(88.0),
            cycle_count: Some(120),
            energy_now_wh: Some(energy),
            energy_full_wh: Some(53.2),
        }
    }

    #[test]
    fn cycle_count_is_reported() {
        let mut store = BatteryAnalytics::new(100);
        store.observe(sample(1000, 80.0, false, 10.0, 42.0), true);
        assert_eq!(store.cycle_count(), Some(120));
    }

    #[test]
    fn health_has_one_point_per_day() {
        let mut store = BatteryAnalytics::new(100);
        for hour in 0..5u64 {
            store.observe(sample(hour * 3600, 80.0, false, 10.0, 42.0), true);
        }
        let trend = store.health_trend();
        assert_eq!(trend.len(), 1);
        assert!((trend[0].1 - 88.0).abs() < 1e-4);
    }

    #[test]
    fn discharge_session_tracks_percentage() {
        let mut store = BatteryAnalytics::new(100);
        let start = 10_000;
        for minute in 0..60u64 {
            let percent = 80.0 - minute as f32 * 0.5;
            store.observe(
                sample(start + minute * 60, percent, false, 10.0, 42.0),
                true,
            );
        }
        store.finish();
        let session = store.session().expect("session");
        assert_eq!(session.kind, SessionKind::Discharge);
        assert!((session.start_pct - 80.0).abs() < 0.6);
        assert!((session.end_pct - 50.5).abs() < 0.6);
        assert!(session.energy_wh > 8.0 && session.energy_wh < 10.0);
    }

    #[test]
    fn standby_drain_accumulates_when_screen_off() {
        let mut store = BatteryAnalytics::new(200);
        let start = 1_000_000;
        // Sample every 5 minutes for 8 hours (real cadence; gaps stay gaps).
        for step in 0..96u64 {
            let percent = 80.0 - (step as f32) / 12.0;
            store.observe(
                sample(start + step * 300, percent, false, 5.0, 42.0),
                false,
            );
        }
        let day = (start / DAY_SECS) as i64;
        let daily = store.standby_drain(day).expect("daily");
        assert!((daily.standby_percent - 7.0).abs() < 1.0);
        assert!(daily.standby_wh > 30.0);
    }

    #[test]
    fn energy_by_app_ranks_the_top_consumer() {
        let mut store = BatteryAnalytics::new(10);
        let now = 500_000;
        store.record_app_energy(now, "org.example.Browser", 12.0);
        store.record_app_energy(now, "org.example.Mail", 3.0);
        store.record_app_energy(now, "org.example.Browser", 6.0);
        let rows = store.energy_by_app(now, 1);
        assert_eq!(rows[0].0, "org.example.Browser");
        assert!((rows[0].1 - 18.0).abs() < 1e-6);
    }

    #[test]
    fn estimated_cycles_uses_discharged_energy() {
        let mut store = BatteryAnalytics::new(100);
        let start = 10_000;
        for minute in 0..120u64 {
            store.observe(
                sample(start + minute * 60, 80.0 - minute as f32 * 0.1, false, 10.0, 42.0),
                true,
            );
        }
        let estimate = store.estimated_cycles().expect("estimate");
        assert!(estimate > 0.0);
    }

    #[test]
    fn temperature_history_has_daily_min_max_avg() {
        let mut store = BatteryAnalytics::new(100);
        let mut first = sample(1000, 80.0, false, 10.0, 42.0);
        first.temp_c = Some(28.0);
        let mut second = sample(2000, 79.0, false, 10.0, 41.0);
        second.temp_c = Some(32.0);
        store.observe(first, true);
        store.observe(second, true);
        let history = store.temperature_history();
        assert_eq!(history.len(), 1);
        assert!((history[0].1 - 28.0).abs() < 1e-4);
        assert!((history[0].2 - 32.0).abs() < 1e-4);
        assert!((history[0].3 - 30.0).abs() < 1e-4);
    }

    #[test]
    fn report_labels_missing_and_estimates() {
        let store = BatteryAnalytics::new(10);
        let report = store.report();
        assert!(report.contains("Not reported by this battery"));
        let mut store = BatteryAnalytics::new(10);
        store.observe(sample(1000, 80.0, false, 10.0, 42.0), true);
        let report = store.report();
        assert!(report.contains("Cycle count: 120"));
        assert!(report.contains("(Estimate)"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut store = BatteryAnalytics::new(100);
        store.observe(sample(1000, 80.0, false, 10.0, 42.0), true);
        let dir = std::env::temp_dir().join(format!("aether-analytics-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("battery.json");
        store.save(&path).unwrap();
        let loaded = BatteryAnalytics::load(&path).unwrap();
        assert_eq!(loaded.samples.len(), 1);
        assert_eq!(loaded.cycle_count(), Some(120));
        let _ = fs::remove_dir_all(dir);
    }
}

impl BatteryAnalytics {
    /// Convenience for tests/UI: the most recent closed session.
    pub fn session(&self) -> Option<&Session> {
        self.sessions.last()
    }
}
