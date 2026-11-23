//! AetherOS Player default app (P4-T14, DA-AC-06).
//!
//! * [`playlist`] — queue model, seeded shuffle, repeat modes.
//! * [`transport`] — playback state machine, position, volume, formatting.
//! * [`engine`] — `Engine` trait, GStreamer adapter and the mock engine, plus
//!   the codec policy note (H.264/AAC out of the base image, P0-T03).
//! * [`app`] — `PlayerController`, the only place that mutates Slint
//!   properties.

slint::include_modules!();

pub mod app;
pub mod engine;
pub mod playlist;
pub mod transport;

pub use app::{demo_playlist, PlayerController};
pub use engine::{Engine, EngineError, GStreamerEngine, MockEngine};
pub use playlist::{Playlist, Repeat, Track};
pub use transport::{format_time, PlaybackState, TickOutcome, Transport};
