//! Playback transport state for the Player app (P4-T14, DA-AC-06).
//!
//! State machine: `Stopped -> Playing <-> Paused -> Stopped`. Position only
//! advances through [`Transport::tick`], which reports when a track ends so the
//! player can auto-advance. Everything is deterministic: the tests never sleep.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

impl PlaybackState {
    pub fn label(self) -> &'static str {
        match self {
            PlaybackState::Stopped => "Stopped",
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
        }
    }

    /// Stable code for the .slint surface.
    pub fn code(self) -> i32 {
        match self {
            PlaybackState::Stopped => 0,
            PlaybackState::Playing => 1,
            PlaybackState::Paused => 2,
        }
    }
}

/// What a [`Transport::tick`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// Nothing to do (stopped or paused).
    Idle,
    /// Position moved forward.
    Progressed,
    /// The track reached its end and playback paused there.
    Ended,
}

#[derive(Debug, Clone)]
pub struct Transport {
    state: PlaybackState,
    position: Duration,
    duration: Duration,
    volume: f32,
    muted: bool,
}

impl Default for Transport {
    fn default() -> Self {
        Transport {
            state: PlaybackState::Stopped,
            position: Duration::ZERO,
            duration: Duration::ZERO,
            volume: 0.7,
            muted: false,
        }
    }
}

impl Transport {
    pub fn new() -> Transport {
        Transport::default()
    }

    /// Load a track: position resets to zero and playback stops.
    pub fn load(&mut self, duration: Duration) {
        self.duration = duration;
        self.position = Duration::ZERO;
        self.state = PlaybackState::Stopped;
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    pub fn position(&self) -> Duration {
        self.position
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Start (or resume) playback. A zero-length track stays stopped.
    pub fn play(&mut self) {
        if self.duration.is_zero() {
            return;
        }
        if self.position >= self.duration {
            self.position = Duration::ZERO;
        }
        self.state = PlaybackState::Playing;
    }

    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
    }

    pub fn toggle(&mut self) {
        if self.state == PlaybackState::Playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position = Duration::ZERO;
    }

    /// Seek, clamped to `0..=duration`.
    pub fn seek(&mut self, position: Duration) -> Duration {
        self.position = position.min(self.duration);
        self.position
    }

    /// Seek by fraction of the track (the slider sends `0.0..=1.0`).
    pub fn seek_fraction(&mut self, fraction: f32) -> Duration {
        let fraction = fraction.clamp(0.0, 1.0);
        self.seek(self.duration.mul_f32(fraction))
    }

    /// Advance the position. Returns [`TickOutcome::Ended`] on the tick that
    /// reaches the end of the track; the caller decides what plays next.
    pub fn tick(&mut self, delta: Duration) -> TickOutcome {
        if self.state != PlaybackState::Playing {
            return TickOutcome::Idle;
        }
        let next = self.position + delta;
        if next >= self.duration {
            self.position = self.duration;
            self.state = PlaybackState::Paused;
            TickOutcome::Ended
        } else {
            self.position = next;
            TickOutcome::Progressed
        }
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    /// Volume the engine should use (zero while muted).
    pub fn effective_volume(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }

    /// 0.0 at the start, 1.0 at the end; 0.0 for an empty track.
    pub fn progress(&self) -> f32 {
        if self.duration.is_zero() {
            0.0
        } else {
            (self.position.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
        }
    }

    pub fn elapsed_label(&self) -> String {
        format_time(self.position)
    }

    pub fn total_label(&self) -> String {
        format_time(self.duration)
    }
}

/// `m:ss` below an hour, `h:mm:ss` from one hour.
pub fn format_time(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_formatting_switches_at_one_hour() {
        assert_eq!(format_time(Duration::ZERO), "0:00");
        assert_eq!(format_time(Duration::from_secs(5)), "0:05");
        assert_eq!(format_time(Duration::from_secs(65)), "1:05");
        assert_eq!(format_time(Duration::from_secs(232)), "3:52");
        assert_eq!(format_time(Duration::from_secs(600)), "10:00");
        assert_eq!(format_time(Duration::from_secs(3665)), "1:01:05");
        assert_eq!(format_time(Duration::from_secs(3600)), "1:00:00");
    }

    #[test]
    fn transport_seek_and_volume_are_clamped() {
        let mut transport = Transport::new();
        transport.load(Duration::from_secs(200));
        assert_eq!(transport.state(), PlaybackState::Stopped);

        assert_eq!(transport.seek(Duration::from_secs(500)), Duration::from_secs(200));
        assert_eq!(transport.seek_fraction(0.25), Duration::from_secs(50));
        assert_eq!(transport.seek_fraction(2.0), Duration::from_secs(200));
        assert_eq!(transport.seek_fraction(-1.0), Duration::ZERO);
        assert_eq!(transport.progress(), 0.0);

        transport.set_volume(2.0);
        assert_eq!(transport.volume(), 1.0);
        transport.set_volume(-1.0);
        assert_eq!(transport.volume(), 0.0);
        transport.set_volume(0.42);
        assert_eq!(transport.effective_volume(), 0.42);
        transport.toggle_mute();
        assert!(transport.is_muted());
        assert_eq!(transport.effective_volume(), 0.0);
        transport.toggle_mute();
        assert_eq!(transport.effective_volume(), 0.42);
        assert_eq!(transport.elapsed_label(), "0:00");
        assert_eq!(transport.total_label(), "3:20");
    }

    #[test]
    fn playback_state_machine() {
        let mut transport = Transport::new();
        transport.load(Duration::from_secs(120));
        assert_eq!(transport.tick(Duration::from_secs(5)), TickOutcome::Idle, "stopped");

        transport.play();
        assert_eq!(transport.state(), PlaybackState::Playing);
        assert_eq!(transport.tick(Duration::from_secs(5)), TickOutcome::Progressed);
        assert_eq!(transport.position(), Duration::from_secs(5));

        transport.pause();
        assert_eq!(transport.state(), PlaybackState::Paused);
        assert_eq!(transport.tick(Duration::from_secs(5)), TickOutcome::Idle);
        assert_eq!(transport.position(), Duration::from_secs(5), "paused freezes");

        transport.toggle();
        assert_eq!(transport.state(), PlaybackState::Playing);
        transport.stop();
        assert_eq!(transport.state(), PlaybackState::Stopped);
        assert_eq!(transport.position(), Duration::ZERO);

        // An empty track never starts.
        let mut empty = Transport::new();
        empty.play();
        assert_eq!(empty.state(), PlaybackState::Stopped);
    }

    #[test]
    fn reaching_the_end_pauses_once_and_reports_it() {
        let mut transport = Transport::new();
        transport.load(Duration::from_secs(10));
        transport.play();
        transport.tick(Duration::from_secs(6));
        assert_eq!(transport.progress(), 0.6);
        assert_eq!(transport.tick(Duration::from_secs(6)), TickOutcome::Ended);
        assert_eq!(transport.position(), Duration::from_secs(10), "clamped to duration");
        assert_eq!(transport.state(), PlaybackState::Paused);
        assert_eq!(transport.progress(), 1.0);
        assert_eq!(transport.tick(Duration::from_secs(1)), TickOutcome::Idle, "only once");

        // Replaying a finished track starts from the beginning.
        transport.play();
        assert_eq!(transport.position(), Duration::ZERO);
        assert_eq!(transport.state(), PlaybackState::Playing);
    }
}
