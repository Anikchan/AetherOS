//! AetherOS desktop shell (P4, S3).
//!
//! * `ui/surfaces.slint` — menu bar, dock, launcher, control centre,
//!   notification centre, lock screen, overview, settings app and the composed
//!   `ShellRoot` surface host.
//! * [`model`] — all shell rules as pure Rust (dock order, Focus suppression,
//!   overlay toggling, credentials, power modes).
//! * [`search`] — launcher ranking plus the inline calculator.
//! * [`config`] — F4 settings schema and layered TOML persistence.
//! * [`desktop`] — `.desktop` discovery for the dock and the launcher.
//! * [`services`] — nmcli / bluetoothctl / wpctl / brightnessctl / sysfs battery
//!   adapters, with a mock for tests.
//! * [`app`] — `ShellController`, the only place that mutates Slint properties.

slint::include_modules!();

pub mod app;
pub mod config;
pub mod desktop;
pub mod model;
pub mod render;
pub mod search;
pub mod services;

pub use app::ShellController;
pub use model::{Overlay, PowerMode, ShellState};
