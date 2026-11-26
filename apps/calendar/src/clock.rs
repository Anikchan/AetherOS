//! Clock core (P4-T16, DA-07): digital and analogue time, stopwatch,
//! countdown timer, alarms and fixed-offset time zones.
//!
//! The module is pure Rust and time is passed in (or ticked by a delta) so the
//! symmetry, lap, expiry and alarm rules are unit-testable without sleeping. The
//! analogue path helpers return SVG command strings for the Slint `Path`
//! elements, so the face geometry comes from the same angle math the tests
//! check.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::calendar::WEEKDAY_NAMES;

/// One line of the analogue dial plus both hands, drawn in a 24x24-ish viewbox
/// scaled by the caller (`dial_path(size)` uses a `size` x `size` viewbox).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandAngles {
    /// Degrees clockwise from 12 o'clock.
    pub hour: f32,
    pub minute: f32,
    pub second: f32,
}

/// Clock hand angles for a wall-clock time.
///
/// The hour hand advances continuously with the minutes (`0.5°` per minute) and
/// the minute hand with the seconds (`0.1°` per second), which is what a real
/// clock does.
pub fn analog_angles(hour: u32, minute: u32, second: u32) -> HandAngles {
    let hour = hour % 12;
    let minute = minute.min(59);
    let second = second.min(59);
    HandAngles {
        hour: hour as f32 * 30.0 + minute as f32 * 0.5 + second as f32 / 120.0,
        minute: minute as f32 * 6.0 + second as f32 * 0.1,
        second: second as f32 * 6.0,
    }
}

/// End point of a hand of `radius` at `angle` degrees, as an offset from the
/// centre (y grows downwards, like Slint/SVG).
pub fn hand_endpoint(angle_deg: f32, radius: f32) -> (f32, f32) {
    let radians = angle_deg.to_radians();
    (radius * radians.sin(), -radius * radians.cos())
}

fn path_segment(from: (f32, f32), to: (f32, f32)) -> String {
    format!(
        "M {:.2} {:.2} L {:.2} {:.2}",
        from.0, from.1, to.0, to.1
    )
}

/// Twelve hour ticks on a face of `size` logical pixels.
pub fn dial_path(size: f32) -> String {
    let centre = size / 2.0;
    let inner = size * 0.415;
    let outer = size * 0.462;
    (0..12)
        .map(|hour| {
            let angle = hour as f32 * 30.0;
            let (sx, sy) = hand_endpoint(angle, inner);
            let (ex, ey) = hand_endpoint(angle, outer);
            path_segment((centre + sx, centre + sy), (centre + ex, centre + ey))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hour and minute hands from the centre to their tips.
pub fn hands_path(size: f32, angles: HandAngles) -> String {
    let centre = size / 2.0;
    let (hour_x, hour_y) = hand_endpoint(angles.hour, size * 0.24);
    let (minute_x, minute_y) = hand_endpoint(angles.minute, size * 0.34);
    format!(
        "{} {}",
        path_segment(
            (centre, centre),
            (centre + hour_x, centre + hour_y)
        ),
        path_segment(
            (centre, centre),
            (centre + minute_x, centre + minute_y)
        )
    )
}

/// Second hand with a short counterweight tail.
pub fn second_path(size: f32, angle_deg: f32) -> String {
    let centre = size / 2.0;
    let (tip_x, tip_y) = hand_endpoint(angle_deg, size * 0.38);
    let (tail_x, tail_y) = hand_endpoint(angle_deg, -size * 0.08);
    path_segment(
        (centre + tail_x, centre + tail_y),
        (centre + tip_x, centre + tip_y),
    )
}

/// `09:41:07` in 24-hour mode, `9:41:07 AM` in 12-hour mode.
pub fn digital(hour: u32, minute: u32, second: u32, use_24h: bool) -> String {
    let hour = hour.min(23);
    let minute = minute.min(59);
    let second = second.min(59);
    if use_24h {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else {
        let suffix = if hour < 12 { "AM" } else { "PM" };
        let display = match hour % 12 {
            0 => 12,
            other => other,
        };
        format!("{display}:{minute:02}:{second:02} {suffix}")
    }
}

/// `Monday, 23 November 2026` for the clock face header.
pub fn long_date(date: chrono::NaiveDate) -> String {
    date.format("%A, %-d %B %Y").to_string()
}

/// `mm:ss.d` — stopwatch precision (tenths, like the reference app).
pub fn format_stopwatch(elapsed: Duration) -> String {
    let total_tenths = elapsed.as_millis() / 100;
    let minutes = total_tenths / 600;
    let seconds = (total_tenths / 10) % 60;
    let tenths = total_tenths % 10;
    format!("{minutes:02}:{seconds:02}.{tenths}")
}

/// `mm:ss` for the countdown timer (rounded up, so it never shows 00:00 early).
pub fn format_countdown(remaining: Duration) -> String {
    let seconds = remaining.as_secs() + if remaining.subsec_millis() > 0 { 1 } else { 0 };
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Stopwatch state machine: start/pause, laps and reset.
///
/// Time only advances through [`Stopwatch::tick`] so tests are deterministic;
/// the interactive binary drives it from a one-second `slint::Timer`.
#[derive(Debug, Clone, Default)]
pub struct Stopwatch {
    elapsed: Duration,
    lap_start: Duration,
    running: bool,
    laps: Vec<Duration>,
}

impl Stopwatch {
    pub fn new() -> Stopwatch {
        Stopwatch::default()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn laps(&self) -> &[Duration] {
        &self.laps
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn toggle(&mut self) {
        self.running = !self.running;
    }

    /// Advance by `delta`; ignored while paused or stopped.
    pub fn tick(&mut self, delta: Duration) {
        if self.running {
            self.elapsed += delta;
        }
    }

    /// Record a lap split (time since the previous lap). Returns `None` when the
    /// stopwatch is not running.
    pub fn lap(&mut self) -> Option<Duration> {
        if !self.running {
            return None;
        }
        let split = self.elapsed.saturating_sub(self.lap_start);
        self.lap_start = self.elapsed;
        self.laps.push(split);
        Some(split)
    }

    /// Back to zero, stopped, laps cleared.
    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.lap_start = Duration::ZERO;
        self.running = false;
        self.laps.clear();
    }
}

/// Countdown timer state machine.
#[derive(Debug, Clone)]
pub struct Timer {
    total: Duration,
    remaining: Duration,
    running: bool,
    finished: bool,
}

impl Timer {
    pub fn new(total: Duration) -> Timer {
        Timer {
            total,
            remaining: total,
            running: false,
            finished: false,
        }
    }

    pub fn with_seconds(seconds: u64) -> Timer {
        Timer::new(Duration::from_secs(seconds))
    }

    pub fn total(&self) -> Duration {
        self.total
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn start(&mut self) {
        if self.finished || self.remaining.is_zero() {
            self.remaining = self.total;
            self.finished = false;
        }
        self.running = true;
    }

    pub fn pause(&mut self) {
        self.running = false;
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.pause();
        } else {
            self.start();
        }
    }

    /// Advance the countdown. Returns `true` on the tick that expires it.
    pub fn tick(&mut self, delta: Duration) -> bool {
        if !self.running || self.finished {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(delta);
        if self.remaining.is_zero() {
            self.running = false;
            self.finished = true;
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.remaining = self.total;
        self.running = false;
        self.finished = false;
    }

    /// Extend (or shorten with `secs` capped at the total) the countdown.
    pub fn add_seconds(&mut self, seconds: u64) {
        self.remaining += Duration::from_secs(seconds);
        self.total = self.total.max(self.remaining);
        self.finished = false;
    }

    /// 0.0 at the start, 1.0 when expired.
    pub fn progress(&self) -> f32 {
        if self.total.is_zero() {
            return 1.0;
        }
        let elapsed = self.total.saturating_sub(self.remaining).as_secs_f32();
        (elapsed / self.total.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// A fixed-offset time zone for the second clock (DA-07 "time zones").
///
/// v1 uses fixed offsets: the host's own local zone is the main clock, and the
/// extra clock answers "what time is it in …". DST-aware zone data is a later
/// task (it needs the tz database in the image).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    pub name: &'static str,
    pub offset_minutes: i32,
}

impl Zone {
    /// `UTC+05:30`, `UTC-08:00`, or `UTC` for the zero offset.
    pub fn offset_label(self) -> String {
        if self.offset_minutes == 0 {
            return "UTC".to_string();
        }
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let total = self.offset_minutes.abs();
        format!("UTC{sign}{:02}:{:02}", total / 60, total % 60)
    }
}

/// Zones offered by the second clock (v1 fixture list, alphabetical by region).
pub const ZONES: [Zone; 8] = [
    Zone {
        name: "UTC",
        offset_minutes: 0,
    },
    Zone {
        name: "London",
        offset_minutes: 0,
    },
    Zone {
        name: "Berlin",
        offset_minutes: 60,
    },
    Zone {
        name: "New Delhi",
        offset_minutes: 330,
    },
    Zone {
        name: "Tokyo",
        offset_minutes: 540,
    },
    Zone {
        name: "Sydney",
        offset_minutes: 600,
    },
    Zone {
        name: "New York",
        offset_minutes: -300,
    },
    Zone {
        name: "Los Angeles",
        offset_minutes: -480,
    },
];

/// Local time shifted into a fixed offset (day wrap included).
pub fn offset_clock(hour: u32, minute: u32, second: u32, offset_minutes: i32) -> (u32, u32, u32) {
    let total = hour as i64 * 60 + minute as i64 + offset_minutes as i64;
    let total = total.rem_euclid(24 * 60);
    (
        (total / 60) as u32,
        (total % 60) as u32,
        second.min(59),
    )
}

/// One alarm (DA-07: "clock, alarm, timer, stopwatch").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: String,
    pub label: String,
    pub hour: u32,
    pub minute: u32,
    #[serde(default = "alarm_enabled_default")]
    pub enabled: bool,
    /// Monday-first repeat days; empty means every day.
    #[serde(default)]
    pub days: Vec<usize>,
}

fn alarm_enabled_default() -> bool {
    true
}

impl Alarm {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        hour: u32,
        minute: u32,
    ) -> Alarm {
        Alarm {
            id: id.into(),
            label: label.into(),
            hour,
            minute,
            enabled: true,
            days: Vec::new(),
        }
    }

    /// Alarm repeating only on the given Monday-first weekdays.
    pub fn on_days(mut self, days: impl IntoIterator<Item = usize>) -> Alarm {
        self.days = days.into_iter().filter(|day| *day < 7).collect();
        self.days.sort_unstable();
        self.days.dedup();
        self
    }

    /// `07:30`.
    pub fn time_label(&self) -> String {
        format!("{:02}:{:02}", self.hour.min(23), self.minute.min(59))
    }

    /// `Every day`, `Weekdays`, `Weekends`, or `Mon, Wed`.
    pub fn day_label(&self) -> String {
        if self.days.is_empty() {
            return "Every day".to_string();
        }
        if self.days == [0, 1, 2, 3, 4] {
            return "Weekdays".to_string();
        }
        if self.days == [5, 6] {
            return "Weekends".to_string();
        }
        self.days
            .iter()
            .map(|day| &WEEKDAY_NAMES[*day][..3])
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Does this alarm repeat on `weekday` (Monday = 0)?
    pub fn repeats_on(&self, weekday: usize) -> bool {
        self.days.is_empty() || self.days.contains(&(weekday % 7))
    }

    /// Does the alarm fire at exactly this minute?
    pub fn matches(&self, weekday: usize, hour: u32, minute: u32) -> bool {
        self.enabled
            && self.hour == hour
            && self.minute == minute
            && self.repeats_on(weekday)
    }

    /// Whole minutes until the next firing, 0 meaning "right now"; `None` when
    /// the alarm is disabled.
    pub fn minutes_until(&self, weekday: usize, hour: u32, minute: u32) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        let now = hour.min(23) * 60 + minute.min(59);
        let target = self.hour.min(23) * 60 + self.minute.min(59);
        for day_offset in 0..7u32 {
            let day = ((weekday % 7) as u32 + day_offset) % 7;
            if !self.repeats_on(day as usize) {
                continue;
            }
            let delta = day_offset as i64 * 24 * 60 + target as i64 - now as i64;
            if delta >= 0 {
                return Some(delta as u32);
            }
        }
        None
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("an alarm needs an id".into());
        }
        if self.hour > 23 || self.minute > 59 {
            return Err(format!("\"{}\" has a time outside 00:00–23:59", self.label));
        }
        if self.days.iter().any(|day| *day > 6) {
            return Err(format!("\"{}\" has an invalid repeat day", self.label));
        }
        Ok(())
    }
}

/// JSON-backed alarm store, the same atomic-write contract as [`crate::EventStore`].
#[derive(Debug, Clone)]
pub struct AlarmStore {
    path: PathBuf,
    alarms: Vec<Alarm>,
}

impl AlarmStore {
    pub fn new(path: impl Into<PathBuf>) -> AlarmStore {
        AlarmStore {
            path: path.into(),
            alarms: Vec::new(),
        }
    }

    /// `$XDG_CONFIG_HOME/aether/calendar/alarms.json` (or `~/.config/...`).
    pub fn at_config_dir() -> AlarmStore {
        AlarmStore::new(crate::calendar::config_dir().join("calendar").join("alarms.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn alarms(&self) -> &[Alarm] {
        &self.alarms
    }

    pub fn len(&self) -> usize {
        self.alarms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.alarms.is_empty()
    }

    /// Read the file; a missing file is the first run, a corrupt one is reported
    /// and the loaded alarms are kept.
    pub fn load(&mut self) -> Result<(), String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                let parsed: Vec<Alarm> = serde_json::from_str(&text).map_err(|error| {
                    format!(
                        "{} is not valid alarm JSON ({error}); keeping the {} alarm(s) already loaded",
                        self.path.display(),
                        self.alarms.len()
                    )
                })?;
                self.alarms = parsed
                    .into_iter()
                    .filter(|alarm| alarm.validate().is_ok())
                    .collect();
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.alarms.clear();
                Ok(())
            }
            Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
        }
    }

    pub fn load_or_default(&mut self) {
        let _ = self.load();
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.alarms)
            .map_err(|error| format!("cannot serialise alarms: {error}"))?;
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, json)
            .map_err(|error| format!("cannot write {}: {error}", temp.display()))?;
        fs::rename(&temp, &self.path)
            .map_err(|error| format!("cannot replace {}: {error}", self.path.display()))
    }

    pub fn add(&mut self, alarm: Alarm) -> Result<(), String> {
        alarm.validate()?;
        if self.alarms.iter().any(|existing| existing.id == alarm.id) {
            return Err(format!("alarm id \"{}\" already exists", alarm.id));
        }
        self.alarms.push(alarm);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<Alarm> {
        let index = self.alarms.iter().position(|alarm| alarm.id == id)?;
        Some(self.alarms.remove(index))
    }

    pub fn get(&self, id: &str) -> Option<&Alarm> {
        self.alarms.iter().find(|alarm| alarm.id == id)
    }

    /// Flip `enabled`; `None` when the id is unknown.
    pub fn toggle(&mut self, id: &str) -> Option<bool> {
        let alarm = self.alarms.iter_mut().find(|alarm| alarm.id == id)?;
        alarm.enabled = !alarm.enabled;
        Some(alarm.enabled)
    }

    /// Enabled alarms that match this minute, earliest first.
    pub fn due(&self, weekday: usize, hour: u32, minute: u32) -> Vec<&Alarm> {
        let mut due: Vec<&Alarm> = self
            .alarms
            .iter()
            .filter(|alarm| alarm.matches(weekday, hour, minute))
            .collect();
        due.sort_by(|a, b| (a.hour, a.minute).cmp(&(b.hour, b.minute)));
        due
    }

    /// Next free `alarm-N` id.
    pub fn next_id(&self) -> String {
        let mut sequence = self.alarms.len() + 1;
        loop {
            let candidate = format!("alarm-{sequence}");
            if self.get(&candidate).is_none() {
                return candidate;
            }
            sequence += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digital_formatting_covers_12_and_24_hour_modes() {
        assert_eq!(digital(9, 41, 7, true), "09:41:07");
        assert_eq!(digital(0, 0, 0, true), "00:00:00");
        assert_eq!(digital(23, 59, 59, true), "23:59:59");
        assert_eq!(digital(9, 41, 7, false), "9:41:07 AM");
        assert_eq!(digital(12, 5, 0, false), "12:05:00 PM");
        assert_eq!(digital(0, 30, 15, false), "12:30:15 AM");
        assert_eq!(digital(13, 30, 0, false), "1:30:00 PM");
        assert_eq!(digital(99, 0, 0, true), "23:00:00", "out-of-range hours clamp");
    }

    #[test]
    fn analog_angles_follow_clock_arithmetic() {
        // 3:00 -> hour hand exactly at 3 o'clock (90°), minute hand at 12.
        let three = analog_angles(3, 0, 0);
        assert_eq!(three.hour, 90.0);
        assert_eq!(three.minute, 0.0);
        assert_eq!(three.second, 0.0);

        // 9:30 -> hour hand halfway between 9 and 10, minute hand at 6 (180°).
        let half_past = analog_angles(9, 30, 0);
        assert_eq!(half_past.hour, 285.0);
        assert_eq!(half_past.minute, 180.0);

        // 12:00:30 -> hour hand creeps by 0.25°, second hand at 180°.
        let late = analog_angles(12, 0, 30);
        assert!((late.hour - 0.25).abs() < f32::EPSILON);
        assert!((late.second - 180.0).abs() < f32::EPSILON);

        // 18:45:15 wraps to the 12-hour dial.
        let evening = analog_angles(18, 45, 15);
        assert_eq!(evening.hour, 6.0 * 30.0 + 45.0 * 0.5 + 15.0 / 120.0);
        assert_eq!(evening.minute, 271.5);
        assert_eq!(evening.second, 90.0);
    }

    #[test]
    fn hand_endpoints_and_paths_are_geometrically_sane() {
        // 12 o'clock points up (negative y), 3 o'clock points right.
        let (x, y) = hand_endpoint(0.0, 10.0);
        assert!(x.abs() < 1e-4 && (y + 10.0).abs() < 1e-4);
        let (x, y) = hand_endpoint(90.0, 10.0);
        assert!((x - 10.0).abs() < 1e-4 && y.abs() < 1e-4);

        let dial = dial_path(200.0);
        assert_eq!(dial.matches("M ").count(), 12, "twelve hour ticks");
        let hands = hands_path(200.0, analog_angles(1, 0, 0));
        assert_eq!(hands.matches("M ").count(), 2, "hour + minute hands");
        let seconds = second_path(200.0, 180.0);
        assert!(seconds.contains("M "));
        assert!(seconds.contains("L "));
    }

    #[test]
    fn stopwatch_ignores_time_while_paused_and_records_laps() {
        let mut stopwatch = Stopwatch::new();
        stopwatch.tick(Duration::from_secs(5));
        assert_eq!(stopwatch.elapsed(), Duration::ZERO, "stopped watch never advances");

        stopwatch.start();
        stopwatch.tick(Duration::from_millis(1500));
        assert_eq!(stopwatch.elapsed(), Duration::from_millis(1500));
        assert_eq!(stopwatch.lap(), Some(Duration::from_millis(1500)));

        stopwatch.tick(Duration::from_millis(500));
        assert_eq!(stopwatch.lap(), Some(Duration::from_millis(500)));
        assert_eq!(stopwatch.laps().len(), 2);

        stopwatch.stop();
        assert_eq!(stopwatch.lap(), None, "no laps while paused");
        stopwatch.tick(Duration::from_secs(10));
        assert_eq!(stopwatch.elapsed(), Duration::from_secs(2), "paused watch freezes");

        stopwatch.reset();
        assert_eq!(stopwatch.elapsed(), Duration::ZERO);
        assert!(stopwatch.laps().is_empty());
        assert!(!stopwatch.is_running());
    }

    #[test]
    fn stopwatch_formats_tenths() {
        assert_eq!(format_stopwatch(Duration::ZERO), "00:00.0");
        assert_eq!(format_stopwatch(Duration::from_millis(12_300)), "00:12.3");
        assert_eq!(format_stopwatch(Duration::from_secs(65)), "01:05.0");
        assert_eq!(format_stopwatch(Duration::from_secs(3600)), "60:00.0");
    }

    #[test]
    fn countdown_timer_expires_once_and_can_restart() {
        let mut timer = Timer::with_seconds(3);
        assert_eq!(format_countdown(timer.remaining()), "00:03");
        assert!(!timer.tick(Duration::from_secs(1)), "paused timer does not advance");

        timer.start();
        assert!(!timer.tick(Duration::from_secs(1)));
        assert_eq!(timer.remaining(), Duration::from_secs(2));
        assert!(!timer.tick(Duration::from_secs(1)));
        assert!(timer.tick(Duration::from_secs(5)), "the tick past zero expires it");
        assert!(timer.is_finished());
        assert!(!timer.is_running());
        assert_eq!(timer.remaining(), Duration::ZERO);
        assert_eq!(format_countdown(timer.remaining()), "00:00");
        assert_eq!(timer.progress(), 1.0);
        assert!(!timer.tick(Duration::from_secs(1)), "an expired timer stays expired");

        // Starting again restarts from the full duration.
        timer.start();
        assert_eq!(timer.remaining(), Duration::from_secs(3));
        assert!(!timer.is_finished());
        assert_eq!(timer.progress(), 0.0);
        assert_eq!(format_countdown(Duration::from_millis(1500)), "00:02", "rounds up");
    }

    #[test]
    fn fixed_offset_zones_shift_and_wrap_the_clock() {
        assert_eq!(offset_clock(9, 41, 7, 0), (9, 41, 7));
        assert_eq!(offset_clock(9, 41, 7, 330), (15, 11, 7), "New Delhi is UTC+05:30");
        assert_eq!(offset_clock(23, 30, 0, 60), (0, 30, 0), "wraps past midnight");
        assert_eq!(offset_clock(1, 15, 0, -480), (17, 15, 0), "wraps back a day");
        assert_eq!(ZONES.len(), 8);
        assert_eq!(ZONES[0].offset_label(), "UTC");
        assert_eq!(ZONES[3].offset_label(), "UTC+05:30");
        assert_eq!(ZONES[7].offset_label(), "UTC-08:00");
        // The zoned clock reuses the same digital formatter.
        let (hour, minute, second) = offset_clock(9, 41, 7, 330);
        assert_eq!(digital(hour, minute, second, true), "15:11:07");
    }

    #[test]
    fn alarms_match_the_minute_and_repeat_days() {
        let daily = Alarm::new("alarm-1", "Wake up", 7, 30);
        assert!(daily.matches(0, 7, 30), "Monday 07:30 fires");
        assert!(daily.matches(6, 7, 30), "Sunday 07:30 fires too");
        assert!(!daily.matches(0, 7, 31), "a different minute does not");
        assert_eq!(daily.time_label(), "07:30");
        assert_eq!(daily.day_label(), "Every day");
        assert!(daily.validate().is_ok());

        let weekdays = Alarm::new("alarm-2", "Stand-up", 9, 30)
            .on_days([0, 1, 2, 3, 4]);
        assert_eq!(weekdays.day_label(), "Weekdays");
        assert!(weekdays.matches(4, 9, 30));
        assert!(!weekdays.matches(5, 9, 30), "no weekend firing");
        let weekend = Alarm::new("alarm-3", "Long run", 8, 0).on_days([5, 6]);
        assert_eq!(weekend.day_label(), "Weekends");
        let custom = Alarm::new("alarm-4", "Bin day", 18, 0).on_days([2, 4, 9]);
        assert_eq!(custom.day_label(), "Wed, Fri", "invalid days are dropped");

        let mut disabled = Alarm::new("alarm-5", "Off", 7, 0);
        disabled.enabled = false;
        assert!(!disabled.matches(0, 7, 0));
        assert_eq!(disabled.minutes_until(0, 6, 0), None);
    }

    #[test]
    fn next_occurrence_counts_minutes_across_days() {
        let alarm = Alarm::new("alarm-1", "Wake up", 7, 30);
        assert_eq!(alarm.minutes_until(0, 7, 30), Some(0), "right now");
        assert_eq!(alarm.minutes_until(0, 7, 0), Some(30));
        assert_eq!(alarm.minutes_until(0, 8, 0), Some(24 * 60 - 30), "tomorrow");

        let weekdays = Alarm::new("alarm-2", "Work", 9, 0).on_days([0, 1, 2, 3, 4]);
        // Friday 10:00 -> Monday 09:00 is 2 days + 23 hours.
        assert_eq!(weekdays.minutes_until(4, 10, 0), Some(2 * 24 * 60 + 23 * 60));
        // Saturday -> Monday morning.
        assert_eq!(weekdays.minutes_until(5, 10, 0), Some(24 * 60 + 23 * 60));
        // Invalid times are refused at validation, not by panicking.
        assert!(Alarm::new("alarm-3", "Bad", 24, 0).validate().is_err());
        assert!(Alarm::new("alarm-4", "Bad", 7, 60).validate().is_err());
        assert!(Alarm::new("", "Bad", 7, 0).validate().is_err());
    }

    fn temp_alarm_store(tag: &str) -> AlarmStore {
        let base = std::env::temp_dir().join(format!(
            "aether-alarms-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        AlarmStore::new(base.join("calendar").join("alarms.json"))
    }

    #[test]
    fn alarm_store_persists_toggles_and_reports_due_alarms() {
        let mut store = temp_alarm_store("crud");
        assert_eq!(store.load(), Ok(()));
        assert!(store.is_empty());

        store
            .add(Alarm::new("alarm-1", "Wake up", 7, 30))
            .unwrap();
        store
            .add(Alarm::new("alarm-2", "Lunch", 12, 0).on_days([0, 1, 2, 3, 4]))
            .unwrap();
        assert!(store.add(Alarm::new("alarm-1", "Dup", 8, 0)).is_err());
        store.save().unwrap();

        let mut reloaded = AlarmStore::new(store.path().to_path_buf());
        reloaded.load().unwrap();
        assert_eq!(reloaded.len(), 2);
        assert_eq!(
            reloaded.due(0, 7, 30).iter().map(|alarm| alarm.id.as_str()).collect::<Vec<_>>(),
            vec!["alarm-1"]
        );
        assert_eq!(reloaded.due(5, 12, 0).len(), 0, "no weekend lunch");
        assert_eq!(reloaded.due(0, 7, 29).len(), 0);

        assert_eq!(reloaded.toggle("alarm-1"), Some(false));
        assert_eq!(reloaded.due(0, 7, 30).len(), 0, "disabled alarms do not fire");
        assert_eq!(reloaded.toggle("alarm-1"), Some(true));
        assert_eq!(reloaded.toggle("missing"), None);
        assert_eq!(reloaded.next_id(), "alarm-3");
        assert!(reloaded.remove("alarm-2").is_some());
        assert_eq!(reloaded.len(), 1);

        let _ = fs::remove_dir_all(store.path().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn corrupt_alarm_file_is_reported_and_keeps_loaded_alarms() {
        let store = temp_alarm_store("corrupt");
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "[{\"id\":\"alarm-1\",\"label\":\"Kept\",\"hour\":7,\"minute\":0}]",
        )
        .unwrap();
        let mut store = AlarmStore::new(store.path().to_path_buf());
        store.load().unwrap();
        assert_eq!(store.len(), 1);
        assert!(store.alarms()[0].enabled, "enabled defaults to true");

        fs::write(store.path(), "not json").unwrap();
        let error = store.load().unwrap_err();
        assert!(error.contains("not valid alarm JSON"));
        assert_eq!(store.len(), 1, "the loaded alarm survives");
        let _ = fs::remove_dir_all(store.path().parent().unwrap().parent().unwrap());
    }
}
