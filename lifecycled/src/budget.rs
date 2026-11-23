//! Background task budget (P5-T06, D3 section 3 and F2 exceptions).
//!
//! Protected apps keep running while in the background, but only audio and
//! call protection also block reclaim. Everything else that keeps running in
//! the background draws from a per-window CPU-time budget; when a repeated
//! offender exhausts it, the policy lowers its `cpu.weight` so the foreground
//! app keeps the machine responsive.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::identity::AppId;

/// Per-app budget for background CPU time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundBudgetConfig {
    /// CPU-time allowance per app in one window (default 60 s per 15 min).
    pub allowance: Duration,
    /// Rolling window (default 15 min).
    pub window: Duration,
    /// CPU weight applied to background apps (systemd default is 100).
    pub background_weight: u32,
    /// CPU weight applied when an app exhausts its allowance.
    pub throttled_weight: u32,
}

impl Default for BackgroundBudgetConfig {
    fn default() -> Self {
        Self {
            allowance: Duration::from_secs(60),
            window: Duration::from_secs(15 * 60),
            background_weight: 50,
            throttled_weight: 10,
        }
    }
}

#[derive(Debug, Clone)]
struct Usage {
    used: Duration,
    window_started: Instant,
}

/// Tracks background CPU time per app, with a fixed window reset.
#[derive(Debug)]
pub struct BackgroundBudget {
    config: BackgroundBudgetConfig,
    usage: HashMap<AppId, Usage>,
}

impl BackgroundBudget {
    pub fn new(config: BackgroundBudgetConfig) -> Self {
        Self {
            config,
            usage: HashMap::new(),
        }
    }

    pub fn config(&self) -> BackgroundBudgetConfig {
        self.config
    }

    fn roll_window(window: Duration, usage: &mut Usage, now: Instant) {
        if now.saturating_duration_since(usage.window_started) >= window {
            usage.used = Duration::ZERO;
            usage.window_started = now;
        }
    }

    /// Record background CPU time. Returns `true` when the app is now over
    /// its allowance and should be throttled.
    pub fn consume(&mut self, id: &AppId, cpu_time: Duration, now: Instant) -> bool {
        let usage = self.usage.entry(id.clone()).or_insert(Usage {
            used: Duration::ZERO,
            window_started: now,
        });
        Self::roll_window(self.config.window, usage, now);
        usage.used += cpu_time;
        usage.used > self.config.allowance
    }

    pub fn used(&self, id: &AppId) -> Duration {
        self.usage.get(id).map(|u| u.used).unwrap_or_default()
    }

    pub fn exhausted(&self, id: &AppId) -> bool {
        self.usage
            .get(id)
            .map(|u| u.used > self.config.allowance)
            .unwrap_or(false)
    }

    pub fn forget(&mut self, id: &AppId) {
        self.usage.remove(id);
    }

    /// Weight to apply for an app in the background.
    pub fn weight_for(&self, id: &AppId) -> u32 {
        if self.exhausted(id) {
            self.config.throttled_weight
        } else {
            self.config.background_weight
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> AppId {
        AppId::new("org.example.Mail").unwrap()
    }

    #[test]
    fn allowance_is_enforced_and_reset_per_window() {
        let mut budget = BackgroundBudget::new(BackgroundBudgetConfig {
            allowance: Duration::from_secs(10),
            window: Duration::from_secs(60),
            ..Default::default()
        });
        let start = Instant::now();
        assert!(!budget.consume(&id(), Duration::from_secs(5), start));
        assert!(!budget.consume(&id(), Duration::from_secs(5), start));
        assert!(budget.consume(&id(), Duration::from_secs(1), start));
        assert!(budget.exhausted(&id()));
        assert_eq!(budget.weight_for(&id()), 10);

        // New window: allowance is restored.
        let later = start + Duration::from_secs(61);
        assert!(!budget.consume(&id(), Duration::from_secs(1), later));
        assert!(!budget.exhausted(&id()));
        assert_eq!(budget.weight_for(&id()), 50);
    }

    #[test]
    fn apps_have_independent_budgets() {
        let mut budget = BackgroundBudget::new(BackgroundBudgetConfig {
            allowance: Duration::from_secs(1),
            window: Duration::from_secs(60),
            ..Default::default()
        });
        let now = Instant::now();
        let a = AppId::new("org.example.A").unwrap();
        let b = AppId::new("org.example.B").unwrap();
        budget.consume(&a, Duration::from_secs(2), now);
        assert!(budget.exhausted(&a));
        assert!(!budget.exhausted(&b));
        budget.forget(&a);
        assert!(!budget.exhausted(&a));
    }
}
