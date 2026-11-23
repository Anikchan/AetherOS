//! Screen-time store (P5-T17, F14 section 2.2, ST-01..ST-04).
//!
//! Data comes from compositor focus/screen events (S2/S3) and the power
//! daemon's screen state. Everything is local. Missing days are reported as
//! gaps, never as zero (ST-AC-03).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::analytics::DAY_SECS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DayScreenTime {
    pub day: i64,
    pub screen_on_secs: u64,
    pub per_app_secs: BTreeMap<String, u64>,
    pub pickups: u32,
    pub notifications: BTreeMap<String, u32>,
}

impl DayScreenTime {
    /// ST-AC-02: per-app totals must add up to screen-on time within a minute.
    pub fn per_app_matches_screen_on(&self) -> bool {
        let sum: u64 = self.per_app_secs.values().sum();
        sum.abs_diff(self.screen_on_secs) <= 60
    }

    pub fn notification_total(&self) -> u32 {
        self.notifications.values().sum()
    }
}

/// Accuracy of the store against an external compositor focus log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusLogAccuracy {
    pub screen_on_diff_secs: u64,
    pub per_app_diff_secs: u64,
    pub within_one_minute: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScreenTimeStore {
    pub days: BTreeMap<i64, DayScreenTime>,
    #[serde(skip)]
    screen_on: bool,
    #[serde(skip)]
    screen_on_since: Option<u64>,
    #[serde(skip)]
    focus: Option<(String, u64)>,
}

impl ScreenTimeStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn day(&mut self, at: u64) -> &mut DayScreenTime {
        let day = (at / DAY_SECS) as i64;
        self.days.entry(day).or_insert_with(|| DayScreenTime {
            day,
            ..Default::default()
        })
    }

    /// Split an interval across day boundaries and add `seconds` to each day.
    fn add_interval(&mut self, start: u64, end: u64, app: Option<&str>, screen_only: bool) {
        if end <= start {
            return;
        }
        let mut cursor = start;
        while cursor < end {
            let day = cursor / DAY_SECS;
            let day_end = (day + 1) * DAY_SECS;
            let slice_end = end.min(day_end);
            let secs = slice_end - cursor;
            let record = self.day(cursor);
            if screen_only {
                record.screen_on_secs += secs;
            }
            if let Some(app) = app {
                *record.per_app_secs.entry(app.to_string()).or_insert(0) += secs;
            }
            cursor = slice_end;
        }
    }

    fn flush_screen(&mut self, at: u64) {
        if let Some(start) = self.screen_on_since.take() {
            if at > start {
                self.add_interval(start, at, None, true);
            }
        }
    }

    fn flush_focus(&mut self, at: u64) {
        if let Some((app, start)) = self.focus.take() {
            if at > start {
                self.add_interval(start, at, Some(&app), false);
            }
        }
    }

    /// Screen turned on or off (lid, power button, idle).
    pub fn set_screen_on(&mut self, on: bool, at: u64) {
        if on && !self.screen_on {
            self.screen_on = true;
            self.screen_on_since = Some(at);
            self.day(at).pickups += 1;
        } else if !on && self.screen_on {
            if let Some((app, start)) = self.focus.take() {
                if at > start {
                    self.add_interval(start, at, Some(&app), false);
                }
            }
            self.flush_screen(at);
            self.screen_on = false;
        }
    }

    /// Compositor focus change; `None` means no focused app.
    pub fn set_focus(&mut self, app: Option<&str>, at: u64) {
        if let Some((current, start)) = self.focus.clone() {
            if at > start {
                self.add_interval(start, at, Some(&current), false);
            }
        }
        self.focus = app.map(|app| (app.to_string(), at));
    }

    /// A pickup also counts when unlocking without a screen state change.
    pub fn record_pickup(&mut self, at: u64) {
        self.day(at).pickups += 1;
    }

    pub fn record_notification(&mut self, at: u64, app: &str) {
        *self.day(at).notifications.entry(app.to_string()).or_insert(0) += 1;
    }

    /// Flush open intervals (daemon shutdown, day rollover).
    pub fn flush(&mut self, at: u64) {
        self.flush_focus(at);
        self.flush_screen(at);
        if self.screen_on {
            // Keep the screen-on interval open across the flush.
            self.screen_on_since = Some(at);
        }
        if let Some((app, _)) = self.focus.take() {
            self.focus = Some((app, at));
        }
    }

    pub fn summary(&self, day: i64) -> Option<&DayScreenTime> {
        self.days.get(&day)
    }

    /// Last `n` days, oldest first; missing days are `None` (gaps).
    pub fn timeline(&self, now: u64, n: u64) -> Vec<(i64, Option<&DayScreenTime>)> {
        let today = (now / DAY_SECS) as i64;
        (0..n as i64)
            .map(|offset| {
                let day = today - (n as i64 - 1) + offset;
                (day, self.days.get(&day))
            })
            .collect()
    }

    /// ST-AC-01/ST-AC-02: compare one day with a compositor focus log.
    ///
    /// `log` entries are `(app, start_epoch, end_epoch)`.
    pub fn verify_against_focus_log(&self, day: i64, log: &[(String, u64, u64)]) -> FocusLogAccuracy {
        let start = day as u64 * DAY_SECS;
        let end = start + DAY_SECS;
        let mut screen_on = 0u64;
        let mut per_app: BTreeMap<String, u64> = BTreeMap::new();
        for (app, from, to) in log {
            let from = (*from).max(start);
            let to = (*to).min(end);
            if to > from {
                screen_on += to - from;
                *per_app.entry(app.clone()).or_insert(0) += to - from;
            }
        }
        let observed = self.days.get(&day).cloned().unwrap_or_default();
        let screen_on_diff = observed.screen_on_secs.abs_diff(screen_on);
        let mut per_app_diff = 0u64;
        for (app, secs) in &per_app {
            let ours = observed.per_app_secs.get(app).copied().unwrap_or(0);
            per_app_diff += ours.abs_diff(*secs);
        }
        FocusLogAccuracy {
            screen_on_diff_secs: screen_on_diff,
            per_app_diff_secs: per_app_diff,
            within_one_minute: screen_on_diff <= 60 && observed.per_app_matches_screen_on(),
        }
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

    #[test]
    fn screen_on_time_accumulates() {
        let mut store = ScreenTimeStore::new();
        store.set_screen_on(true, 1000);
        store.set_screen_on(false, 1600);
        let day = (1000 / DAY_SECS) as i64;
        assert_eq!(store.summary(day).unwrap().screen_on_secs, 600);
    }

    #[test]
    fn per_app_time_comes_from_focus() {
        let mut store = ScreenTimeStore::new();
        store.set_screen_on(true, 1000);
        store.set_focus(Some("org.example.Mail"), 1000);
        store.set_focus(Some("org.example.Browser"), 1300);
        store.set_focus(None, 1600);
        store.set_screen_on(false, 1600);
        let day = (1000 / DAY_SECS) as i64;
        let summary = store.summary(day).unwrap();
        assert_eq!(summary.per_app_secs.get("org.example.Mail"), Some(&300));
        assert_eq!(summary.per_app_secs.get("org.example.Browser"), Some(&300));
        assert_eq!(summary.screen_on_secs, 600);
        assert!(summary.per_app_matches_screen_on());
    }

    #[test]
    fn pickups_and_notifications_are_counted() {
        let mut store = ScreenTimeStore::new();
        store.set_screen_on(true, 1000);
        store.set_screen_on(false, 1100);
        store.set_screen_on(true, 1200);
        store.record_pickup(1300);
        store.record_notification(1400, "org.example.Chat");
        store.record_notification(1500, "org.example.Chat");
        let day = (1000 / DAY_SECS) as i64;
        let summary = store.summary(day).unwrap();
        assert_eq!(summary.pickups, 3);
        assert_eq!(summary.notification_total(), 2);
    }

    #[test]
    fn intervals_split_across_midnight() {
        let mut store = ScreenTimeStore::new();
        let start = DAY_SECS - 300;
        store.set_screen_on(true, start);
        store.set_focus(Some("org.example.Player"), start);
        store.set_screen_on(false, DAY_SECS + 300);
        let first = store.summary(0).unwrap();
        let second = store.summary(1).unwrap();
        assert_eq!(first.screen_on_secs, 300);
        assert_eq!(second.screen_on_secs, 300);
        assert_eq!(first.per_app_secs.get("org.example.Player"), Some(&300));
        assert_eq!(second.per_app_secs.get("org.example.Player"), Some(&300));
    }

    #[test]
    fn missing_days_are_gaps_not_zero() {
        let mut store = ScreenTimeStore::new();
        store.set_screen_on(true, 0);
        store.set_screen_on(false, 60);
        let timeline = store.timeline(2 * DAY_SECS + 100, 3);
        assert_eq!(timeline.len(), 3);
        assert!(timeline[0].1.is_some());
        assert!(timeline[1].1.is_none());
        assert!(timeline[2].1.is_none());
    }

    #[test]
    fn matches_a_focus_log_within_one_minute() {
        let mut store = ScreenTimeStore::new();
        store.set_screen_on(true, 1000);
        store.set_focus(Some("org.example.Mail"), 1000);
        store.set_focus(None, 1300);
        store.set_screen_on(false, 1300);

        let day = (1000 / DAY_SECS) as i64;
        let log = vec![("org.example.Mail".to_string(), 1000u64, 1300u64)];
        let accuracy = store.verify_against_focus_log(day, &log);
        assert!(accuracy.within_one_minute);
        assert_eq!(accuracy.screen_on_diff_secs, 0);
        assert_eq!(accuracy.per_app_diff_secs, 0);
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut store = ScreenTimeStore::new();
        store.set_screen_on(true, 1000);
        store.set_screen_on(false, 1600);
        let dir = std::env::temp_dir().join(format!("aether-screentime-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("screen.json");
        store.save(&path).unwrap();
        let loaded = ScreenTimeStore::load(&path).unwrap();
        let day = (1000 / DAY_SECS) as i64;
        assert_eq!(loaded.summary(day).unwrap().screen_on_secs, 600);
        let _ = fs::remove_dir_all(dir);
    }
}
