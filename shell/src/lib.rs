//! AetherOS desktop shell (P4, S3).
//!
//! * `ui/surfaces.slint` — menu bar, dock, launcher, control centre,
//!   notification centre, lock screen, overview, settings app, the greeter login
//!   surface and the composed `ShellRoot`/`GreeterRoot` hosts.
//! * [`model`] — all shell rules as pure Rust (dock order, Focus suppression,
//!   overlay toggling, credentials, power modes).
//! * [`search`] — launcher ranking plus the inline calculator.
//! * [`config`] — F4 settings schema and layered TOML persistence.
//! * [`effects`] — effect-tier control and the compositor's tier/frame markers
//!   (P4-T04 ↔ P3-T05).
//! * `a11y` — accessibility settings: reduce motion, reduce transparency and
//!   text scaling (F1 SHL-18).
//! * `kbdnav` — keyboard focus across the shell surfaces (F1 SHL-18).
//! * [`desktop`] — `.desktop` discovery for the dock and the launcher.
//! * [`services`] — nmcli / bluetoothctl / wpctl / brightnessctl / sysfs battery
//!   adapters, with a mock for tests.
//! * [`app`] — `ShellController`, the only place that mutates Slint properties.
//! * [`greetd`] — greetd IPC protocol and client (P4-T06).
//! * [`greeter`] — login state machine and greeter configuration (P4-T06).
//! * [`auth`] — lock-screen authenticators: PAM (Linux, feature `pam`) or the
//!   local credential prototype (P4-T06).
//! * [`greeter_app`] — `GreeterController` for the standalone login surface.
//! * [`features`] — Settings → Features page over `aether-config`'s optional
//!   feature manager (P7-T07).
//! * [`host`] — layer-shell host contract: request plan, size negotiation and
//!   the `--host-check` verification of the compositor's markers (P3-T02).
//! * [`nohang`] — G-NH2: the "Not responding" dialog (Wait / Force Quit) over
//!   the compositor's health markers, plus the shell restart ledger and safe
//!   mode after three failed starts (P3-T08).
//! * [`runtime`] — the interactive shell host: follows the compositor journal,
//!   drives the dialog/watchdog state machines on a timer, presents the Slint
//!   surfaces and owns the start/confirm ledger handshake.

slint::include_modules!();

pub mod app;
pub mod a11y;
pub mod auth;
pub mod config;
pub mod desktop;
pub mod effects;
pub mod features;
pub mod greetd;
pub mod greeter;
pub mod host;
pub mod kbdnav;
pub mod keys;
pub mod layers;
pub mod greeter_app;
pub mod model;
pub mod nohang;
pub mod render;
pub mod runtime;
pub mod search;
pub mod services;
pub mod spawn;
pub mod wayland;

pub use app::ShellController;
pub use auth::{default_authenticator, AuthOutcome, Authenticator};
pub use features::FeaturesPage;
pub use greeter_app::GreeterController;
pub use greetd::GreetdConnection;
pub use host::{plan as layer_plan, BindingReport};
pub use keys::{KeyEvent, KeyInput};
pub use layers::Keyboard;
pub use model::{Overlay, PowerMode, ShellState};
pub use nohang::{DialogAction, StartLedger};
pub use runtime::{FileJournal, HostConfig, HostRun, Journal, MemoryJournal, ShellHost};
pub use spawn::{launch_argv, ProcessSpawner, Spawner};
