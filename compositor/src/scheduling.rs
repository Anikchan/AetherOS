//! P3-T06 frame scheduling and damage tracking.
//!
//! Pure timing/geometry model (no Wayland/DRM) so it unit-tests on the dev host
//! and in CI. `AetherCompositor` feeds it the damage it observes in
//! `wl_surface.commit`, asks it once per event-loop tick whether a frame is due,
//! sends the `wl_surface.frame` callbacks for the presented frame, and reports
//! `AETHER_COMPOSITOR_FRAME` / `AETHER_COMPOSITOR_IDLE` /
//! `AETHER_COMPOSITOR_FRAME_STATS` markers.
//!
//! Three behaviours the design asks for:
//!
//! - **Damage-based repaint**: only the union of the damaged rects is composed;
//!   [`DamageRegion`] coalesces overlapping rects and reports the union area so
//!   the glass pipeline blurs just that part (`glass::EffectPlan`).
//! - **Frame callbacks**: callbacks are only signalled when a frame is actually
//!   presented for that surface, so clients throttle to the compositor's rate.
//! - **Idle measurement**: with no damage no frame is submitted at all; the
//!   scheduler tracks idle time and skipped frames so GPU idle use can be
//!   reported (the hardware part of the T06 acceptance gate).

use std::time::{Duration, Instant};

use crate::wm::Rect;

/// Accumulated damage rects since the last presented frame.
#[derive(Debug, Default, Clone)]
pub struct DamageRegion {
    rects: Vec<Rect>,
    generation: u64,
}

impl DamageRegion {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a damage rect. Empty rects are ignored; overlapping rects are
    /// coalesced into their bounding box so the region stays small.
    pub fn add(&mut self, rect: Rect) {
        if rect.w <= 0 || rect.h <= 0 {
            return;
        }
        self.rects.push(rect);
        self.coalesce();
    }

    fn coalesce(&mut self) {
        let mut merged = true;
        while merged {
            merged = false;
            'outer: for left in 0..self.rects.len() {
                for right in (left + 1)..self.rects.len() {
                    if intersects(self.rects[left], self.rects[right]) {
                        let union = bounding_box(self.rects[left], self.rects[right]);
                        self.rects[left] = union;
                        self.rects.remove(right);
                        merged = true;
                        break 'outer;
                    }
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Union area in logical pixels (overlaps counted once).
    pub fn area(&self) -> u64 {
        union_area(&self.rects)
    }

    /// Incremented every time the region is drained (one per presented frame).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Take the pending damage, leaving the region empty.
    pub fn take(&mut self) -> Vec<Rect> {
        self.generation += 1;
        std::mem::take(&mut self.rects)
    }


    pub fn clear(&mut self) {
        self.rects.clear();
    }
}

/// Union area of a set of rects, counting overlapping pixels once
/// (coordinate compression; the sets here are a handful of rects).
pub fn union_area(rects: &[Rect]) -> u64 {
    let mut xs: Vec<i32> = Vec::with_capacity(rects.len() * 2);
    let mut ys: Vec<i32> = Vec::with_capacity(rects.len() * 2);
    for rect in rects.iter().filter(|rect| rect.w > 0 && rect.h > 0) {
        xs.push(rect.x);
        xs.push(rect.x + rect.w);
        ys.push(rect.y);
        ys.push(rect.y + rect.h);
    }
    if xs.is_empty() {
        return 0;
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();

    let mut total = 0u64;
    for window_x in xs.windows(2) {
        for window_y in ys.windows(2) {
            let cell_w = window_x[1] - window_x[0];
            let cell_h = window_y[1] - window_y[0];
            if cell_w <= 0 || cell_h <= 0 {
                continue;
            }
            let covered = rects.iter().any(|rect| {
                rect.x <= window_x[0]
                    && rect.y <= window_y[0]
                    && rect.x + rect.w >= window_x[1]
                    && rect.y + rect.h >= window_y[1]
            });
            if covered {
                total += cell_w as u64 * cell_h as u64;
            }
        }
    }
    total
}

/// One presented frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub index: u64,
    pub damage: Vec<Rect>,
    pub damage_pixels: u64,
    /// Time since the previous frame; `None` for the first frame.
    pub gap_ms: Option<f64>,
    /// Time the damage waited between the commit that created it and this
    /// frame (`0.0` for a frame with no newly-added damage, e.g. a carry-over).
    /// P3-T08 frame-gap logger consumes it.
    pub damage_wait_ms: f64,
    /// Presentation timestamp in milliseconds since compositor start.
    pub time_ms: u32,
}

/// Vsync-ish frame pacer with damage tracking and idle accounting.
#[derive(Debug)]
pub struct FrameScheduler {
    pub target_fps: u32,
    interval: Duration,
    pub damage: DamageRegion,
    started: Instant,
    pub frames: u64,
    pub idle_skips: u64,
    pub presented_pixels: u64,
    pub last_frame: Option<Instant>,
    /// When the first not-yet-presented damage rect was added, for the P3-T08
    /// damage-wait (frame-gap) measurement.
    first_damage: Option<Instant>,
    last_gap_ms: f64,
    total_gap_ms: f64,
    idle_since: Option<Instant>,
    idle_total: Duration,
    idle_reported: bool,
}

impl FrameScheduler {
    pub fn new(target_fps: u32) -> Self {
        let target_fps = target_fps.max(1);
        Self {
            target_fps,
            interval: Duration::from_micros(1_000_000 / target_fps as u64),
            damage: DamageRegion::new(),
            started: Instant::now(),
            frames: 0,
            idle_skips: 0,
            presented_pixels: 0,
            last_frame: None,
            first_damage: None,
            last_gap_ms: 0.0,
            total_gap_ms: 0.0,
            idle_since: None,
            idle_total: Duration::ZERO,
            idle_reported: false,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn add_damage(&mut self, rect: Rect) {
        let was_empty = self.damage.is_empty();
        self.damage.add(rect);
        if was_empty && !self.damage.is_empty() {
            self.first_damage = Some(Instant::now());
        }
        // New damage ends the current idle span.
        self.close_idle_span(Instant::now());
        self.idle_reported = false;
    }

    /// Account one tick where no frame could be scheduled (no damage, or the
    /// frame interval has not elapsed yet).
    pub fn note_idle(&mut self, now: Instant) {
        if !self.has_damage() {
            self.idle_skips += 1;
            self.mark_idle(now);
        }
    }

    pub fn has_damage(&self) -> bool {
        !self.damage.is_empty()
    }

    /// A frame is due when there is damage and the frame interval elapsed.
    pub fn due(&self, now: Instant) -> bool {
        if !self.has_damage() {
            return false;
        }
        match self.last_frame {
            Some(last) => now.duration_since(last) >= self.interval,
            None => true,
        }
    }

    /// Present the pending damage as one frame. Returns `None` when there is
    /// nothing to present (counted as an idle skip).
    pub fn present(&mut self, now: Instant) -> Option<Frame> {
        if !self.has_damage() {
            self.idle_skips += 1;
            self.mark_idle(now);
            return None;
        }
        let damage = self.damage.take();
        let damage_pixels = union_area(&damage);
        let damage_wait_ms = self
            .first_damage
            .take()
            .map(|since| now.saturating_duration_since(since).as_secs_f64() * 1_000.0)
            .unwrap_or(0.0);
        let gap_ms = self
            .last_frame
            .map(|last| now.duration_since(last).as_secs_f64() * 1_000.0);
        if let Some(gap_ms) = gap_ms {
            self.last_gap_ms = gap_ms;
            self.total_gap_ms += gap_ms;
        }
        self.close_idle_span(now);
        self.idle_reported = false;
        self.last_frame = Some(now);
        self.frames += 1;
        self.presented_pixels += damage_pixels;
        Some(Frame {
            index: self.frames,
            damage,
            damage_pixels,
            gap_ms,
            damage_wait_ms,
            time_ms: now.duration_since(self.started).as_millis() as u32,
        })
    }

    fn mark_idle(&mut self, now: Instant) {
        if self.idle_since.is_none() {
            self.idle_since = Some(now);
        }
    }

    fn close_idle_span(&mut self, now: Instant) {
        if let Some(since) = self.idle_since.take() {
            self.idle_total += now.duration_since(since);
        }
    }

    /// Idle time accumulated so far, including the current open idle span.
    pub fn idle_time(&self, now: Instant) -> Duration {
        let open = self
            .idle_since
            .map(|since| now.duration_since(since))
            .unwrap_or(Duration::ZERO);
        self.idle_total + open
    }

    /// Fraction of wall time spent with no frame submitted (0.0 = always busy).
    pub fn idle_ratio(&self, now: Instant) -> f64 {
        let elapsed = now.duration_since(self.started).as_secs_f64();
        if elapsed <= 0.0 {
            return 0.0;
        }
        (self.idle_time(now).as_secs_f64() / elapsed).clamp(0.0, 1.0)
    }

    /// Estimated GPU idle percentage: time we did not composite at all. Real
    /// GPU counters replace this once the graphics stack is on hardware.
    pub fn estimated_gpu_idle_percent(&self, now: Instant) -> f64 {
        self.idle_ratio(now) * 100.0
    }

    /// True once per idle span, so the caller can print one marker per span
    /// instead of one per skipped frame.
    pub fn take_idle_report(&mut self, now: Instant) -> bool {
        if self.idle_since.is_none() {
            return false;
        }
        let idle = self.idle_time(now);
        if idle >= self.interval && !self.idle_reported {
            self.idle_reported = true;
            return true;
        }
        false
    }

    /// Mean inter-frame gap over presented frames (0.0 before the first gap).
    pub fn average_gap_ms(&self) -> f64 {
        if self.frames > 1 {
            self.total_gap_ms / (self.frames - 1) as f64
        } else {
            0.0
        }
    }

    pub fn last_gap_ms(&self) -> f64 {
        self.last_gap_ms
    }

    pub fn stats_line(&self, now: Instant) -> String {
        format!(
            "frames={} idle_skips={} idle_ms={} idle_pct={:.1} presented_px={} avg_gap_ms={:.2} last_gap_ms={:.2} target_fps={}",
            self.frames,
            self.idle_skips,
            self.idle_time(now).as_millis(),
            self.estimated_gpu_idle_percent(now),
            self.presented_pixels,
            self.average_gap_ms(),
            self.last_gap_ms,
            self.target_fps
        )
    }
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn bounding_box(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_damage_is_coalesced() {
        let mut damage = DamageRegion::new();
        damage.add(Rect { x: 0, y: 0, w: 100, h: 100 });
        damage.add(Rect { x: 50, y: 50, w: 100, h: 100 });
        assert_eq!(damage.len(), 1, "overlapping rects merge");
        assert_eq!(damage.rects()[0], Rect { x: 0, y: 0, w: 150, h: 150 });
        assert_eq!(damage.area(), 150 * 150);
    }

    #[test]
    fn disjoint_damage_stays_separate() {
        let mut damage = DamageRegion::new();
        damage.add(Rect { x: 0, y: 0, w: 10, h: 10 });
        damage.add(Rect { x: 100, y: 100, w: 10, h: 10 });
        assert_eq!(damage.len(), 2);
        assert_eq!(damage.area(), 200);
    }

    #[test]
    fn empty_rects_are_ignored() {
        let mut damage = DamageRegion::new();
        damage.add(Rect { x: 0, y: 0, w: 0, h: 10 });
        damage.add(Rect { x: 0, y: 0, w: 10, h: -1 });
        assert!(damage.is_empty());
        assert_eq!(damage.area(), 0);
    }

    #[test]
    fn union_area_counts_overlap_once() {
        let rects = [
            Rect { x: 0, y: 0, w: 100, h: 100 },
            Rect { x: 50, y: 0, w: 100, h: 100 },
            Rect { x: 0, y: 0, w: 10, h: 10 },
        ];
        // Union is 150 wide x 100 tall.
        assert_eq!(union_area(&rects), 15_000);
        assert_eq!(union_area(&[]), 0);
    }

    #[test]
    fn take_clears_the_region_and_bumps_the_generation() {
        let mut damage = DamageRegion::new();
        damage.add(Rect { x: 0, y: 0, w: 4, h: 4 });
        assert_eq!(damage.generation(), 0);
        let taken = damage.take();
        assert_eq!(taken.len(), 1);
        assert!(damage.is_empty());
        assert_eq!(damage.generation(), 1);
    }

    #[test]
    fn no_damage_means_no_frame_and_no_gap() {
        let mut scheduler = FrameScheduler::new(60);
        let start = Instant::now();
        assert!(!scheduler.due(start));
        assert_eq!(scheduler.present(start), None);
        assert_eq!(scheduler.frames, 0);
        assert_eq!(scheduler.idle_skips, 1);
        assert_eq!(scheduler.average_gap_ms(), 0.0);
    }

    #[test]
    fn frame_is_paced_to_the_target_interval() {
        let mut scheduler = FrameScheduler::new(60);
        assert_eq!(scheduler.interval(), Duration::from_micros(16_666));
        let start = Instant::now();
        scheduler.add_damage(Rect { x: 0, y: 0, w: 100, h: 100 });
        assert!(scheduler.due(start));
        let frame = scheduler.present(start).expect("first frame presents");
        assert_eq!(frame.index, 1);
        assert_eq!(frame.damage_pixels, 10_000);
        assert!(frame.gap_ms.is_none());

        // Too early for the next frame even with damage.
        scheduler.add_damage(Rect { x: 0, y: 0, w: 10, h: 10 });
        assert!(!scheduler.due(start + Duration::from_millis(5)));
        assert!(scheduler.due(start + Duration::from_millis(17)));
        let second = scheduler
            .present(start + Duration::from_millis(17))
            .expect("second frame presents");
        assert_eq!(second.index, 2);
        let gap = second.gap_ms.expect("gap between frames");
        assert!((gap - 17.0).abs() < 0.5, "gap {gap}");
        assert!((scheduler.average_gap_ms() - 17.0).abs() < 0.5);
    }

    #[test]
    fn damage_after_an_idle_span_closes_it() {
        let mut scheduler = FrameScheduler::new(60);
        let start = Instant::now();
        // 100 ms idle.
        assert_eq!(scheduler.present(start), None);
        let idle_at = start + Duration::from_millis(100);
        let idle = scheduler.idle_time(idle_at);
        assert!(idle >= Duration::from_millis(100), "idle {idle:?}");
        assert!(scheduler.idle_ratio(idle_at) > 0.9);
        assert!(scheduler.estimated_gpu_idle_percent(idle_at) > 90.0);

        scheduler.add_damage(Rect { x: 0, y: 0, w: 8, h: 8 });
        scheduler.present(idle_at).expect("frame after idle");
        assert!(
            scheduler.idle_time(idle_at + Duration::from_millis(1))
                <= Duration::from_millis(101),
            "idle span stops accumulating once a frame is presented"
        );
    }

    #[test]
    fn idle_is_reported_once_per_span() {
        let mut scheduler = FrameScheduler::new(60);
        let start = Instant::now();
        scheduler.present(start);
        assert!(!scheduler.take_idle_report(start));
        let later = start + Duration::from_millis(50);
        assert!(scheduler.take_idle_report(later));
        assert!(!scheduler.take_idle_report(later), "reported only once");
        scheduler.add_damage(Rect { x: 0, y: 0, w: 4, h: 4 });
        assert!(!scheduler.take_idle_report(later), "damage ends the idle span");
    }

    #[test]
    fn damage_wait_is_measured_from_the_commit() {
        let mut scheduler = FrameScheduler::new(60);
        scheduler.add_damage(Rect { x: 0, y: 0, w: 10, h: 10 });
        std::thread::sleep(Duration::from_millis(20));
        let frame = scheduler.present(Instant::now()).expect("frame");
        assert!(
            frame.damage_wait_ms >= 10.0,
            "damage waited {:?} ms",
            frame.damage_wait_ms
        );
        // No new damage: the waiting time resets once the damage is presented.
        scheduler.add_damage(Rect { x: 0, y: 0, w: 4, h: 4 });
        let frame = scheduler.present(Instant::now()).expect("second frame");
        assert!(frame.damage_wait_ms < 10.0);
    }

    #[test]
    fn stats_line_reports_the_measurements() {
        let mut scheduler = FrameScheduler::new(60);
        let start = Instant::now();
        scheduler.add_damage(Rect { x: 0, y: 0, w: 20, h: 10 });
        scheduler.present(start);
        let line = scheduler.stats_line(start + Duration::from_millis(10));
        assert!(line.contains("frames=1"), "{line}");
        assert!(line.contains("presented_px=200"), "{line}");
        assert!(line.contains("target_fps=60"), "{line}");
    }
}
