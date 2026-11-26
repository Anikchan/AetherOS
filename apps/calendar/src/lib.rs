//! AetherOS Calendar & Clock default app (P4-T16, DA-07).
//!
//! * [`calendar`] — month and week grids, navigation and the JSON event store.
//! * [`clock`] — digital/analogue clock maths, time zones, alarms, stopwatch
//!   and countdown timer.
//! * [`app`] — `CalendarController`, the only place that mutates Slint
//!   properties.

slint::include_modules!();

pub mod app;
pub mod calendar;
pub mod clock;

pub use app::CalendarController;
pub use calendar::{
    month_grid, week_grid, week_label, Date, Event, EventColor, EventStore, MonthGrid, MonthYear,
    WeekGrid,
};
pub use clock::{Alarm, AlarmStore, Stopwatch, Timer, Zone, ZONES};
