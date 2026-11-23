//! Lifecycle policy: pure decisions, no I/O (P5-T02/T04/T05/T06, D3 §6/§7).
//!
//! The policy receives events plus a snapshot of tracked apps and returns
//! actions. Mechanisms (freezer, reclaimer, killer, notifier) live elsewhere,
//! which makes every rule here unit-testable on any host.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::budget::{BackgroundBudget, BackgroundBudgetConfig};
use crate::identity::AppId;
use crate::pressure::PressureLevel;
use crate::protocol::LifecycleMessage;
use crate::state::{
    can_transition, AppProcess, AppState, Band, ProtectReason, TransitionError,
};

/// Where the machine gets power from. The daemon maps `aether-powerd` events
/// to this type so the lifecycle crate stays independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerSourceKind {
    Ac,
    Battery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimerKind {
    Grace,
    Hibernate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KillReason {
    MemoryPressure,
    UserQuit,
    Crash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateReason {
    Focused,
    FocusMovedAway,
    WindowShown,
    WindowHidden,
    GraceElapsed,
    HibernationTimeout,
    PressureElevated,
    ProtectionAdded,
    ProtectionRemoved,
    UserQuit,
    Killer,
    AppExited,
}

impl StateReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Focused => "focused",
            Self::FocusMovedAway => "focus_moved_away",
            Self::WindowShown => "window_shown",
            Self::WindowHidden => "window_hidden",
            Self::GraceElapsed => "grace_elapsed",
            Self::HibernationTimeout => "hibernation_timeout",
            Self::PressureElevated => "pressure_elevated",
            Self::ProtectionAdded => "protection_added",
            Self::ProtectionRemoved => "protection_removed",
            Self::UserQuit => "user_quit",
            Self::Killer => "killer",
            Self::AppExited => "app_exited",
        }
    }
}

/// Events from the compositor, portals, PSI, and timers (D3 section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    FocusChanged { to: Option<AppId> },
    WindowShown(AppId),
    WindowHidden(AppId),
    ProtectionChanged {
        id: AppId,
        reason: ProtectReason,
        active: bool,
    },
    PressureChanged(PressureLevel),
    TimerElapsed { id: AppId, timer: TimerKind },
    AppExited(AppId),
    /// User pressed "End task" in the Activity Manager.
    UserQuit(AppId),
    /// Machine changed power source; changes the grace period.
    PowerSourceChanged(PowerSourceKind),
    /// Background CPU time used by an app since the previous tick.
    BackgroundCpu { id: AppId, cpu_time: Duration },
}

/// Actions the daemon turns into mechanism calls. Extends the D3 contract with
/// explicit timer scheduling and CPU-weight control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SetState {
        id: AppId,
        to: AppState,
        reason: StateReason,
    },
    Reclaim {
        id: AppId,
        bytes: u64,
    },
    Kill {
        id: AppId,
        reason: KillReason,
    },
    Notify {
        id: AppId,
        message: LifecycleMessage,
    },
    Schedule {
        id: AppId,
        timer: TimerKind,
        after: Duration,
    },
    CancelTimers {
        id: AppId,
    },
    SetCpuWeight {
        id: AppId,
        weight: u32,
    },
    Log {
        id: Option<AppId>,
        text: String,
    },
}

/// Policy configuration. Defaults come from D3 section 3 and are tuned in P5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyConfig {
    pub grace_period_ac: Duration,
    pub grace_period_battery: Duration,
    pub hibernate_after: Duration,
    /// Fraction of `memory.current` requested from `memory.reclaim`.
    pub reclaim_percent: u8,
    /// Wait after `MemoryWarning` before a kill.
    pub memory_warning_grace: Duration,
    /// Minimum spacing between kills (kill-storm guard).
    pub kill_cooldown: Duration,
    pub background_budget: BackgroundBudgetConfig,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            grace_period_ac: Duration::from_secs(30),
            grace_period_battery: Duration::from_secs(10),
            hibernate_after: Duration::from_secs(10 * 60),
            reclaim_percent: 80,
            memory_warning_grace: Duration::from_millis(500),
            kill_cooldown: Duration::from_secs(2),
            background_budget: BackgroundBudgetConfig::default(),
        }
    }
}

impl PolicyConfig {
    pub fn grace_period(&self, source: PowerSourceKind) -> Duration {
        match source {
            PowerSourceKind::Ac => self.grace_period_ac,
            PowerSourceKind::Battery => self.grace_period_battery,
        }
    }

    pub fn reclaim_bytes(&self, memory_current: u64) -> u64 {
        memory_current / 100 * u64::from(self.reclaim_percent)
    }
}

/// Decision layer interface (D3 section 4).
pub trait Policy {
    fn on_event(&mut self, event: Event, apps: &[AppProcess]) -> Vec<Action>;
}

/// The D3 decision flow.
#[derive(Debug)]
pub struct LifecyclePolicy {
    config: PolicyConfig,
    source: PowerSourceKind,
    last_kill: Option<Instant>,
    warned: HashSet<AppId>,
    budget: BackgroundBudget,
    last_level: PressureLevel,
}

impl LifecyclePolicy {
    pub fn new(config: PolicyConfig) -> Self {
        Self {
            budget: BackgroundBudget::new(config.background_budget),
            config,
            source: PowerSourceKind::Ac,
            last_kill: None,
            warned: HashSet::new(),
            last_level: PressureLevel::Normal,
        }
    }

    pub fn config(&self) -> PolicyConfig {
        self.config
    }

    pub fn source(&self) -> PowerSourceKind {
        self.source
    }

    pub fn set_source(&mut self, source: PowerSourceKind) {
        self.source = source;
    }

    /// Apply a background policy received from `aether-powerd`.
    pub fn set_background_policy(&mut self, grace: Duration, hibernate_after: Duration) {
        match self.source {
            PowerSourceKind::Ac => self.config.grace_period_ac = grace,
            PowerSourceKind::Battery => self.config.grace_period_battery = grace,
        }
        self.config.hibernate_after = hibernate_after;
    }

    pub fn set_pressure_level(&mut self, level: PressureLevel) {
        self.last_level = level;
    }

    pub fn last_kill_at(&self) -> Option<Instant> {
        self.last_kill
    }

    /// Testable entry point: all decisions use the supplied `now`.
    pub fn on_event_at(
        &mut self,
        event: Event,
        apps: &[AppProcess],
        now: Instant,
    ) -> Vec<Action> {
        match event {
            Event::FocusChanged { to } => self.focus_changed(to.as_ref(), apps, now),
            Event::WindowShown(id) => self.window_shown(&id, apps, now),
            Event::WindowHidden(id) => self.window_hidden(&id, apps),
            Event::ProtectionChanged { id, reason, active } => {
                self.protection_changed(&id, reason, active, apps)
            }
            Event::PressureChanged(level) => self.pressure_changed(level, apps, now),
            Event::TimerElapsed { id, timer } => self.timer_elapsed(&id, timer, apps),
            Event::AppExited(id) => self.app_exited(&id, apps),
            Event::UserQuit(id) => self.user_quit(&id, apps),
            Event::PowerSourceChanged(source) => {
                self.source = source;
                vec![Action::Log {
                    id: None,
                    text: format!("power_source={}", match source {
                        PowerSourceKind::Ac => "ac",
                        PowerSourceKind::Battery => "battery",
                    }),
                }]
            }
            Event::BackgroundCpu { id, cpu_time } => {
                let throttled = self.budget.consume(&id, cpu_time, now);
                let weight = self.budget.weight_for(&id);
                let mut actions = vec![Action::SetCpuWeight {
                    id: id.clone(),
                    weight,
                }];
                if throttled {
                    actions.push(Action::Notify {
                        id: id.clone(),
                        message: LifecycleMessage::BackgroundBudgetExceeded,
                    });
                    actions.push(Action::Log {
                        id: Some(id),
                        text: "background task budget exhausted; cpu.weight lowered".into(),
                    });
                }
                actions
            }
        }
    }

    fn state_of<'a>(apps: &'a [AppProcess], id: &AppId) -> Option<&'a AppProcess> {
        apps.iter().find(|a| &a.id == id)
    }

    fn foreground(apps: &[AppProcess]) -> Option<&AppProcess> {
        apps.iter().find(|a| a.state == AppState::Foreground)
    }

    fn focus_changed(
        &mut self,
        to: Option<&AppId>,
        apps: &[AppProcess],
        now: Instant,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(previous) = Self::foreground(apps) {
            if Some(&previous.id) != to {
                actions.push(Action::SetState {
                    id: previous.id.clone(),
                    to: AppState::Visible,
                    reason: StateReason::FocusMovedAway,
                });
                actions.push(Action::Log {
                    id: Some(previous.id.clone()),
                    text: format!(
                        "focus -> {} (last_focused={:?})",
                        to.map(|i| i.as_str()).unwrap_or("none"),
                        now.saturating_duration_since(previous.last_focused)
                    ),
                });
            }
        }
        if let Some(id) = to {
            actions.push(Action::CancelTimers { id: id.clone() });
            if let Some(app) = Self::state_of(apps, id) {
                let was_frozen = app.state.is_frozen();
                actions.push(Action::SetState {
                    id: id.clone(),
                    to: AppState::Foreground,
                    reason: StateReason::Focused,
                });
                if was_frozen {
                    actions.push(Action::Notify {
                        id: id.clone(),
                        message: LifecycleMessage::DidResume,
                    });
                }
            }
        }
        actions
    }

    fn window_shown(&mut self, id: &AppId, apps: &[AppProcess], _now: Instant) -> Vec<Action> {
        let Some(app) = Self::state_of(apps, id) else {
            return Vec::new();
        };
        if app.state.is_frozen() && !can_transition(app.state, AppState::Visible) {
            return Vec::new();
        }
        let mut actions = vec![Action::CancelTimers { id: id.clone() }];
        actions.push(Action::SetState {
            id: id.clone(),
            to: AppState::Visible,
            reason: StateReason::WindowShown,
        });
        if app.state.is_frozen() {
            actions.push(Action::Notify {
                id: id.clone(),
                message: LifecycleMessage::DidResume,
            });
        }
        actions
    }

    fn window_hidden(&mut self, id: &AppId, apps: &[AppProcess]) -> Vec<Action> {
        let Some(app) = Self::state_of(apps, id) else {
            return Vec::new();
        };
        let grace = self.config.grace_period(self.source);
        let mut actions = vec![Action::SetState {
            id: id.clone(),
            to: AppState::Background,
            reason: StateReason::WindowHidden,
        }];
        actions.push(Action::Schedule {
            id: id.clone(),
            timer: TimerKind::Grace,
            after: grace,
        });
        actions.push(Action::Log {
            id: Some(id.clone()),
            text: format!(
                "state {}->Background reason=window_hidden; grace={:?} source={:?}",
                app.state, grace, self.source
            ),
        });
        actions
    }

    fn protection_changed(
        &mut self,
        id: &AppId,
        reason: ProtectReason,
        active: bool,
        apps: &[AppProcess],
    ) -> Vec<Action> {
        let Some(app) = Self::state_of(apps, id) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if active {
            // Protection added while frozen: thaw and return to Background.
            if app.state.is_frozen() {
                actions.push(Action::SetState {
                    id: id.clone(),
                    to: AppState::Background,
                    reason: StateReason::ProtectionAdded,
                });
            }
            actions.push(Action::Log {
                id: Some(id.clone()),
                text: format!(
                    "protection {} added (blocks_freeze={}, blocks_reclaim={})",
                    reason.as_str(),
                    reason.blocks_freeze(),
                    reason.blocks_reclaim()
                ),
            });
        } else {
            actions.push(Action::Log {
                id: Some(id.clone()),
                text: format!("protection {} removed", reason.as_str()),
            });
            // The app is still background and unprotected; restart the grace
            // period so it can be suspended again.
            if app.state == AppState::Background
                && !app.protect.iter().any(|r| *r != reason && r.blocks_freeze())
            {
                actions.push(Action::Schedule {
                    id: id.clone(),
                    timer: TimerKind::Grace,
                    after: self.config.grace_period(self.source),
                });
            }
        }
        actions
    }

    fn pressure_changed(
        &mut self,
        level: PressureLevel,
        apps: &[AppProcess],
        now: Instant,
    ) -> Vec<Action> {
        self.last_level = level;
        let mut actions = Vec::new();
        match level {
            PressureLevel::Normal => {
                actions.push(Action::Log {
                    id: None,
                    text: "pressure normal".into(),
                });
            }
            PressureLevel::Elevated => {
                for app in apps {
                    if app.state == AppState::Suspended {
                        if app.protect.iter().any(|r| r.blocks_reclaim()) {
                            continue;
                        }
                        actions.push(Action::Reclaim {
                            id: app.id.clone(),
                            bytes: self.config.reclaim_bytes(app.memory_current),
                        });
                        actions.push(Action::SetState {
                            id: app.id.clone(),
                            to: AppState::Hibernated,
                            reason: StateReason::PressureElevated,
                        });
                    } else if app.state == AppState::Background && !app.blocks_reclaim() {
                        actions.push(Action::Reclaim {
                            id: app.id.clone(),
                            bytes: self.config.reclaim_bytes(app.memory_current),
                        });
                        actions.push(Action::SetCpuWeight {
                            id: app.id.clone(),
                            weight: self.budget.weight_for(&app.id),
                        });
                    }
                }
                actions.push(Action::Log {
                    id: None,
                    text: "pressure elevated: hibernate suspended, reclaim background".into(),
                });
            }
            PressureLevel::Critical => {
                // Warn everyone, then kill at most one app per decision.
                for app in apps {
                    if app.state.is_alive() {
                        self.warned.insert(app.id.clone());
                        actions.push(Action::Notify {
                            id: app.id.clone(),
                            message: LifecycleMessage::MemoryWarning,
                        });
                    }
                }
                let cooldown_ok = self
                    .last_kill
                    .map(|last| now.saturating_duration_since(last) >= self.config.kill_cooldown)
                    .unwrap_or(true);
                if !cooldown_ok {
                    actions.push(Action::Log {
                        id: None,
                        text: "pressure critical: kill suppressed by 2 s cooldown".into(),
                    });
                    return actions;
                }
                if let Some(victim) = select_kill_candidate(apps, level) {
                    self.last_kill = Some(now);
                    self.warned.remove(&victim.id);
                    actions.push(Action::Log {
                        id: Some(victim.id.clone()),
                        text: format!(
                            "kill reason=MemoryPressure band={:?} memory={}",
                            victim.band(),
                            victim.memory_current
                        ),
                    });
                    actions.push(Action::Kill {
                        id: victim.id.clone(),
                        reason: KillReason::MemoryPressure,
                    });
                    actions.push(Action::SetState {
                        id: victim.id.clone(),
                        to: AppState::Terminated,
                        reason: StateReason::Killer,
                    });
                }
            }
        }
        actions
    }

    fn timer_elapsed(&mut self, id: &AppId, timer: TimerKind, apps: &[AppProcess]) -> Vec<Action> {
        let Some(app) = Self::state_of(apps, id) else {
            return Vec::new();
        };
        match timer {
            TimerKind::Grace => {
                if app.state != AppState::Background {
                    return Vec::new();
                }
                if app.blocks_freeze() {
                    return vec![Action::Log {
                        id: Some(id.clone()),
                        text: format!(
                            "suspend skipped: protected by {}",
                            app.protect
                                .iter()
                                .map(|r| r.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                    }];
                }
                vec![
                    Action::Notify {
                        id: id.clone(),
                        message: LifecycleMessage::WillSuspend,
                    },
                    Action::SetState {
                        id: id.clone(),
                        to: AppState::Suspended,
                        reason: StateReason::GraceElapsed,
                    },
                    Action::Schedule {
                        id: id.clone(),
                        timer: TimerKind::Hibernate,
                        after: self.config.hibernate_after,
                    },
                ]
            }
            TimerKind::Hibernate => {
                if app.state != AppState::Suspended {
                    return Vec::new();
                }
                if app.protect.iter().any(|r| r.blocks_reclaim()) {
                    return vec![Action::Log {
                        id: Some(id.clone()),
                        text: "hibernate skipped: reclaim blocked".into(),
                    }];
                }
                vec![
                    Action::Reclaim {
                        id: id.clone(),
                        bytes: self.config.reclaim_bytes(app.memory_current),
                    },
                    Action::SetState {
                        id: id.clone(),
                        to: AppState::Hibernated,
                        reason: StateReason::HibernationTimeout,
                    },
                ]
            }
        }
    }

    fn app_exited(&mut self, id: &AppId, _apps: &[AppProcess]) -> Vec<Action> {
        self.warned.remove(id);
        self.budget.forget(id);
        vec![
            Action::CancelTimers { id: id.clone() },
            Action::SetState {
                id: id.clone(),
                to: AppState::Terminated,
                reason: StateReason::AppExited,
            },
        ]
    }

    fn user_quit(&mut self, id: &AppId, apps: &[AppProcess]) -> Vec<Action> {
        let Some(app) = Self::state_of(apps, id) else {
            return Vec::new();
        };
        if !app.state.is_alive() {
            return Vec::new();
        }
        vec![
            Action::CancelTimers { id: id.clone() },
            Action::Kill {
                id: id.clone(),
                reason: KillReason::UserQuit,
            },
            Action::SetState {
                id: id.clone(),
                to: AppState::Terminated,
                reason: StateReason::UserQuit,
            },
        ]
    }
}

impl Policy for LifecyclePolicy {
    fn on_event(&mut self, event: Event, apps: &[AppProcess]) -> Vec<Action> {
        self.on_event_at(event, apps, Instant::now())
    }
}

/// The killer picks the lowest-priority eligible band, then the largest
/// memory, then the oldest `last_focused` (D3 section 6).
pub fn select_kill_candidate<'a>(
    apps: &'a [AppProcess],
    level: PressureLevel,
) -> Option<&'a AppProcess> {
    let mut candidates: Vec<&AppProcess> = apps
        .iter()
        .filter(|app| app.state.is_alive())
        .filter(|app| match level {
            PressureLevel::Normal => false,
            PressureLevel::Elevated => app.band().killable_at_elevated(),
            PressureLevel::Critical => app.band().killable_at_critical(),
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.band()
            .cmp(&a.band())
            .then_with(|| b.memory_current.cmp(&a.memory_current))
            .then_with(|| a.last_focused.cmp(&b.last_focused))
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates.first().copied()
}

/// True when a transition the policy intends to make is legal. The daemon
/// uses this before applying `SetState` so a buggy event cannot leave an app
/// in an impossible state.
pub fn transition_is_legal(from: AppState, to: AppState) -> Result<(), TransitionError> {
    crate::state::validate_transition(from, to).map(|_| ())
}

/// Convenience for the Activity Manager: the band name shown to the user.
pub fn band_label(band: Band) -> &'static str {
    match band {
        Band::System => "System",
        Band::Foreground => "Foreground",
        Band::Visible => "Visible",
        Band::Protected => "Protected",
        Band::Background => "Background",
        Band::Suspended => "Suspended",
        Band::Hibernated => "Hibernated",
        Band::Terminated => "Terminated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app(name: &str, state: AppState, memory: u64, now: Instant) -> AppProcess {
        let mut app = AppProcess::new(
            AppId::new(name).unwrap(),
            PathBuf::from(format!("/cgroup/{name}")),
            now,
        );
        app.state = state;
        app.memory_current = memory;
        app
    }

    fn state_changes(actions: &[Action]) -> Vec<(AppId, AppState, StateReason)> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::SetState { id, to, reason } => Some((id.clone(), *to, *reason)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn focus_moves_previous_to_visible_and_new_to_foreground() {
        let now = Instant::now();
        let apps = vec![
            app("org.example.A", AppState::Foreground, 0, now),
            app("org.example.B", AppState::Background, 0, now),
        ];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let b = AppId::new("org.example.B").unwrap();
        let actions = policy.on_event_at(
            Event::FocusChanged { to: Some(b.clone()) },
            &apps,
            now,
        );
        let changes = state_changes(&actions);
        assert!(changes.contains(&(
            AppId::new("org.example.A").unwrap(),
            AppState::Visible,
            StateReason::FocusMovedAway
        )));
        assert!(changes.contains(&(b, AppState::Foreground, StateReason::Focused)));
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::CancelTimers { .. })));
    }

    #[test]
    fn focusing_a_hibernated_app_sends_did_resume() {
        let now = Instant::now();
        let id = AppId::new("org.example.Mail").unwrap();
        let apps = vec![app("org.example.Mail", AppState::Hibernated, 0, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(Event::FocusChanged { to: Some(id.clone()) }, &apps, now);
        assert!(actions.contains(&Action::Notify {
            id: id.clone(),
            message: LifecycleMessage::DidResume
        }));
        assert!(state_changes(&actions).contains(&(id, AppState::Foreground, StateReason::Focused)));
    }

    #[test]
    fn hidden_window_uses_the_power_source_grace_period() {
        let now = Instant::now();
        let id = AppId::new("org.example.Mail").unwrap();
        let apps = vec![app("org.example.Mail", AppState::Visible, 0, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());

        let actions = policy.on_event_at(Event::WindowHidden(id.clone()), &apps, now);
        let grace = actions
            .iter()
            .find_map(|a| match a {
                Action::Schedule { timer: TimerKind::Grace, after, .. } => Some(*after),
                _ => None,
            })
            .expect("grace timer");
        assert_eq!(grace, Duration::from_secs(30));

        policy.set_source(PowerSourceKind::Battery);
        let actions = policy.on_event_at(Event::WindowHidden(id), &apps, now);
        let grace = actions
            .iter()
            .find_map(|a| match a {
                Action::Schedule { timer: TimerKind::Grace, after, .. } => Some(*after),
                _ => None,
            })
            .unwrap();
        assert_eq!(grace, Duration::from_secs(10));
    }

    #[test]
    fn grace_elapsed_suspends_unprotected_background_app() {
        let now = Instant::now();
        let id = AppId::new("org.example.Mail").unwrap();
        let apps = vec![app("org.example.Mail", AppState::Background, 1024, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(
            Event::TimerElapsed {
                id: id.clone(),
                timer: TimerKind::Grace,
            },
            &apps,
            now,
        );
        assert!(actions.contains(&Action::Notify {
            id: id.clone(),
            message: LifecycleMessage::WillSuspend
        }));
        assert!(state_changes(&actions).contains(&(id.clone(), AppState::Suspended, StateReason::GraceElapsed)));
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::Schedule { timer: TimerKind::Hibernate, .. }
        )));
    }

    #[test]
    fn protection_blocks_suspend() {
        let now = Instant::now();
        let id = AppId::new("org.example.Music").unwrap();
        let mut music = app("org.example.Music", AppState::Background, 1024, now);
        music.add_protection(ProtectReason::Audio);
        let apps = vec![music];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(
            Event::TimerElapsed {
                id: id.clone(),
                timer: TimerKind::Grace,
            },
            &apps,
            now,
        );
        assert!(state_changes(&actions).is_empty());
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::Log { text, .. } if text.contains("protected by Audio"))));
    }

    #[test]
    fn protection_added_while_suspended_thaws_to_background() {
        let now = Instant::now();
        let id = AppId::new("org.example.Music").unwrap();
        let mut music = app("org.example.Music", AppState::Suspended, 1024, now);
        music.state = AppState::Suspended;
        let apps = vec![music];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(
            Event::ProtectionChanged {
                id: id.clone(),
                reason: ProtectReason::Audio,
                active: true,
            },
            &apps,
            now,
        );
        assert!(state_changes(&actions).contains(&(
            id,
            AppState::Background,
            StateReason::ProtectionAdded
        )));
    }

    #[test]
    fn hibernate_reclaims_configured_ratio() {
        let now = Instant::now();
        let id = AppId::new("org.example.Mail").unwrap();
        let apps = vec![app("org.example.Mail", AppState::Suspended, 10_000, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(
            Event::TimerElapsed {
                id: id.clone(),
                timer: TimerKind::Hibernate,
            },
            &apps,
            now,
        );
        assert!(actions.contains(&Action::Reclaim {
            id: id.clone(),
            bytes: 8_000
        }));
        assert!(state_changes(&actions).contains(&(id, AppState::Hibernated, StateReason::HibernationTimeout)));
    }

    #[test]
    fn elevated_pressure_hibernates_and_reclaims_without_killing() {
        let now = Instant::now();
        let apps = vec![
            app("org.example.Fg", AppState::Foreground, 100, now),
            app("org.example.Bg", AppState::Background, 2_000, now),
            app("org.example.Susp", AppState::Suspended, 3_000, now),
        ];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions =
            policy.on_event_at(Event::PressureChanged(PressureLevel::Elevated), &apps, now);
        assert!(!actions.iter().any(|a| matches!(a, Action::Kill { .. })));
        assert!(actions.contains(&Action::Reclaim {
            id: AppId::new("org.example.Susp").unwrap(),
            bytes: 2_400
        }));
        assert!(actions.contains(&Action::Reclaim {
            id: AppId::new("org.example.Bg").unwrap(),
            bytes: 1_600
        }));
        assert!(state_changes(&actions).contains(&(
            AppId::new("org.example.Susp").unwrap(),
            AppState::Hibernated,
            StateReason::PressureElevated
        )));
    }

    #[test]
    fn critical_pressure_warns_then_kills_the_lowest_band() {
        let now = Instant::now();
        let newer = now + Duration::from_secs(1);
        let mut old = app("org.example.Old", AppState::Hibernated, 100, now);
        old.last_focused = now;
        let mut newer_bg = app("org.example.Bg", AppState::Background, 500, newer);
        newer_bg.last_focused = newer;
        let apps = vec![
            app("org.example.Fg", AppState::Foreground, 900, now),
            newer_bg,
            old,
        ];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions =
            policy.on_event_at(Event::PressureChanged(PressureLevel::Critical), &apps, now);

        // Every alive app gets MemoryWarning first.
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a, Action::Notify { message: LifecycleMessage::MemoryWarning, .. }))
                .count(),
            3
        );
        // The hibernated app (band 6) is killed first.
        assert!(actions.contains(&Action::Kill {
            id: AppId::new("org.example.Old").unwrap(),
            reason: KillReason::MemoryPressure
        }));
        assert!(!actions.iter().any(|a| matches!(
            a,
            Action::Kill { id, .. } if id == &AppId::new("org.example.Fg").unwrap()
        )));
    }

    #[test]
    fn foreground_is_never_killed() {
        let now = Instant::now();
        let apps = vec![app("org.example.Fg", AppState::Foreground, 10_000, now)];
        assert!(select_kill_candidate(&apps, PressureLevel::Critical).is_none());
        assert!(select_kill_candidate(&apps, PressureLevel::Elevated).is_none());
    }

    #[test]
    fn within_a_band_largest_memory_wins() {
        let now = Instant::now();
        let small = app("org.example.Small", AppState::Background, 100, now);
        let big = app("org.example.Big", AppState::Background, 900, now);
        let apps = vec![small, big];
        let victim = select_kill_candidate(&apps, PressureLevel::Critical).unwrap();
        assert_eq!(victim.id, AppId::new("org.example.Big").unwrap());
    }

    #[test]
    fn ties_break_by_oldest_last_focus() {
        let now = Instant::now();
        let mut older = app("org.example.Older", AppState::Background, 500, now);
        older.last_focused = now;
        let mut younger = app("org.example.Younger", AppState::Background, 500, now);
        younger.last_focused = now + Duration::from_secs(30);
        let apps = vec![younger, older];
        let victim = select_kill_candidate(&apps, PressureLevel::Critical).unwrap();
        assert_eq!(victim.id, AppId::new("org.example.Older").unwrap());
    }

    #[test]
    fn kill_storm_guard_allows_one_kill_per_two_seconds() {
        let now = Instant::now();
        let apps = vec![app("org.example.Bg", AppState::Background, 500, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());

        let first =
            policy.on_event_at(Event::PressureChanged(PressureLevel::Critical), &apps, now);
        assert_eq!(
            first.iter().filter(|a| matches!(a, Action::Kill { .. })).count(),
            1
        );

        let second = policy.on_event_at(
            Event::PressureChanged(PressureLevel::Critical),
            &apps,
            now + Duration::from_millis(500),
        );
        assert_eq!(
            second.iter().filter(|a| matches!(a, Action::Kill { .. })).count(),
            0
        );

        let third = policy.on_event_at(
            Event::PressureChanged(PressureLevel::Critical),
            &apps,
            now + Duration::from_secs(2),
        );
        assert_eq!(
            third.iter().filter(|a| matches!(a, Action::Kill { .. })).count(),
            1
        );
    }

    #[test]
    fn audio_app_is_not_selected_at_elevated_pressure() {
        let now = Instant::now();
        let mut music = app("org.example.Music", AppState::Background, 900, now);
        music.add_protection(ProtectReason::Audio);
        let apps = vec![music];
        assert!(select_kill_candidate(&apps, PressureLevel::Elevated).is_none());
    }

    #[test]
    fn background_budget_lowers_cpu_weight() {
        let now = Instant::now();
        let id = AppId::new("org.example.Bg").unwrap();
        let apps = vec![app("org.example.Bg", AppState::Background, 0, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig {
            background_budget: BackgroundBudgetConfig {
                allowance: Duration::from_secs(1),
                window: Duration::from_secs(60),
                ..Default::default()
            },
            ..Default::default()
        });
        let actions = policy.on_event_at(
            Event::BackgroundCpu {
                id: id.clone(),
                cpu_time: Duration::from_secs(2),
            },
            &apps,
            now,
        );
        assert!(actions.contains(&Action::SetCpuWeight {
            id: id.clone(),
            weight: 10
        }));
        assert!(actions.contains(&Action::Notify {
            id,
            message: LifecycleMessage::BackgroundBudgetExceeded
        }));
    }

    #[test]
    fn user_quit_kills_and_terminates() {
        let now = Instant::now();
        let id = AppId::new("org.example.Mail").unwrap();
        let apps = vec![app("org.example.Mail", AppState::Background, 0, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(Event::UserQuit(id.clone()), &apps, now);
        assert!(actions.contains(&Action::Kill {
            id: id.clone(),
            reason: KillReason::UserQuit
        }));
        assert!(state_changes(&actions).contains(&(id, AppState::Terminated, StateReason::UserQuit)));
    }

    #[test]
    fn exited_app_is_terminated_and_timers_cancelled() {
        let now = Instant::now();
        let id = AppId::new("org.example.Mail").unwrap();
        let apps = vec![app("org.example.Mail", AppState::Background, 0, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = policy.on_event_at(Event::AppExited(id.clone()), &apps, now);
        assert!(actions.contains(&Action::CancelTimers { id: id.clone() }));
        assert!(state_changes(&actions).contains(&(id, AppState::Terminated, StateReason::AppExited)));
    }

    #[test]
    fn policy_trait_entry_point_works() {
        let now = Instant::now();
        let apps = vec![app("org.example.Mail", AppState::Background, 0, now)];
        let mut policy = LifecyclePolicy::new(PolicyConfig::default());
        let actions = Policy::on_event(
            &mut policy,
            Event::TimerElapsed {
                id: AppId::new("org.example.Mail").unwrap(),
                timer: TimerKind::Grace,
            },
            &apps,
        );
        assert!(!actions.is_empty());
    }
}
