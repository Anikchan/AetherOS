//! P3-T08 no-hang design (addendum 1, F11 section 3).
//!
//! Pure state machines (no Wayland/DRM) so they unit-test on the dev host and
//! in CI. Four pieces, matching the NH-01..NH-07 rules:
//!
//! - [`HangDetector`] — the standard Wayland ping/pong watchdog per shell
//!   client (NH-01, NH-02). The compositor sends a ping and keeps rendering;
//!   the reply is a normal event and nothing in the render path waits for it.
//!   A client that does not reply within the timeout moves to
//!   `NotResponding` (default 5 s, the acceptance limit is 6 s) and the shell
//!   shows "Not responding" with Wait / Force Quit. A later pong moves it back
//!   to `Responsive` (recovery).
//! - [`FrameGapLogger`] — logs every frame that spent longer than the threshold
//!   in the damage queue before presentation (NH-07). Idle time is not a gap:
//!   only pending damage measured from the commit that created it counts, so an
//!   idle desktop does not produce false hangs.
//! - [`ShellWatchdog`] — commit freshness of the shell's layer surfaces
//!   (NH-03). The compositor keeps the windows when the shell is restarted by
//!   the service manager; this reports the stall so the restart is visible in
//!   the log.
//! - [`HangDetector::clients_to_ping`] — the asynchronous update drive: pings
//!   are queued per client on an interval, never blocking a frame.
//!
//! Markers (printed by `AetherCompositor`):
//! - `AETHER_COMPOSITOR_PING: client=… serial=…`
//! - `AETHER_COMPOSITOR_PONG: client=… serial=… rtt_ms=… recovered=…`
//! - `AETHER_COMPOSITOR_APP_STATE: client=… state=not_responding after_ms=…`
//! - `AETHER_COMPOSITOR_FRAME_GAP: ms=… threshold_ms=… gaps=… max_ms=…`
//! - `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout|ok …`
//! - `AETHER_COMPOSITOR_NO_HANG_SUMMARY: …`

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Wayland ping timeout: 5 s per NH-02 (the acceptance limit is 6 s).
pub const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a responsive client is pinged again.
pub const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(2);
/// A frame waiting more than 1 s in the damage queue is a logged gap (NH-07).
pub const DEFAULT_FRAME_GAP: Duration = Duration::from_secs(1);
/// The shell must commit a layer surface at least this often (NH-03).
pub const DEFAULT_SHELL_WATCHDOG: Duration = Duration::from_secs(5);

/// Responsiveness of one app/client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    /// Last ping was answered (or the client was just registered).
    Responsive,
    /// A ping is in flight; waiting for the pong.
    PingPending,
    /// The ping timed out: show "Not responding".
    NotResponding,
}

impl AppState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Responsive => "responsive",
            Self::PingPending => "ping_pending",
            Self::NotResponding => "not_responding",
        }
    }
}

/// A state change the compositor reports as a marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub client: u64,
    pub from: AppState,
    pub to: AppState,
    /// Time since the unanswered ping was sent.
    pub after: Duration,
}

/// The reply to a ping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pong {
    pub client: u64,
    pub serial: u32,
    pub rtt: Duration,
    /// The client was `NotResponding` before this pong.
    pub recovered: bool,
}

#[derive(Debug, Clone)]
struct ClientHealth {
    state: AppState,
    pending: Option<(u32, Instant)>,
    last_ping_at: Option<Instant>,
    not_responding_since: Option<Instant>,
    pings: u64,
    pongs: u64,
    last_rtt: Option<Duration>,
}

/// Per-shell-client ping/pong watchdog.
#[derive(Debug)]
pub struct HangDetector {
    timeout: Duration,
    clients: HashMap<u64, ClientHealth>,
}

impl HangDetector {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            clients: HashMap::new(),
        }
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn register(&mut self, client: u64) {
        self.clients.entry(client).or_insert(ClientHealth {
            state: AppState::Responsive,
            pending: None,
            last_ping_at: None,
            not_responding_since: None,
            pings: 0,
            pongs: 0,
            last_rtt: None,
        });
    }

    pub fn unregister(&mut self, client: u64) {
        self.clients.remove(&client);
    }

    pub fn state(&self, client: u64) -> Option<AppState> {
        self.clients.get(&client).map(|health| health.state)
    }

    pub fn is_known(&self, client: u64) -> bool {
        self.clients.contains_key(&client)
    }

    /// Record that a ping with `serial` was sent. Refused while a ping is
    /// already in flight so a slow client cannot stack pings.
    pub fn begin_ping(&mut self, client: u64, serial: u32, now: Instant) -> bool {
        let Some(health) = self.clients.get_mut(&client) else {
            return false;
        };
        if health.pending.is_some() {
            return false;
        }
        health.state = AppState::PingPending;
        health.pending = Some((serial, now));
        health.last_ping_at = Some(now);
        health.pings += 1;
        true
    }

    /// Record a pong. Returns `None` when no ping was in flight for `client`
    /// (or the client is unknown).
    pub fn pong(&mut self, client: u64, now: Instant) -> Option<Pong> {
        let health = self.clients.get_mut(&client)?;
        let (serial, sent) = health.pending.take()?;
        let recovered = health.not_responding_since.is_some();
        let rtt = now.saturating_duration_since(sent);
        health.state = AppState::Responsive;
        health.not_responding_since = None;
        health.pongs += 1;
        health.last_rtt = Some(rtt);
        Some(Pong {
            client,
            serial,
            rtt,
            recovered,
        })
    }

    /// Advance the timeouts. Returns the clients that moved to
    /// `NotResponding` on this call, sorted by client id.
    pub fn tick(&mut self, now: Instant) -> Vec<Transition> {
        let mut transitions = Vec::new();
        for (client, health) in self.clients.iter_mut() {
            if health.state != AppState::PingPending {
                continue;
            }
            let Some((_, sent)) = health.pending else {
                continue;
            };
            let waited = now.saturating_duration_since(sent);
            if waited >= self.timeout {
                health.state = AppState::NotResponding;
                // Keep the pending ping: smithay already tracks it and only a
                // matching late pong can clear it, which is exactly how a
                // recovered client is detected.
                health.not_responding_since = Some(now);
                transitions.push(Transition {
                    client: *client,
                    from: AppState::PingPending,
                    to: AppState::NotResponding,
                    after: waited,
                });
            }
        }
        transitions.sort_by_key(|transition| transition.client);
        transitions
    }

    /// Clients that should be pinged now: responsive clients whose last ping is
    /// older than `interval`, plus `NotResponding` clients without a ping in
    /// flight. No ping is queued while one is in flight: smithay refuses a
    /// second ping for the same client, and the late pong is what marks the
    /// client as recovered.
    pub fn clients_to_ping(&self, now: Instant, interval: Duration) -> Vec<u64> {
        let mut clients: Vec<u64> = self
            .clients
            .iter()
            .filter(|(_, health)| {
                if health.pending.is_some() {
                    return false;
                }
                match health.state {
                    AppState::PingPending => false,
                    AppState::Responsive => health
                        .last_ping_at
                        .map(|last| now.saturating_duration_since(last) >= interval)
                        .unwrap_or(true),
                    AppState::NotResponding => health
                        .not_responding_since
                        .map(|since| now.saturating_duration_since(since) >= interval)
                        .unwrap_or(true),
                }
            })
            .map(|(client, _)| *client)
            .collect();
        clients.sort_unstable();
        clients
    }

    pub fn not_responding(&self) -> Vec<u64> {
        let mut clients: Vec<u64> = self
            .clients
            .iter()
            .filter(|(_, health)| health.state == AppState::NotResponding)
            .map(|(client, _)| *client)
            .collect();
        clients.sort_unstable();
        clients
    }

    /// (responsive, pending, not_responding) counts for the summary marker.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for health in self.clients.values() {
            match health.state {
                AppState::Responsive => counts.0 += 1,
                AppState::PingPending => counts.1 += 1,
                AppState::NotResponding => counts.2 += 1,
            }
        }
        counts
    }

    pub fn summary_line(&self) -> String {
        let (responsive, pending, not_responding) = self.counts();
        format!(
            "clients={} responsive={responsive} ping_pending={pending} not_responding={not_responding} timeout_ms={}",
            self.clients.len(),
            self.timeout.as_millis()
        )
    }
}

/// Logs frames that waited too long between damage and presentation.
#[derive(Debug, Clone)]
pub struct FrameGapLogger {
    threshold: Duration,
    pub gaps: u64,
    pub max_gap: Duration,
    pub last_gap: Option<Duration>,
}

impl FrameGapLogger {
    pub fn new(threshold: Duration) -> Self {
        Self {
            threshold,
            gaps: 0,
            max_gap: Duration::ZERO,
            last_gap: None,
        }
    }

    pub fn threshold(&self) -> Duration {
        self.threshold
    }

    /// Observe the time damaged content waited for its frame. Returns the gap
    /// when it is at or over the threshold (the caller prints one marker).
    pub fn observe(&mut self, wait: Option<Duration>) -> Option<Duration> {
        let gap = wait?;
        self.last_gap = Some(gap);
        if gap >= self.threshold {
            self.gaps += 1;
            self.max_gap = self.max_gap.max(gap);
            return Some(gap);
        }
        None
    }

    pub fn marker_line(&self, gap: Duration) -> String {
        format!(
            "AETHER_COMPOSITOR_FRAME_GAP: ms={} threshold_ms={} gaps={} max_ms={}",
            gap.as_millis(),
            self.threshold.as_millis(),
            self.gaps,
            self.max_gap.as_millis()
        )
    }

    pub fn summary_line(&self) -> String {
        format!(
            "gaps={} threshold_ms={} max_ms={}",
            self.gaps,
            self.threshold.as_millis(),
            self.max_gap.as_millis()
        )
    }
}

/// Commit-freshness watchdog for the shell's layer surfaces (NH-03).
#[derive(Debug, Clone)]
pub struct ShellWatchdog {
    timeout: Duration,
    armed: bool,
    last_activity: Option<Instant>,
    expired: bool,
    pub timeouts: u64,
}

impl ShellWatchdog {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            armed: false,
            last_activity: None,
            expired: false,
            timeouts: 0,
        }
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn is_expired(&self) -> bool {
        self.expired
    }

    /// Start watching when the first shell surface appears.
    pub fn arm(&mut self, now: Instant) {
        if !self.armed {
            self.armed = true;
            self.last_activity = Some(now);
            self.expired = false;
        } else {
            self.note_activity(now);
        }
    }

    /// A shell surface committed. Returns `true` when this recovers an expired
    /// watchdog (so the caller can print the recovery marker).
    pub fn note_activity(&mut self, now: Instant) -> bool {
        if !self.armed {
            return false;
        }
        let recovered = self.expired;
        self.last_activity = Some(now);
        self.expired = false;
        recovered
    }

    /// Returns the idle time once when the shell stops committing for longer
    /// than the timeout.
    pub fn check(&mut self, now: Instant) -> Option<Duration> {
        if !self.armed || self.expired {
            return None;
        }
        let last = self.last_activity?;
        let idle = now.saturating_duration_since(last);
        if idle >= self.timeout {
            self.expired = true;
            self.timeouts += 1;
            return Some(idle);
        }
        None
    }

    pub fn marker_line(&self, idle: Duration) -> String {
        format!(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms={} timeout_ms={} timeouts={}",
            idle.as_millis(),
            self.timeout.as_millis(),
            self.timeouts
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_then_pong_keeps_the_app_responsive() {
        let now = Instant::now();
        let mut detector = HangDetector::new(Duration::from_secs(5));
        detector.register(1);
        assert_eq!(detector.state(1), Some(AppState::Responsive));
        assert!(detector.begin_ping(1, 10, now));
        assert_eq!(detector.state(1), Some(AppState::PingPending));
        // A second ping is refused while one is in flight.
        assert!(!detector.begin_ping(1, 11, now));

        let pong = detector
            .pong(1, now + Duration::from_millis(12))
            .expect("pong answered the pending ping");
        assert_eq!(pong.serial, 10);
        assert_eq!(pong.rtt, Duration::from_millis(12));
        assert!(!pong.recovered);
        assert_eq!(detector.state(1), Some(AppState::Responsive));
        assert!(detector.tick(now + Duration::from_secs(10)).is_empty());
    }

    #[test]
    fn missed_pong_becomes_not_responding_after_the_timeout() {
        let now = Instant::now();
        let mut detector = HangDetector::new(Duration::from_secs(5));
        detector.register(7);
        assert!(detector.begin_ping(7, 1, now));

        assert!(
            detector
                .tick(now + Duration::from_millis(4999))
                .is_empty(),
            "still within the 5 s timeout"
        );
        let transitions = detector.tick(now + Duration::from_secs(5));
        assert_eq!(transitions.len(), 1);
        let transition = transitions[0];
        assert_eq!(transition.client, 7);
        assert_eq!(transition.from, AppState::PingPending);
        assert_eq!(transition.to, AppState::NotResponding);
        assert_eq!(transition.after, Duration::from_secs(5));
        assert_eq!(detector.state(7), Some(AppState::NotResponding));
        assert_eq!(detector.not_responding(), vec![7]);

        // Reported once, not every tick.
        assert!(detector.tick(now + Duration::from_secs(6)).is_empty());
    }

    #[test]
    fn a_late_pong_recovers_a_not_responding_app() {
        let now = Instant::now();
        let mut detector = HangDetector::new(Duration::from_millis(100));
        detector.register(2);
        detector.begin_ping(2, 5, now);
        detector.tick(now + Duration::from_millis(150));
        assert_eq!(detector.state(2), Some(AppState::NotResponding));

        // The original ping is still pending: smithay refuses a second one, so
        // the compositor does not queue a retry while the ping is in flight.
        let retry_at = now + Duration::from_millis(300);
        assert!(!detector.begin_ping(2, 6, retry_at));
        assert!(detector
            .clients_to_ping(retry_at, Duration::from_millis(100))
            .is_empty());
        let pong = detector
            .pong(2, retry_at + Duration::from_millis(20))
            .expect("late pong of the original ping");
        assert!(pong.recovered);
        assert_eq!(detector.state(2), Some(AppState::Responsive));
        assert!(detector.not_responding().is_empty());
    }

    #[test]
    fn pongs_without_a_pending_ping_are_ignored() {
        let mut detector = HangDetector::new(Duration::from_secs(5));
        detector.register(3);
        assert!(detector.pong(3, Instant::now()).is_none());
        assert!(detector.pong(99, Instant::now()).is_none());
        assert!(!detector.begin_ping(99, 1, Instant::now()));
    }

    #[test]
    fn clients_to_ping_respects_the_interval() {
        let now = Instant::now();
        let mut detector = HangDetector::new(Duration::from_secs(5));
        detector.register(0);
        detector.register(1);
        assert_eq!(
            detector.clients_to_ping(now, Duration::from_secs(2)),
            vec![0, 1]
        );
        detector.begin_ping(0, 1, now);
        assert_eq!(
            detector.clients_to_ping(now + Duration::from_millis(500), Duration::from_secs(2)),
            vec![1]
        );
        detector.pong(0, now + Duration::from_millis(10));
        assert_eq!(
            detector.clients_to_ping(now + Duration::from_secs(2), Duration::from_secs(2)),
            vec![0, 1]
        );
    }

    #[test]
    fn counts_summarize_the_clients() {
        let now = Instant::now();
        let mut detector = HangDetector::new(Duration::from_millis(100));
        detector.register(0);
        detector.register(1);
        detector.begin_ping(1, 1, now);
        detector.tick(now + Duration::from_millis(150));
        assert_eq!(detector.counts(), (1, 0, 1));
        let line = detector.summary_line();
        assert!(line.contains("clients=2"), "{line}");
        assert!(line.contains("not_responding=1"), "{line}");
    }

    #[test]
    fn frame_gaps_recorded_only_over_the_threshold() {
        let mut logger = FrameGapLogger::new(Duration::from_secs(1));
        assert_eq!(logger.observe(None), None, "idle is not a gap");
        assert_eq!(logger.observe(Some(Duration::from_millis(200))), None);
        assert_eq!(logger.gaps, 0);
        let gap = logger
            .observe(Some(Duration::from_millis(1500)))
            .expect("over the 1 s threshold");
        assert_eq!(gap, Duration::from_millis(1500));
        assert_eq!(logger.gaps, 1);
        assert_eq!(logger.max_gap, Duration::from_millis(1500));
        let line = logger.marker_line(gap);
        assert!(line.contains("threshold_ms=1000"), "{line}");
        assert!(line.contains("gaps=1"), "{line}");
    }

    #[test]
    fn shell_watchdog_expires_once_and_recovers() {
        let start = Instant::now();
        let mut watchdog = ShellWatchdog::new(Duration::from_secs(5));
        assert!(watchdog.check(start + Duration::from_secs(10)).is_none());

        watchdog.arm(start);
        assert!(watchdog.is_armed());
        assert!(watchdog
            .check(start + Duration::from_millis(4999))
            .is_none());
        let idle = watchdog
            .check(start + Duration::from_secs(5))
            .expect("watchdog expired");
        assert_eq!(idle, Duration::from_secs(5));
        assert!(watchdog.is_expired());
        assert!(watchdog.check(start + Duration::from_secs(9)).is_none());

        assert!(watchdog.note_activity(start + Duration::from_secs(9)));
        assert!(!watchdog.is_expired());
        assert!(watchdog.check(start + Duration::from_secs(9)).is_none());
        assert!(watchdog
            .check(start + Duration::from_secs(14))
            .is_some());
        assert_eq!(watchdog.timeouts, 2);
        let line = watchdog.marker_line(Duration::from_secs(5));
        assert!(line.contains("state=timeout"), "{line}");
        assert!(line.contains("timeouts=2"), "{line}");
    }
}
