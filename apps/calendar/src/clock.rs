//! Clock core (P4-T16, DA-07): digital and analogue time, stopwatch and
//! countdown timer.
//!
//! The module is pure Rust and time is passed in (or ticked by a delta) so the
//! symmetry, lap and expiry rules are unit-testable without sleeping. The
//! analogue path helpers return SVG command strings for the Slint `Path`
//! elements, so the face geometry comes from the same angle math the tests
//! check.

use std::time::Duration;

/// One line of the analogue dial plus both hands, drawn in a 24x24-ish viewbox
/// scaled by the caller (`dial_path(size)` uses a `size` x `size` viewbox).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandAngles {
    /// Degrees clockwise from 12 o'clock.
    pub hour: f32,
    pub minute: f32,
    pub second: f32,
}

/// Clock hand angles for a wall-clock time.
///
/// The hour hand advances continuously with the minutes (`0.5°` per minute) and
/// the minute hand with the seconds (`0.1°` per second), which is what a real
/// clock does.
pub fn analog_angles(hour: u32, minute: u32, second: u32) -> HandAngles {
    let hour = hour % 12;
    let minute = minute.min(59);
    let second = second.min(59);
    HandAngles {
        hour: hour as f32 * 30.0 + minute as f32 * 0.5 + second as f32 / 120.0,
        minute: minute as f32 * 6.0 + second as f32 * 0.1,
        second: second as f32 * 6.0,
    }
}

/// End point of a hand of `radius` at `angle` degrees, as an offset from the
/// centre (y grows downwards, like Slint/SVG).
pub fn hand_endpoint(angle_deg: f32, radius: f32) -> (f32, f32) {
    let radians = angle_deg.to_radians();
    (radius * radians.sin(), -radius * radians.cos())
}

fn path_segment(from: (f32, f32), to: (f32, f32)) -> String {
    format!(
        "M {:.2} {:.2} L {:.2} {:.2}",
        from.0, from.1, to.0, to.1
    )
}

/// Twelve hour ticks on a face of `size` logical pixels.
pub fn dial_path(size: f32) -> String {
    let centre = size / 2.0;
    let inner = size * 0.415;
    let outer = size * 0.462;
    (0..12)
        .map(|hour| {
            let angle = hour as f32 * 30.0;
            let (sx, sy) = hand_endpoint(angle, inner);
            let (ex, ey) = hand_endpoint(angle, outer);
            path_segment((centre + sx, centre + sy), (centre + ex, centre + ey))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hour and minute hands from the centre to their tips.
pub fn hands_path(size: f32, angles: HandAngles) -> String {
    let centre = size / 2.0;
    let (hour_x, hour_y) = hand_endpoint(angles.hour, size * 0.24);
    let (minute_x, minute_y) = hand_endpoint(angles.minute, size * 0.34);
    format!(
        "{} {}",
        path_segment(
            (centre, centre),
            (centre + hour_x, centre + hour_y)
        ),
        path_segment(
            (centre, centre),
            (centre + minute_x, centre + minute_y)
        )
    )
}

/// Second hand with a short counterweight tail.
pub fn second_path(size: f32, angle_deg: f32) -> String {
    let centre = size / 2.0;
    let (tip_x, tip_y) = hand_endpoint(angle_deg, size * 0.38);
    let (tail_x, tail_y) = hand_endpoint(angle_deg, -size * 0.08);
    path_segment(
        (centre + tail_x, centre + tail_y),
        (centre + tip_x, centre + tip_y),
    )
}

/// `09:41:07` in 24-hour mode, `9:41:07 AM` in 12-hour mode.
pub fn digital(hour: u32, minute: u32, second: u32, use_24h: bool) -> String {
    let hour = hour.min(23);
    let minute = minute.min(59);
    let second = second.min(59);
    if use_24h {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else {
        let suffix = if hour < 12 { "AM" } else { "PM" };
        let display = match hour % 12 {
            0 => 12,
            other => other,
        };
        format!("{display}:{minute:02}:{second:02} {suffix}")
    }
}

/// `Monday, 23 November 2026` for the clock face header.
pub fn long_date(date: chrono::NaiveDate) -> String {
    date.format("%A, %-d %B %Y").to_string()
}

/// `mm:ss.d` — stopwatch precision (tenths, like the reference app).
pub fn format_stopwatch(elapsed: Duration) -> String {
    let total_tenths = elapsed.as_millis() / 100;
    let minutes = total_tenths / 600;
    let seconds = (total_tenths / 10) % 60;
    let tenths = total_tenths % 10;
    format!("{minutes:02}:{seconds:02}.{tenths}")
}

/// `mm:ss` for the countdown timer (rounded up, so it never shows 00:00 early).
pub fn format_countdown(remaining: Duration) -> String {
    let seconds = remaining.as_secs() + if remaining.subsec_millis() > 0 { 1 } else { 0 };
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Stopwatch state machine: start/pause, laps and reset.
///
/// Time only advances through [`Stopwatch::tick`] so tests are deterministic;
/// the interactive binary drives it from a one-second `slint::Timer`.
#[derive(Debug, Clone, Default)]
pub struct Stopwatch {
    elapsed: Duration,
    lap_start: Duration,
    running: bool,
    laps: Vec<Duration>,
}

impl Stopwatch {
    pub fn new() -> Stopwatch {
        Stopwatch::default()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub fn laps(&self) -> &[Duration] {
        &self.laps
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn toggle(&mut self) {
        self.running = !self.running;
    }

    /// Advance by `delta`; ignored while paused or stopped.
    pub fn tick(&mut self, delta: Duration) {
        if self.running {
            self.elapsed += delta;
        }
    }

    /// Record a lap split (time since the previous lap). Returns `None` when the
    /// stopwatch is not running.
    pub fn lap(&mut self) -> Option<Duration> {
        if !self.running {
            return None;
        }
        let split = self.elapsed.saturating_sub(self.lap_start);
        self.lap_start = self.elapsed;
        self.laps.push(split);
        Some(split)
    }

    /// Back to zero, stopped, laps cleared.
    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.lap_start = Duration::ZERO;
        self.running = false;
        self.laps.clear();
    }
}

/// Countdown timer state machine.
#[derive(Debug, Clone)]
pub struct Timer {
    total: Duration,
    remaining: Duration,
    running: bool,
    finished: bool,
}

impl Timer {
    pub fn new(total: Duration) -> Timer {
        Timer {
            total,
            remaining: total,
            running: false,
            finished: false,
        }
    }

    pub fn with_seconds(seconds: u64) -> Timer {
        Timer::new(Duration::from_secs(seconds))
    }

    pub fn total(&self) -> Duration {
        self.total
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn start(&mut self) {
        if self.finished || self.remaining.is_zero() {
            self.remaining = self.total;
            self.finished = false;
        }
        self.running = true;
    }

    pub fn pause(&mut self) {
        self.running = false;
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.pause();
        } else {
            self.start();
        }
    }

    /// Advance the countdown. Returns `true` on the tick that expires it.
    pub fn tick(&mut self, delta: Duration) -> bool {
        if !self.running || self.finished {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(delta);
        if self.remaining.is_zero() {
            self.running = false;
            self.finished = true;
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.remaining = self.total;
        self.running = false;
        self.finished = false;
    }

    /// Extend (or shorten with `secs` capped at the total) the countdown.
    pub fn add_seconds(&mut self, seconds: u64) {
        self.remaining += Duration::from_secs(seconds);
        self.total = self.total.max(self.remaining);
        self.finished = false;
    }

    /// 0.0 at the start, 1.0 when expired.
    pub fn progress(&self) -> f32 {
        if self.total.is_zero() {
            return 1.0;
        }
        let elapsed = self.total.saturating_sub(self.remaining).as_secs_f32();
        (elapsed / self.total.as_secs_f32()).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digital_formatting_covers_12_and_24_hour_modes() {
        assert_eq!(digital(9, 41, 7, true), "09:41:07");
        assert_eq!(digital(0, 0, 0, true), "00:00:00");
        assert_eq!(digital(23, 59, 59, true), "23:59:59");
        assert_eq!(digital(9, 41, 7, false), "9:41:07 AM");
        assert_eq!(digital(12, 5, 0, false), "12:05:00 PM");
        assert_eq!(digital(0, 30, 15, false), "12:30:15 AM");
        assert_eq!(digital(13, 30, 0, false), "1:30:00 PM");
        assert_eq!(digital(99, 0, 0, true), "23:00:00", "out-of-range hours clamp");
    }

    #[test]
    fn analog_angles_follow_clock_arithmetic() {
        // 3:00 -> hour hand exactly at 3 o'clock (90°), minute hand at 12.
        let three = analog_angles(3, 0, 0);
        assert_eq!(three.hour, 90.0);
        assert_eq!(three.minute, 0.0);
        assert_eq!(three.second, 0.0);

        // 9:30 -> hour hand halfway between 9 and 10, minute hand at 6 (180°).
        let half_past = analog_angles(9, 30, 0);
        assert_eq!(half_past.hour, 285.0);
        assert_eq!(half_past.minute, 180.0);

        // 12:00:30 -> hour hand creeps by 0.25°, second hand at 180°.
        let late = analog_angles(12, 0, 30);
        assert!((late.hour - 0.25).abs() < f32::EPSILON);
        assert!((late.second - 180.0).abs() < f32::EPSILON);

        // 18:45:15 wraps to the 12-hour dial.
        let evening = analog_angles(18, 45, 15);
        assert_eq!(evening.hour, 6.0 * 30.0 + 45.0 * 0.5 + 15.0 / 120.0);
        assert_eq!(evening.minute, 271.5);
        assert_eq!(evening.second, 90.0);
    }

    #[test]
    fn hand_endpoints_and_paths_are_geometrically_sane() {
        // 12 o'clock points up (negative y), 3 o'clock points right.
        let (x, y) = hand_endpoint(0.0, 10.0);
        assert!(x.abs() < 1e-4 && (y + 10.0).abs() < 1e-4);
        let (x, y) = hand_endpoint(90.0, 10.0);
        assert!((x - 10.0).abs() < 1e-4 && y.abs() < 1e-4);

        let dial = dial_path(200.0);
        assert_eq!(dial.matches("M ").count(), 12, "twelve hour ticks");
        let hands = hands_path(200.0, analog_angles(1, 0, 0));
        assert_eq!(hands.matches("M ").count(), 2, "hour + minute hands");
        let seconds = second_path(200.0, 180.0);
        assert!(seconds.contains("M "));
        assert!(seconds.contains("L "));
    }

    #[test]
    fn stopwatch_ignores_time_while_paused_and_records_laps() {
        let mut stopwatch = Stopwatch::new();
        stopwatch.tick(Duration::from_secs(5));
        assert_eq!(stopwatch.elapsed(), Duration::ZERO, "stopped watch never advances");

        stopwatch.start();
        stopwatch.tick(Duration::from_millis(1500));
        assert_eq!(stopwatch.elapsed(), Duration::from_millis(1500));
        assert_eq!(stopwatch.lap(), Some(Duration::from_millis(1500)));

        stopwatch.tick(Duration::from_millis(500));
        assert_eq!(stopwatch.lap(), Some(Duration::from_millis(500)));
        assert_eq!(stopwatch.laps().len(), 2);

        stopwatch.stop();
        assert_eq!(stopwatch.lap(), None, "no laps while paused");
        stopwatch.tick(Duration::from_secs(10));
        assert_eq!(stopwatch.elapsed(), Duration::from_secs(2), "paused watch freezes");

        stopwatch.reset();
        assert_eq!(stopwatch.elapsed(), Duration::ZERO);
        assert!(stopwatch.laps().is_empty());
        assert!(!stopwatch.is_running());
    }

    #[test]
    fn stopwatch_formats_tenths() {
        assert_eq!(format_stopwatch(Duration::ZERO), "00:00.0");
        assert_eq!(format_stopwatch(Duration::from_millis(12_300)), "00:12.3");
        assert_eq!(format_stopwatch(Duration::from_secs(65)), "01:05.0");
        assert_eq!(format_stopwatch(Duration::from_secs(3600)), "60:00.0");
    }

    #[test]
    fn countdown_timer_expires_once_and_can_restart() {
        let mut timer = Timer::with_seconds(3);
        assert_eq!(format_countdown(timer.remaining()), "00:03");
        assert!(!timer.tick(Duration::from_secs(1)), "paused timer does not advance");

        timer.start();
        assert!(!timer.tick(Duration::from_secs(1)));
        assert_eq!(timer.remaining(), Duration::from_secs(2));
        assert!(!timer.tick(Duration::from_secs(1)));
        assert!(timer.tick(Duration::from_secs(5)), "the tick past zero expires it");
        assert!(timer.is_finished());
        assert!(!timer.is_running());
        assert_eq!(timer.remaining(), Duration::ZERO);
        assert_eq!(format_countdown(timer.remaining()), "00:00");
        assert_eq!(timer.progress(), 1.0);
        assert!(!timer.tick(Duration::from_secs(1)), "an expired timer stays expired");

        // Starting again restarts from the full duration.
        timer.start();
        assert_eq!(timer.remaining(), Duration::from_secs(3));
        assert!(!timer.is_finished());
        assert_eq!(timer.progress(), 0.0);
        assert_eq!(format_countdown(Duration::from_millis(1500)), "00:02", "rounds up");
    }
}
