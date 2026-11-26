//! Calendar controller: the only place that mutates Slint properties (P4-T16).
//!
//! Owns the month view state, the selected day, the persisted event store and
//! the clock tab state machines. Tests build it against a temporary
//! `EventStore` and drive it through the headless element queries.

use std::cell::{Ref, RefCell};
use std::rc::Rc;
use std::time::Duration;

use chrono::Local;
use slint::{ModelRc, VecModel};

use crate::calendar::{
    month_grid, parse_time, week_grid, week_label, Date, Event, EventColor, EventStore,
    MonthYear, WEEKDAY_NAMES, WeekGrid,
};
use crate::clock::{self, Alarm, AlarmStore, Stopwatch, Timer, Zone, ZONES};
use crate::{AlarmView, CalendarApp, DayCellView, EventView, WeekDayView};

pub struct CalendarController {
    ui: CalendarApp,
    store: RefCell<EventStore>,
    alarms: RefCell<AlarmStore>,
    view: RefCell<MonthYear>,
    selected: RefCell<Date>,
    use_24h: RefCell<bool>,
    stopwatch: RefCell<Stopwatch>,
    timer: RefCell<Timer>,
    /// Index into [`ZONES`] for the second clock.
    zone: RefCell<usize>,
    /// Minute that already fired, so one alarm does not ring repeatedly.
    fired_alarms: RefCell<Option<(u32, u32)>>,
    alarm_banner: RefCell<String>,
}

impl CalendarController {
    pub fn new(ui: CalendarApp, store: EventStore) -> Rc<CalendarController> {
        let today = Date::today();
        // Alarms live beside the events file, so a controller built against a
        // temporary store never touches the user's config directory.
        let mut alarms = AlarmStore::new(store.path().with_file_name("alarms.json"));
        alarms.load_or_default();
        let controller = Rc::new(CalendarController {
            ui,
            store: RefCell::new(store),
            alarms: RefCell::new(alarms),
            view: RefCell::new(MonthYear::from_date(today)),
            selected: RefCell::new(today),
            use_24h: RefCell::new(true),
            stopwatch: RefCell::new(Stopwatch::new()),
            timer: RefCell::new(Timer::with_seconds(5 * 60)),
            zone: RefCell::new(0),
            fired_alarms: RefCell::new(None),
            alarm_banner: RefCell::new(String::new()),
        });
        controller.wire();
        controller.sync();
        controller
    }

    /// Demo content for the screenshot renderer and the first-run experience.
    pub fn with_demo_events(ui: CalendarApp, store: EventStore) -> Rc<CalendarController> {
        let controller = CalendarController::new(ui, store);
        controller.seed_demo();
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &CalendarApp {
        &self.ui
    }

    pub fn store(&self) -> Ref<'_, EventStore> {
        self.store.borrow()
    }

    /// Alarms (loaded from `alarms.json` beside the events file).
    pub fn alarms(&self) -> Ref<'_, AlarmStore> {
        self.alarms.borrow()
    }

    pub fn view(&self) -> MonthYear {
        *self.view.borrow()
    }

    /// The seven-day week shown by the Week tab (Monday first).
    pub fn week(&self) -> WeekGrid {
        week_grid(self.selected())
    }

    /// Zone of the second clock.
    pub fn zone(&self) -> Zone {
        ZONES[*self.zone.borrow()]
    }

    /// Text of the ringing-alarm banner (empty when no alarm rang).
    pub fn alarm_banner(&self) -> String {
        self.alarm_banner.borrow().clone()
    }

    pub fn selected(&self) -> Date {
        *self.selected.borrow()
    }

    pub fn events_on_selected(&self) -> usize {
        let day = self.selected();
        self.store.borrow().on_day(day).len()
    }

    /// Advance the clock tab state machines (called by the UI loop and tests)
    /// and ring any alarm that becomes due.
    pub fn tick(&self, delta: Duration) {
        self.stopwatch.borrow_mut().tick(delta);
        self.timer.borrow_mut().tick(delta);
        let (hour, minute, _) = crate::calendar::now_parts();
        self.poll_alarms(Date::today().weekday_index(), hour, minute);
        self.sync_clock();
    }

    /// Ring the enabled alarms that match `hour:minute`, once per minute.
    ///
    /// Returns the labels that started ringing on this call (empty when none),
    /// so both the UI loop and the tests can assert the alarm contract without
    /// waiting for a wall-clock minute.
    pub fn poll_alarms(&self, weekday: usize, hour: u32, minute: u32) -> Vec<String> {
        if *self.fired_alarms.borrow() == Some((hour, minute)) {
            return Vec::new();
        }
        let labels: Vec<String> = self
            .alarms
            .borrow()
            .due(weekday, hour, minute)
            .iter()
            .map(|alarm| format!("{} · {}", alarm.label, alarm.time_label()))
            .collect();
        if labels.is_empty() {
            return Vec::new();
        }
        *self.fired_alarms.borrow_mut() = Some((hour, minute));
        *self.alarm_banner.borrow_mut() = format!("Alarm: {}", labels.join(", "));
        self.sync_clock();
        labels
    }

    /// A few events around today so the month grid, dots and day panel show
    /// real content in screenshots. Kept in memory only: rendering never writes
    /// to the user's config directory.
    fn seed_demo(&self) {
        let today = Date::today();
        let mut store = self.store.borrow_mut();
        let seed = [
            ("Stand-up", today, today, 9 * 60 + 30, 9 * 60 + 45, EventColor::Accent, "Design room"),
            ("Design review", today, today, 14 * 60, 15 * 60 + 30, EventColor::Warning, "Review the AetherOS shell"),
            ("Release polish", today, today.add_days(2), 9 * 60, 18 * 60, EventColor::Success, "Freeze the 0.1.0 image"),
            ("1:1 with Priya", today.add_days(1), today.add_days(1), 11 * 60, 11 * 60 + 30, EventColor::Accent, ""),
            ("Weekly review", today.add_days(-1), today.add_days(-1), 16 * 60, 17 * 60, EventColor::Neutral, ""),
            ("Focus block", today.add_days(4), today.add_days(4), 8 * 60, 12 * 60, EventColor::Danger, ""),
        ];
        for (title, start, end, start_minute, end_minute, color, notes) in seed {
            let event = Event::new(store.next_id(), title, start, start_minute, end_minute)
                .with_span(end)
                .with_color(color)
                .with_notes(notes);
            let _ = store.add(event);
        }
    }

    // --------------------------------------------------------------- sync ---

    pub fn sync(&self) {
        let today = Date::today();
        let view = *self.view.borrow();
        let selected = *self.selected.borrow();
        let grid = month_grid(view.year, view.month);

        let days: Vec<DayCellView> = {
            let store = self.store.borrow();
            grid.cells
                .iter()
                .map(|cell| DayCellView {
                    day: cell.date.day as i32,
                    in_month: cell.in_month,
                    today: cell.date == today,
                    weekend: cell.date.is_weekend(),
                    has_events: store.has_events_on(cell.date),
                    selected: cell.date == selected,
                    iso: cell.date.iso().into(),
                })
                .collect()
        };

        let events: Vec<EventView> = self
            .store
            .borrow()
            .on_day(selected)
            .iter()
            .map(|event| EventView {
                id: event.id.clone().into(),
                title: event.title.clone().into(),
                time: event.time_label().into(),
                note: event.notes.clone().into(),
                kind: event.color.code(),
                multiday: event.is_multiday(),
                day_label: event.day_range_label().into(),
            })
            .collect();

        let count = events.len();
        self.ui.set_month_label(view.label().into());
        self.ui.set_today_label(today.long_label().into());
        self.ui.set_selected_label(selected.long_label().into());
        self.ui.set_selected_count_label(
            format!("{count} event{}", if count == 1 { "" } else { "s" }).into(),
        );
        self.ui.set_days(ModelRc::new(VecModel::from(days)));
        self.ui.set_events(ModelRc::new(VecModel::from(events)));

        // Week tab: the Monday-first week of the selected day, with the day's
        // event titles listed per column.
        let week = week_grid(selected);
        let week_days: Vec<WeekDayView> = week
            .cells
            .iter()
            .map(|cell| {
                let titles: Vec<slint::SharedString> = self
                    .store
                    .borrow()
                    .on_day(cell.date)
                    .iter()
                    .take(3)
                    .map(|event| slint::SharedString::from(&event.title))
                    .collect();
                WeekDayView {
                    weekday: WEEKDAY_NAMES[cell.date.weekday_index()][..3]
                        .to_uppercase()
                        .into(),
                    date: cell.date.short_label().into(),
                    day: cell.date.day as i32,
                    today: cell.date == today,
                    selected: cell.date == selected,
                    iso: cell.date.iso().into(),
                    events: ModelRc::new(VecModel::from(titles)),
                }
            })
            .collect();
        self.ui
            .set_week_label(week_label(week.start).into());
        self.ui
            .set_week_days(ModelRc::new(VecModel::from(week_days)));

        self.sync_clock();
    }

    fn sync_clock(&self) {
        let (hour, minute, second) = crate::calendar::now_parts();
        let use_24h = *self.use_24h.borrow();
        let angles = clock::analog_angles(hour, minute, second);
        self.ui
            .set_clock_digital(clock::digital(hour, minute, second, use_24h).into());
        self.ui
            .set_clock_date(clock::long_date(Local::now().date_naive()).into());
        self.ui.set_dial_command(clock::dial_path(200.0).into());
        self.ui
            .set_hands_command(clock::hands_path(200.0, angles).into());
        self.ui
            .set_second_command(clock::second_path(200.0, angles.second).into());

        let stopwatch = self.stopwatch.borrow();
        self.ui
            .set_stopwatch_text(clock::format_stopwatch(stopwatch.elapsed()).into());
        self.ui.set_stopwatch_running(stopwatch.is_running());
        let laps: Vec<slint::SharedString> = stopwatch
            .laps()
            .iter()
            .map(|lap| clock::format_stopwatch(*lap).into())
            .collect();
        self.ui.set_laps(ModelRc::new(VecModel::from(laps)));

        let timer = self.timer.borrow();
        self.ui
            .set_timer_text(clock::format_countdown(timer.remaining()).into());
        self.ui.set_timer_progress(timer.progress());
        self.ui.set_timer_running(timer.is_running());
        drop(timer);

        // Time zones: the list plus the selected zone's clock.
        let zone_index = *self.zone.borrow();
        let zone = ZONES[zone_index];
        let (zone_hour, zone_minute, zone_second) =
            clock::offset_clock(hour, minute, second, zone.offset_minutes);
        let zone_names: Vec<slint::SharedString> =
            ZONES.iter().map(|zone| zone.name.into()).collect();
        self.ui.set_zones(ModelRc::new(VecModel::from(zone_names)));
        self.ui.set_zone(zone_index as i32);
        let zone_label = if zone.name == zone.offset_label() {
            zone.name.to_string()
        } else {
            format!("{} · {}", zone.name, zone.offset_label())
        };
        self.ui.set_zone_label(zone_label.into());
        self.ui
            .set_zone_time(clock::digital(zone_hour, zone_minute, zone_second, use_24h).into());

        // Alarms and the ringing banner.
        let alarm_rows: Vec<AlarmView> = self
            .alarms
            .borrow()
            .alarms()
            .iter()
            .map(|alarm| AlarmView {
                id: alarm.id.clone().into(),
                label: alarm.label.clone().into(),
                time: alarm.time_label().into(),
                days: alarm.day_label().into(),
                enabled: alarm.enabled,
            })
            .collect();
        self.ui.set_alarms(ModelRc::new(VecModel::from(alarm_rows)));
        self.ui
            .set_alarm_banner(self.alarm_banner.borrow().clone().into());
    }

    // ------------------------------------------------------------ actions ---

    fn navigate(&self, months: i32) {
        let today = Date::today();
        let next = self.view.borrow().shift(months);
        *self.view.borrow_mut() = next;
        {
            let mut selected = self.selected.borrow_mut();
            if !next.contains(*selected) {
                *selected = if next.contains(today) {
                    today
                } else {
                    next.first_day()
                };
            }
        }
        self.sync();
    }

    fn go_today(&self) {
        let today = Date::today();
        *self.view.borrow_mut() = MonthYear::from_date(today);
        *self.selected.borrow_mut() = today;
        self.sync();
    }

    fn select_day(&self, iso: &str) {
        let Some(date) = Date::parse_iso(iso) else {
            return;
        };
        *self.selected.borrow_mut() = date;
        if !self.view.borrow().contains(date) {
            *self.view.borrow_mut() = MonthYear::from_date(date);
        }
        self.sync();
    }

    fn add_event(&self) {
        let title = self.ui.get_new_title().to_string();
        let start = parse_time(&self.ui.get_new_start().to_string());
        let end = parse_time(&self.ui.get_new_end().to_string());
        let (Some(start_minute), Some(end_minute)) = (start, end) else {
            self.ui
                .set_form_error("Times must look like 09:30 (24-hour).".into());
            return;
        };
        if title.trim().is_empty() {
            self.ui.set_form_error("Give the event a title.".into());
            return;
        }
        if start_minute >= end_minute {
            self.ui.set_form_error("The end time must be after the start.".into());
            return;
        }

        let day = *self.selected.borrow();
        let mut store = self.store.borrow_mut();
        let color = EventColor::ALL[store.len() % EventColor::ALL.len()];
        let event = Event::new(store.next_id(), title.trim(), day, start_minute, end_minute)
            .with_color(color);
        match store.add(event) {
            Ok(()) => {
                self.ui.set_new_title("".into());
                self.ui.set_form_error("".into());
                if let Err(error) = store.save() {
                    self.ui.set_form_error(error.into());
                }
            }
            Err(error) => self.ui.set_form_error(error.into()),
        }
        drop(store);
        self.sync();
    }

    fn remove_event(&self, id: &str) {
        let mut store = self.store.borrow_mut();
        if store.remove(id).is_some() {
            if let Err(error) = store.save() {
                self.ui.set_form_error(error.into());
            }
        }
        drop(store);
        self.sync();
    }

    fn shift_week(&self, days: i64) {
        let next = self.selected.borrow().add_days(days);
        *self.selected.borrow_mut() = next;
        *self.view.borrow_mut() = MonthYear::from_date(next);
        self.sync();
    }

    fn add_alarm(&self) {
        let label = self.ui.get_new_alarm_label().to_string();
        let Some(minutes) = parse_time(&self.ui.get_new_alarm_time().to_string()) else {
            self.ui
                .set_form_error("Alarm time must look like 07:30 (24-hour).".into());
            return;
        };
        let label = if label.trim().is_empty() {
            "Alarm".to_string()
        } else {
            label.trim().to_string()
        };
        let mut alarms = self.alarms.borrow_mut();
        let alarm = Alarm::new(alarms.next_id(), label, minutes / 60, minutes % 60);
        match alarms.add(alarm) {
            Ok(()) => {
                self.ui.set_new_alarm_label("".into());
                self.ui.set_form_error("".into());
                if let Err(error) = alarms.save() {
                    self.ui.set_form_error(error.into());
                }
            }
            Err(error) => self.ui.set_form_error(error.into()),
        }
        drop(alarms);
        self.sync();
    }

    fn toggle_alarm(&self, id: &str) {
        let mut alarms = self.alarms.borrow_mut();
        if alarms.toggle(id).is_some() {
            if let Err(error) = alarms.save() {
                self.ui.set_form_error(error.into());
            }
        }
        drop(alarms);
        self.sync_clock();
    }

    fn remove_alarm(&self, id: &str) {
        let mut alarms = self.alarms.borrow_mut();
        if alarms.remove(id).is_some() {
            if let Err(error) = alarms.save() {
                self.ui.set_form_error(error.into());
            }
        }
        drop(alarms);
        self.sync_clock();
    }

    fn dismiss_alarm(&self) {
        self.alarm_banner.borrow_mut().clear();
        self.sync_clock();
    }

    fn wire(self: &Rc<Self>) {
        let with = |f: fn(&CalendarController)| {
            let weak = Rc::downgrade(self);
            move || {
                if let Some(controller) = weak.upgrade() {
                    f(&controller);
                }
            }
        };
        self.ui.on_prev_month(with(Self::handle_prev_month));
        self.ui.on_next_month(with(Self::handle_next_month));
        self.ui.on_prev_week(with(Self::handle_prev_week));
        self.ui.on_next_week(with(Self::handle_next_week));
        self.ui.on_alarm_add(with(Self::add_alarm));
        self.ui.on_alarm_dismiss(with(Self::dismiss_alarm));
        self.ui.on_today(with(Self::handle_today));
        self.ui.on_add_event(with(Self::add_event));
        self.ui.on_stopwatch_toggle(with(Self::handle_stopwatch_toggle));
        self.ui.on_stopwatch_lap(with(Self::handle_stopwatch_lap));
        self.ui.on_stopwatch_reset(with(Self::handle_stopwatch_reset));
        self.ui.on_timer_toggle(with(Self::handle_timer_toggle));
        self.ui.on_timer_reset(with(Self::handle_timer_reset));
        {
            let weak = Rc::downgrade(self);
            self.ui.on_day_selected(move |iso| {
                if let Some(controller) = weak.upgrade() {
                    controller.select_day(iso.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_remove_event(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.remove_event(id.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_toggle_format(move |use_24h| {
                if let Some(controller) = weak.upgrade() {
                    *controller.use_24h.borrow_mut() = use_24h;
                    controller.sync_clock();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_timer_add(move |minutes| {
                if let Some(controller) = weak.upgrade() {
                    controller.timer.borrow_mut().add_seconds(minutes.max(0) as u64 * 60);
                    controller.sync_clock();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_alarm_toggle(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.toggle_alarm(id.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_alarm_remove(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.remove_alarm(id.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_zone_selected(move |index| {
                if let Some(controller) = weak.upgrade() {
                    let index = index.max(0) as usize;
                    *controller.zone.borrow_mut() = index.min(ZONES.len() - 1);
                    controller.sync_clock();
                }
            });
        }
    }

    fn handle_prev_week(&self) {
        self.shift_week(-7);
    }

    fn handle_next_week(&self) {
        self.shift_week(7);
    }

    fn handle_prev_month(&self) {
        self.navigate(-1);
    }

    fn handle_next_month(&self) {
        self.navigate(1);
    }

    fn handle_today(&self) {
        self.go_today();
    }

    fn handle_stopwatch_toggle(&self) {
        self.stopwatch.borrow_mut().toggle();
        self.sync_clock();
    }

    fn handle_stopwatch_lap(&self) {
        self.stopwatch.borrow_mut().lap();
        self.sync_clock();
    }

    fn handle_stopwatch_reset(&self) {
        self.stopwatch.borrow_mut().reset();
        self.sync_clock();
    }

    fn handle_timer_toggle(&self) {
        self.timer.borrow_mut().toggle();
        self.sync_clock();
    }

    fn handle_timer_reset(&self) {
        self.timer.borrow_mut().reset();
        self.sync_clock();
    }
}
