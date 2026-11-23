//! Battery time-remaining estimator (P5-T11, D5 section 6).
//!
//! - one sample per UPower update (10–30 s),
//! - moving average of the energy rate over a 2–5 minute window,
//! - "Estimating..." for the first 2 minutes after a source change,
//! - samples more than 3× the average are rejected unless three in a row.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::types::{BatteryStatus, Estimator, PowerSource};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EstimatorConfig {
    /// Moving-average window (D5: 2–5 minutes).
    pub window: Duration,
    /// No estimate before this much time after a source change.
    pub warmup: Duration,
    /// A sample above this multiple of the average is a spike.
    pub spike_factor: f32,
    /// Number of consecutive spikes that make the new rate real.
    pub spike_streak: usize,
    /// Minimum average rate to avoid dividing by zero.
    pub min_rate_w: f32,
}

impl Default for EstimatorConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(300),
            warmup: Duration::from_secs(120),
            spike_factor: 3.0,
            spike_streak: 3,
            min_rate_w: 0.1,
        }
    }
}

#[derive(Debug, Clone)]
struct Sample {
    at: Instant,
    rate_w: f32,
}

/// Moving-average estimator.
#[derive(Debug)]
pub struct RateEstimator {
    config: EstimatorConfig,
    samples: VecDeque<Sample>,
    last_source: Option<PowerSource>,
    source_changed_at: Option<Instant>,
    last_update: Option<Instant>,
    last_energy_wh: Option<f32>,
    spike_streak: usize,
    rejected: usize,
}

impl RateEstimator {
    pub fn new(config: EstimatorConfig) -> Self {
        Self {
            config,
            samples: VecDeque::new(),
            last_source: None,
            source_changed_at: None,
            last_update: None,
            last_energy_wh: None,
            spike_streak: 0,
            rejected: 0,
        }
    }

    pub fn config(&self) -> EstimatorConfig {
        self.config
    }

    pub fn average_rate(&self) -> Option<f32> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: f32 = self.samples.iter().map(|s| s.rate_w).sum();
        Some(sum / self.samples.len() as f32)
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn rejected_samples(&self) -> usize {
        self.rejected
    }

    pub fn is_estimating(&self) -> bool {
        self.time_to_empty().is_none()
    }

    fn prune(&mut self, now: Instant) {
        while let Some(front) = self.samples.front() {
            if now.saturating_duration_since(front.at) > self.config.window {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }
}

impl Estimator for RateEstimator {
    fn update(&mut self, sample: &BatteryStatus, at: Instant) {
        if self.last_source != Some(sample.source) {
            self.last_source = Some(sample.source);
            self.source_changed_at = Some(at);
            self.samples.clear();
            self.spike_streak = 0;
        }
        self.last_update = Some(at);
        if let Some(energy) = sample.energy_now_wh {
            self.last_energy_wh = Some(energy);
        }

        let Some(rate) = sample.energy_rate_w else {
            return;
        };
        if rate < 0.0 {
            return;
        }

        let average = self.average_rate();
        let is_spike = average
            .map(|avg| avg > self.config.min_rate_w && rate > avg * self.config.spike_factor)
            .unwrap_or(false);

        if is_spike {
            self.spike_streak += 1;
            self.rejected += 1;
            if self.spike_streak < self.config.spike_streak {
                return;
            }
            // The high rate persisted: accept it as a real load change.
            self.spike_streak = 0;
        } else {
            self.spike_streak = 0;
        }

        self.samples.push_back(Sample { at, rate_w: rate });
        self.prune(at);
    }

    fn time_to_empty(&self) -> Option<Duration> {
        let now = self.last_update?;
        let changed = self.source_changed_at?;
        if now.saturating_duration_since(changed) < self.config.warmup {
            return None;
        }
        // Prune against `now` even when no new sample arrived.
        let mut sum = 0.0;
        let mut count = 0usize;
        for sample in &self.samples {
            if now.saturating_duration_since(sample.at) <= self.config.window {
                sum += sample.rate_w;
                count += 1;
            }
        }
        if count < 2 {
            return None;
        }
        let average = sum / count as f32;
        if average <= self.config.min_rate_w {
            return None;
        }
        let energy = self.last_energy_wh?;
        let hours = energy / average;
        Some(Duration::from_secs_f32(hours * 3600.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(source: PowerSource, energy_wh: f32, rate_w: f32) -> BatteryStatus {
        BatteryStatus {
            percent: 80.0,
            source,
            charging: false,
            energy_rate_w: Some(rate_w),
            energy_now_wh: Some(energy_wh),
            ..Default::default()
        }
    }

    #[test]
    fn estimates_within_ten_percent_after_ten_minutes() {
        let mut estimator = RateEstimator::new(EstimatorConfig::default());
        let start = Instant::now();
        // 50 Wh at 10 W = 5 h.
        estimator.update(&sample(PowerSource::Battery, 50.0, 10.0), start);
        for minute in 1..=10u64 {
            // 10 W over 10 minutes uses ~1.67 Wh.
            estimator.update(
                &sample(
                    PowerSource::Battery,
                    50.0 - minute as f32 * 10.0 / 60.0,
                    10.0,
                ),
                start + Duration::from_secs(minute * 60),
            );
        }
        let estimate = estimator.time_to_empty().expect("estimate after 10 min");
        let expected = Duration::from_secs(5 * 3600);
        let diff = estimate.abs_diff(expected);
        assert!(
            diff.as_secs_f32() <= expected.as_secs_f32() * 0.10,
            "estimate {estimate:?} not within 10% of {expected:?}"
        );
    }

    #[test]
    fn reports_estimating_during_warmup() {
        let mut estimator = RateEstimator::new(EstimatorConfig::default());
        let start = Instant::now();
        estimator.update(&sample(PowerSource::Battery, 50.0, 10.0), start);
        estimator.update(
            &sample(PowerSource::Battery, 49.0, 10.0),
            start + Duration::from_secs(60),
        );
        assert!(estimator.time_to_empty().is_none());
        assert!(estimator.is_estimating());
    }

    #[test]
    fn rejects_a_single_spike() {
        let mut estimator = RateEstimator::new(EstimatorConfig::default());
        let start = Instant::now();
        for minute in 0..5u64 {
            estimator.update(
                &sample(PowerSource::Battery, 50.0, 10.0),
                start + Duration::from_secs(minute * 60),
            );
        }
        let before = estimator.average_rate().unwrap();
        estimator.update(
            &sample(PowerSource::Battery, 50.0, 100.0),
            start + Duration::from_secs(300),
        );
        assert_eq!(estimator.rejected_samples(), 1);
        assert!((estimator.average_rate().unwrap() - before).abs() < 0.01);
    }

    #[test]
    fn accepts_three_consecutive_spikes() {
        let mut estimator = RateEstimator::new(EstimatorConfig::default());
        let start = Instant::now();
        for minute in 0..5u64 {
            estimator.update(
                &sample(PowerSource::Battery, 50.0, 10.0),
                start + Duration::from_secs(minute * 60),
            );
        }
        for step in 0..3u64 {
            estimator.update(
                &sample(PowerSource::Battery, 50.0, 40.0),
                start + Duration::from_secs(300 + step * 60),
            );
        }
        assert!(estimator.average_rate().unwrap() > 10.0);
    }

    #[test]
    fn source_change_restarts_the_warmup() {
        let mut estimator = RateEstimator::new(EstimatorConfig::default());
        let start = Instant::now();
        for minute in 0..10u64 {
            estimator.update(
                &sample(PowerSource::Battery, 50.0, 10.0),
                start + Duration::from_secs(minute * 60),
            );
        }
        assert!(estimator.time_to_empty().is_some());
        estimator.update(
            &sample(PowerSource::Ac, 50.0, 10.0),
            start + Duration::from_secs(600),
        );
        assert!(estimator.time_to_empty().is_none());
    }
}
