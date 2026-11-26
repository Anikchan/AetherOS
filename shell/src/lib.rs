//! AetherOS desktop shell (P4, S3).
//!
//! * `ui/surfaces.slint` — menu bar, dock, launcher, control centre,
//!   notification centre, lock screen, overview, settings app, the greeter login
//!   surface and the composed `ShellRoot`/`GreeterRoot` hosts.
//! * [`model`] — all shell rules as pure Rust (dock order, Focus suppression,
//!   overlay toggling, credentials, power modes).
//! * [`search`] — launcher ranking plus the inline calculator.
//! * [`config`] — F4 settings schema and layered TOML persistence.
//! * [`desktop`] — `.desktop` discovery for the dock and the launcher.
//! * [`services`] — nmcli / bluetoothctl / wpctl / brightnessctl / sysfs battery
//!   adapters, with a mock for tests.
//! * [`app`] — `ShellController`, the only place that mutates Slint properties.
//! * [`greetd`] — greetd IPC protocol and client (P4-T06).
//! * [`greeter`] — login state machine and greeter configuration (P4-T06).
//! * [`auth`] — lock-screen authenticators: PAM (Linux, feature `pam`) or the
//!   local credential prototype (P4-T06).
//! * [`greeter_app`] — `GreeterController` for the standalone login surface.

slint::include_modules!();

pub mod app;
pub mod auth;
pub mod config;
pub mod desktop;
pub mod greetd;
pub mod greeter;
pub mod greeter_app;
pub mod model;
pub mod render;
pub mod search;
pub mod services;

pub use app::ShellController;
pub use auth::{default_authenticator, AuthOutcome, Authenticator};
pub use greeter_app::GreeterController;
pub use greetd::GreetdConnection;
pub use model::{Overlay, PowerMode, ShellState};
