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
    month_grid, parse_time, Date, Event, EventColor, EventStore, MonthYear,
};
use crate::clock::{self, Stopwatch, Timer};
use crate::{CalendarApp, DayCellView, EventView};

pub struct CalendarController {
    ui: CalendarApp,
    store: RefCell<EventStore>,
    view: RefCell<MonthYear>,
    selected: RefCell<Date>,
    use_24h: RefCell<bool>,
    stopwatch: RefCell<Stopwatch>,
    timer: RefCell<Timer>,
}

impl CalendarController {
    pub fn new(ui: CalendarApp, store: EventStore) -> Rc<CalendarController> {
        let today = Date::today();
        let controller = Rc::new(CalendarController {
            ui,
            store: RefCell::new(store),
            view: RefCell::new(MonthYear::from_date(today)),
            selected: RefCell::new(today),
            use_24h: RefCell::new(true),
            stopwatch: RefCell::new(Stopwatch::new()),
            timer: RefCell::new(Timer::with_seconds(5 * 60)),
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

    pub fn view(&self) -> MonthYear {
        *self.view.borrow()
    }

    pub fn selected(&self) -> Date {
        *self.selected.borrow()
    }

    pub fn events_on_selected(&self) -> usize {
        let day = self.selected();
        self.store.borrow().on_day(day).len()
    }

    /// Advance the clock tab state machines (called by the UI loop and tests).
    pub fn tick(&self, delta: Duration) {
        self.stopwatch.borrow_mut().tick(delta);
        self.timer.borrow_mut().tick(delta);
        self.sync_clock();
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
