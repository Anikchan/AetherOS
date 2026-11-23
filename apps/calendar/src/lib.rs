//! AetherOS Calendar & Clock default app (P4-T16, DA-07).
//!
//! * [`calendar`] — month grids, navigation and the JSON event store.
//! * [`clock`] — digital/analogue clock maths, stopwatch and countdown timer.
//! * [`app`] — `CalendarController`, the only place that mutates Slint
//!   properties.

slint::include_modules!();

pub mod app;
pub mod calendar;
pub mod clock;

pub use app::CalendarController;
pub use calendar::{
    month_grid, Date, Event, EventColor, EventStore, MonthGrid, MonthYear,
};
pub use clock::{Stopwatch, Timer};
