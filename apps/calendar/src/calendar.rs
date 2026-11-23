//! Calendar core (P4-T16, DA-07): month grids, navigation and the JSON event
//! store.
//!
//! Everything here is pure Rust so the Slint window stays presentation only:
//! [`crate::app::CalendarController`] is the single place that turns these
//! values into Slint properties.
//!
//! Weeks start on Monday. A month grid always holds six weeks (42 cells) so the
//! layout does not jump between months; cells outside the month come from the
//! adjacent months and are flagged with `in_month == false`.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};

pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub const WEEKDAY_NAMES: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// One calendar day. Serialised as `{ "year": 2026, "month": 11, "day": 23 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// Reject impossible dates (`2026-02-30`, `2026-13-01`, ...).
    pub fn new(year: i32, month: u32, day: u32) -> Result<Date, String> {
        NaiveDate::from_ymd_opt(year, month, day)
            .map(|_| Date { year, month, day })
            .ok_or_else(|| format!("{year:04}-{month:02}-{day:02} is not a valid date"))
    }

    pub fn from_naive(date: NaiveDate) -> Date {
        Date {
            year: date.year(),
            month: date.month(),
            day: date.day(),
        }
    }

    /// Chrono view of this day. Deserialised data is validated on load, so the
    /// fallback only guards against hand-written JSON.
    pub fn to_naive(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, self.day)
            .unwrap_or_else(|| NaiveDate::from_ymd_opt(2000, 1, 1).expect("static date"))
    }

    pub fn today() -> Date {
        Date::from_naive(Local::now().date_naive())
    }

    /// `2026-11-23`, the stable key used by the UI and the JSON file.
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn parse_iso(text: &str) -> Option<Date> {
        let mut parts = text.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        Date::new(year, month, day).ok()
    }

    /// Monday = 0 ... Sunday = 6.
    pub fn weekday_index(self) -> usize {
        self.to_naive().weekday().num_days_from_monday() as usize
    }

    pub fn weekday_name(self) -> &'static str {
        WEEKDAY_NAMES[self.weekday_index()]
    }

    pub fn is_weekend(self) -> bool {
        self.weekday_index() >= 5
    }

    /// `Monday, 23 November 2026`.
    pub fn long_label(self) -> String {
        self.to_naive().format("%A, %-d %B %Y").to_string()
    }

    /// `23 Nov`.
    pub fn short_label(self) -> String {
        self.to_naive().format("%-d %b").to_string()
    }

    pub fn add_days(self, days: i64) -> Date {
        Date::from_naive(
            self.to_naive()
                .checked_add_signed(chrono::Duration::days(days))
                .unwrap_or_else(|| self.to_naive()),
        )
    }

    pub fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    pub fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if Date::is_leap_year(year) => 29,
            2 => 28,
            _ => 30,
        }
    }
}

/// A month/year pair, used for navigation across year boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthYear {
    pub year: i32,
    pub month: u32,
}

impl MonthYear {
    pub fn from_date(date: Date) -> MonthYear {
        MonthYear {
            year: date.year,
            month: date.month,
        }
    }

    pub fn label(self) -> String {
        format!("{} {}", MONTH_NAMES[(self.month - 1) as usize], self.year)
    }

    /// Shift by whole months, normalising December -> January and back.
    pub fn shift(self, months: i32) -> MonthYear {
        let total = self.year * 12 + (self.month as i32 - 1) + months;
        MonthYear {
            year: total.div_euclid(12),
            month: total.rem_euclid(12) as u32 + 1,
        }
    }

    pub fn first_day(self) -> Date {
        Date::new(self.year, self.month, 1).expect("month is normalised")
    }

    pub fn contains(self, date: Date) -> bool {
        date.year == self.year && date.month == self.month
    }

    pub fn grid(self) -> MonthGrid {
        month_grid(self.year, self.month)
    }
}

/// One cell of a month grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayCell {
    pub date: Date,
    pub in_month: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthGrid {
    pub year: i32,
    pub month: u32,
    /// 42 cells: Monday-first weeks, leading/trailing days from the adjacent
    /// months.
    pub cells: Vec<DayCell>,
}

/// Build the six-week grid for a month. The month is normalised first, so
/// `month_grid(2026, 13)` (or 0) still produces a valid grid.
pub fn month_grid(year: i32, month: u32) -> MonthGrid {
    // Start from January of `year` and add (month - 1) months: month 0 lands
    // on the previous December, month 13 on the next January.
    let normalised = MonthYear { year, month: 1 }.shift(month as i32 - 1);
    let first = normalised.first_day();
    let leading = first.weekday_index() as i64;
    let days = Date::days_in_month(normalised.year, normalised.month) as i64;
    let weeks = ((leading + days + 6) / 7).max(6);
    let start = first.add_days(-leading);

    let mut cells = Vec::with_capacity((weeks * 7) as usize);
    for offset in 0..weeks * 7 {
        let date = start.add_days(offset);
        cells.push(DayCell {
            date,
            in_month: date.year == normalised.year && date.month == normalised.month,
        });
    }
    MonthGrid {
        year: normalised.year,
        month: normalised.month,
        cells,
    }
}

/// Colour tag of an event (maps to a design-system tone in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventColor {
    Accent,
    Success,
    Warning,
    Danger,
    Neutral,
}

impl Default for EventColor {
    fn default() -> Self {
        EventColor::Accent
    }
}

impl EventColor {
    pub const ALL: [EventColor; 5] = [
        EventColor::Accent,
        EventColor::Success,
        EventColor::Warning,
        EventColor::Danger,
        EventColor::Neutral,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EventColor::Accent => "Blue",
            EventColor::Success => "Green",
            EventColor::Warning => "Amber",
            EventColor::Danger => "Red",
            EventColor::Neutral => "Grey",
        }
    }

    /// Stable code the .slint file maps to a token tone.
    pub fn code(self) -> i32 {
        match self {
            EventColor::Accent => 0,
            EventColor::Success => 1,
            EventColor::Warning => 2,
            EventColor::Danger => 3,
            EventColor::Neutral => 4,
        }
    }
}

/// Minutes from midnight to `HH:MM`.
pub fn format_minutes(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Parse `HH:MM` (also accepts `H:MM`) into minutes from midnight.
pub fn parse_time(text: &str) -> Option<u32> {
    let text = text.trim();
    let (hours, minutes) = text.split_once(':')?;
    let hours: u32 = hours.trim().parse().ok()?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

/// A calendar event. Multi-day events keep a `start_date`/`end_date` span and
/// `covers()` answers the "events on day" query for every day in between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub start_date: Date,
    pub end_date: Date,
    /// Minutes from midnight (local).
    pub start_minute: u32,
    pub end_minute: u32,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub color: EventColor,
}

impl Event {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        date: Date,
        start_minute: u32,
        end_minute: u32,
    ) -> Event {
        Event {
            id: id.into(),
            title: title.into(),
            start_date: date,
            end_date: date,
            start_minute,
            end_minute,
            notes: String::new(),
            color: EventColor::Accent,
        }
    }

    pub fn with_span(mut self, end_date: Date) -> Event {
        self.end_date = end_date;
        self
    }

    pub fn with_notes(mut self, notes: impl Into<String>) -> Event {
        self.notes = notes.into();
        self
    }

    pub fn with_color(mut self, color: EventColor) -> Event {
        self.color = color;
        self
    }

    pub fn time_label(&self) -> String {
        format!(
            "{} – {}",
            format_minutes(self.start_minute),
            format_minutes(self.end_minute)
        )
    }

    pub fn duration_minutes(&self) -> u32 {
        self.end_minute.saturating_sub(self.start_minute)
    }

    pub fn covers(&self, day: Date) -> bool {
        self.start_date <= day && day <= self.end_date
    }

    pub fn is_multiday(&self) -> bool {
        self.end_date > self.start_date
    }

    /// `23 Nov – 25 Nov` for multi-day events, empty otherwise.
    pub fn day_range_label(&self) -> String {
        if self.is_multiday() {
            format!(
                "{} – {}",
                self.start_date.short_label(),
                self.end_date.short_label()
            )
        } else {
            String::new()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("an event needs an id".into());
        }
        if self.title.trim().is_empty() {
            return Err("an event needs a title".into());
        }
        if self.start_minute >= 24 * 60 || self.end_minute > 24 * 60 {
            return Err(format!("\"{}\" has a time outside 00:00–24:00", self.title));
        }
        if self.start_minute >= self.end_minute {
            return Err(format!(
                "\"{}\" must end after it starts",
                self.title
            ));
        }
        if self.end_date < self.start_date {
            return Err(format!("\"{}\" ends before it starts", self.title));
        }
        Ok(())
    }
}

/// JSON-backed event store. Writes are atomic (temp file + rename); a corrupt
/// file is reported and never overwritten, and unreadable dates are dropped on
/// load instead of aborting the app.
#[derive(Debug, Clone)]
pub struct EventStore {
    path: PathBuf,
    events: Vec<Event>,
}

impl EventStore {
    pub fn new(path: impl Into<PathBuf>) -> EventStore {
        EventStore {
            path: path.into(),
            events: Vec::new(),
        }
    }

    /// `$XDG_CONFIG_HOME/aether/calendar/events.json` (or `~/.config/...`).
    pub fn at_config_dir() -> EventStore {
        let root = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.is_empty() => PathBuf::from(value),
            _ => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                Path::new(&home).join(".config")
            }
        };
        EventStore::new(root.join("aether").join("calendar").join("events.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Read the file. A missing file is not an error (first run); a corrupt one
    /// is reported and the in-memory events are left untouched.
    pub fn load(&mut self) -> Result<(), String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                let parsed: Vec<Event> = serde_json::from_str(&text).map_err(|error| {
                    format!(
                        "{} is not valid calendar JSON ({error}); keeping the {} event(s) already loaded",
                        self.path.display(),
                        self.events.len()
                    )
                })?;
                self.events = parsed
                    .into_iter()
                    .filter(|event| event.validate().is_ok())
                    .collect();
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.events.clear();
                Ok(())
            }
            Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
        }
    }

    /// Best-effort load for app start: a broken file must not stop the UI.
    pub fn load_or_default(&mut self) {
        let _ = self.load();
    }

    /// Persist atomically: create the directory, write a temp file, rename.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.events)
            .map_err(|error| format!("cannot serialise events: {error}"))?;
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, json)
            .map_err(|error| format!("cannot write {}: {error}", temp.display()))?;
        fs::rename(&temp, &self.path)
            .map_err(|error| format!("cannot replace {}: {error}", self.path.display()))
    }

    pub fn add(&mut self, event: Event) -> Result<(), String> {
        event.validate()?;
        if self.events.iter().any(|existing| existing.id == event.id) {
            return Err(format!("event id \"{}\" already exists", event.id));
        }
        self.events.push(event);
        Ok(())
    }

    pub fn update(&mut self, event: Event) -> Result<(), String> {
        event.validate()?;
        let slot = self
            .events
            .iter_mut()
            .find(|existing| existing.id == event.id)
            .ok_or_else(|| format!("no event with id \"{}\"", event.id))?;
        *slot = event;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<Event> {
        let index = self.events.iter().position(|event| event.id == id)?;
        Some(self.events.remove(index))
    }

    pub fn get(&self, id: &str) -> Option<&Event> {
        self.events.iter().find(|event| event.id == id)
    }

    /// Every event covering `day`, earliest start first.
    pub fn on_day(&self, day: Date) -> Vec<Event> {
        let mut events: Vec<Event> = self
            .events
            .iter()
            .filter(|event| event.covers(day))
            .cloned()
            .collect();
        events.sort_by(|a, b| {
            a.start_minute
                .cmp(&b.start_minute)
                .then_with(|| a.title.cmp(&b.title))
        });
        events
    }

    pub fn has_events_on(&self, day: Date) -> bool {
        self.events.iter().any(|event| event.covers(day))
    }

    /// Next free `ev-N` id.
    pub fn next_id(&self) -> String {
        let mut sequence = self.events.len() + 1;
        loop {
            let candidate = format!("ev-{sequence}");
            if self.get(&candidate).is_none() {
                return candidate;
            }
            sequence += 1;
        }
    }
}

/// Local wall clock split into its parts (used by the clock tab).
pub fn now_parts() -> (u32, u32, u32) {
    let now = Local::now();
    (now.hour(), now.minute(), now.second())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> EventStore {
        let base = std::env::temp_dir().join(format!(
            "aether-calendar-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        EventStore::new(base.join("calendar").join("events.json"))
    }

    #[test]
    fn leap_years_and_month_lengths() {
        assert!(Date::is_leap_year(2028));
        assert!(Date::is_leap_year(2000));
        assert!(!Date::is_leap_year(2100));
        assert!(!Date::is_leap_year(2026));
        assert_eq!(Date::days_in_month(2028, 2), 29);
        assert_eq!(Date::days_in_month(2026, 2), 28);
        assert_eq!(Date::days_in_month(2026, 11), 30);

        let february = month_grid(2028, 2);
        assert_eq!(february.cells.len(), 42);
        let days: Vec<u32> = february
            .cells
            .iter()
            .filter(|cell| cell.in_month)
            .map(|cell| cell.date.day)
            .collect();
        assert_eq!(days.len(), 29);
        assert_eq!(days.first().copied(), Some(1));
        assert_eq!(days.last().copied(), Some(29));
        assert!(Date::new(2026, 2, 29).is_err());
    }

    #[test]
    fn first_weekday_is_monday_based_and_grids_start_on_monday() {
        // Known first days: 1 Nov 2026 is a Sunday, 1 Jul 2026 a Wednesday.
        assert_eq!(Date::new(2026, 11, 1).unwrap().weekday_index(), 6);
        assert_eq!(Date::new(2026, 7, 1).unwrap().weekday_index(), 2);
        assert_eq!(Date::new(2025, 1, 1).unwrap().weekday_index(), 2);
        assert_eq!(Date::new(2026, 6, 1).unwrap().weekday_index(), 0);

        for (year, month) in [(2026, 11), (2026, 7), (2025, 1), (2026, 2), (2028, 2)] {
            let grid = month_grid(year, month);
            assert_eq!(grid.cells.len(), 42);
            assert_eq!(
                grid.cells[0].date.weekday_index(),
                0,
                "grid for {year}-{month} must start on a Monday"
            );
            // Leading cells come from the previous month, trailing ones from
            // the next month.
            assert!(!grid.cells[0].in_month);
            assert!(!grid.cells[41].in_month);
            let in_month: Vec<&DayCell> = grid.cells.iter().filter(|c| c.in_month).collect();
            assert_eq!(in_month.len() as u32, Date::days_in_month(year, month));
            assert_eq!(in_month[0].date, MonthYear { year, month }.first_day());
            // The grid contains the day before the first and the day after the
            // last day of the month.
            let first = in_month[0].date;
            let last = in_month[in_month.len() - 1].date;
            assert!(grid.cells.iter().any(|c| c.date == first.add_days(-1)));
            assert!(grid.cells.iter().any(|c| c.date == last.add_days(1)));
        }
    }

    #[test]
    fn month_navigation_crosses_year_boundaries() {
        let december = MonthYear {
            year: 2026,
            month: 12,
        };
        assert_eq!(
            december.shift(1),
            MonthYear {
                year: 2027,
                month: 1
            }
        );
        assert_eq!(december.shift(2).label(), "February 2027");
        let january = MonthYear {
            year: 2027,
            month: 1,
        };
        assert_eq!(
            january.shift(-1),
            MonthYear {
                year: 2026,
                month: 12
            }
        );
        assert_eq!(
            december.shift(-12),
            MonthYear {
                year: 2025,
                month: 12
            }
        );
        assert_eq!(MonthYear::from_date(Date::new(2026, 11, 23).unwrap()).label(), "November 2026");
        assert!(MonthYear { year: 2026, month: 11 }.contains(Date::new(2026, 11, 30).unwrap()));
        assert!(!MonthYear { year: 2026, month: 11 }.contains(Date::new(2026, 12, 1).unwrap()));
    }

    #[test]
    fn time_parsing_and_formatting_round_trip() {
        assert_eq!(parse_time("09:05"), Some(545));
        assert_eq!(parse_time(" 23:59 "), Some(1439));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("9:5x"), None);
        assert_eq!(parse_time("noon"), None);
        assert_eq!(format_minutes(545), "09:05");
        assert_eq!(format_minutes(0), "00:00");
    }

    #[test]
    fn event_crud_and_json_round_trip() {
        let mut store = temp_store("crud");
        assert_eq!(store.load(), Ok(()));
        assert!(store.is_empty());

        let day = Date::new(2026, 11, 23).unwrap();
        let mut event = Event::new("ev-1", "Design review", day, 14 * 60, 15 * 60 + 30)
            .with_notes("Design room");
        store.add(event.clone()).unwrap();
        store.add(Event::new("ev-2", "Stand-up", day, 9 * 60 + 30, 9 * 60 + 45)).unwrap();
        store.save().unwrap();

        let mut reloaded = EventStore::new(store.path().to_path_buf());
        reloaded.load().unwrap();
        assert_eq!(reloaded.len(), 2);
        assert_eq!(reloaded.get("ev-1"), Some(&event));

        let on_day = reloaded.on_day(day);
        assert_eq!(on_day.len(), 2);
        assert_eq!(on_day[0].title, "Stand-up", "events sort by start time");
        assert_eq!(on_day[1].time_label(), "14:00 – 15:30");
        assert_eq!(on_day[1].duration_minutes(), 90);

        // Update.
        event.title = "Design review (moved)".into();
        event.start_minute = 16 * 60;
        event.end_minute = 17 * 60;
        reloaded.update(event.clone()).unwrap();
        assert_eq!(reloaded.get("ev-1").unwrap().title, "Design review (moved)");
        assert_eq!(reloaded.on_day(day)[1].start_minute, 16 * 60);

        // Remove.
        let removed = reloaded.remove("ev-2").unwrap();
        assert_eq!(removed.title, "Stand-up");
        assert!(reloaded.remove("ev-2").is_none());

        let _ = fs::remove_dir_all(store.path().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn duplicate_and_missing_ids_are_rejected() {
        let mut store = temp_store("ids");
        let day = Date::new(2026, 11, 23).unwrap();
        store
            .add(Event::new("ev-1", "First", day, 600, 660))
            .unwrap();
        let duplicate = store.add(Event::new("ev-1", "Second", day, 700, 760));
        assert!(duplicate.is_err(), "duplicate ids are refused");
        assert_eq!(store.len(), 1);

        let missing = store.update(Event::new("ev-9", "Ghost", day, 700, 760));
        assert!(missing.is_err(), "updating a missing event is an error");
        assert!(store.get("ev-9").is_none());

        // Validation: end before start.
        assert!(store
            .add(Event::new("ev-3", "Backwards", day, 800, 700))
            .is_err());
        // Validation: empty title.
        assert!(store.add(Event::new("ev-4", "  ", day, 700, 800)).is_err());

        // Ids are generated without colliding.
        assert_eq!(store.next_id(), "ev-2");
        store
            .add(Event::new("ev-2", "Second", day, 700, 760))
            .unwrap();
        assert_eq!(store.next_id(), "ev-3");
    }

    #[test]
    fn multi_day_events_cover_every_day_in_the_span() {
        let start = Date::new(2026, 11, 23).unwrap();
        let end = Date::new(2026, 11, 25).unwrap();
        let mut store = temp_store("multiday");
        store
            .add(
                Event::new("ev-1", "Release polish", start, 9 * 60, 18 * 60)
                    .with_span(end)
                    .with_color(EventColor::Success),
            )
            .unwrap();

        for offset in 0..3 {
            let day = start.add_days(offset);
            assert_eq!(store.on_day(day).len(), 1, "{day:?} is covered");
            assert!(store.has_events_on(day));
        }
        assert!(!store.has_events_on(start.add_days(-1)));
        assert!(!store.has_events_on(end.add_days(1)));
        let event = store.get("ev-1").unwrap();
        assert!(event.is_multiday());
        assert_eq!(event.day_range_label(), "23 Nov – 25 Nov");
        assert_eq!(event.color, EventColor::Success);
        assert_eq!(event.color.label(), "Green");
    }

    #[test]
    fn corrupt_file_is_reported_and_keeps_loaded_events() {
        let store = temp_store("corrupt");
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let day = Date::new(2026, 11, 23).unwrap();
        fs::write(
            store.path(),
            format!(
                "[{{\"id\":\"ev-1\",\"title\":\"Kept\",\"start_date\":{},\"end_date\":{},\"start_minute\":600,\"end_minute\":660}}]",
                serde_json::to_string(&day).unwrap(),
                serde_json::to_string(&day).unwrap()
            ),
        )
        .unwrap();
        let mut store = EventStore::new(store.path().to_path_buf());
        store.load().unwrap();
        assert_eq!(store.len(), 1);

        fs::write(store.path(), "not json at all").unwrap();
        let error = store.load().unwrap_err();
        assert!(error.contains("not valid calendar JSON"));
        assert_eq!(store.len(), 1, "the previously loaded events survive");

        let _ = fs::remove_dir_all(store.path().parent().unwrap().parent().unwrap());
    }
}
