//! Lifecycle state machine (P5-T02, D3 section 3).
//!
//! The table in D3 section 3 is encoded once, in [`can_transition`], and the
//! policy layer is only allowed to emit transitions through
//! [`validate_transition`]. Every row of the table has a unit test.

use std::fmt;
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::identity::AppId;

/// State of one app as seen by the lifecycle daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    Foreground,
    Visible,
    Background,
    Suspended,
    Hibernated,
    Terminated,
}

impl AppState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "Foreground",
            Self::Visible => "Visible",
            Self::Background => "Background",
            Self::Suspended => "Suspended",
            Self::Hibernated => "Hibernated",
            Self::Terminated => "Terminated",
        }
    }

    /// States whose processes are expected to still exist.
    pub fn is_alive(self) -> bool {
        !matches!(self, Self::Terminated)
    }

    /// True when the app's processes are frozen (suspended or hibernated).
    pub fn is_frozen(self) -> bool {
        matches!(self, Self::Suspended | Self::Hibernated)
    }

    /// True when the app is on screen (focused or visible).
    pub fn is_on_screen(self) -> bool {
        matches!(self, Self::Foreground | Self::Visible)
    }

    /// True when the app counts towards the inactive-memory gate G-PM2.
    pub fn is_inactive(self) -> bool {
        matches!(self, Self::Suspended | Self::Hibernated)
    }
}

impl fmt::Display for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Protection reasons that block suspension (D3 section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectReason {
    Audio,
    Download,
    Vpn,
    Call,
    Navigation,
    UserPinned,
}

impl ProtectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Download => "Download",
            Self::Vpn => "Vpn",
            Self::Call => "Call",
            Self::Navigation => "Navigation",
            Self::UserPinned => "UserPinned",
        }
    }

    /// Audio and Call block freeze *and* reclaim so playback is not
    /// interrupted. The other reasons only block freeze.
    pub fn blocks_freeze(self) -> bool {
        true
    }

    pub fn blocks_reclaim(self) -> bool {
        matches!(self, Self::Audio | Self::Call)
    }
}

/// One event that may change app state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// Focus moved to `to` (`None` means no app is focused).
    FocusChanged { to: Option<AppId> },
    WindowShown(AppId),
    WindowHidden(AppId),
    ProtectionChanged {
        id: AppId,
        reason: ProtectReason,
        active: bool,
    },
    AppExited(AppId),
    /// User asked to end the app ("End task" in the Activity Manager).
    UserQuit(AppId),
}

/// Why a transition is or is not allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    /// The target state is not reachable from the source state in D3 §3.
    Forbidden { from: AppState, to: AppState },
    /// The app already is in the target state.
    SameState(AppState),
    /// Terminated is terminal.
    AlreadyTerminated,
}

impl std::error::Error for TransitionError {}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forbidden { from, to } => write!(f, "forbidden {from} -> {to}"),
            Self::SameState(state) => write!(f, "already in {state}"),
            Self::AlreadyTerminated => f.write_str("app is already terminated"),
        }
    }
}

/// D3 section 3, encoded as a function.
pub fn can_transition(from: AppState, to: AppState) -> bool {
    use AppState::*;
    if from == to {
        return false;
    }
    match (from, to) {
        // Focus moves.
        (Foreground, Visible) => true,
        (Visible, Foreground) => true,
        (Background, Foreground) => true,
        (Suspended | Hibernated, Foreground | Visible) => true,
        // Window hidden or minimized.
        (Foreground, Background) => true,
        (Visible, Background) => true,
        // Grace period elapsed.
        (Background, Suspended) => true,
        // Hibernation timeout or memory pressure.
        (Suspended, Hibernated) => true,
        // Protection added while suspended: thaw and return to Background.
        (Suspended | Hibernated, Background) => true,
        // Killer or user quit; foreground apps are never killed by the killer,
        // but the table allows termination only for non-foreground apps.
        (Visible | Background | Suspended | Hibernated, Terminated) => true,
        _ => false,
    }
}

/// Validated transition with the reason recorded in the log (D3 section 9).
pub fn validate_transition(
    from: AppState,
    to: AppState,
) -> Result<(AppState, AppState), TransitionError> {
    if from == AppState::Terminated {
        return Err(TransitionError::AlreadyTerminated);
    }
    if from == to {
        return Err(TransitionError::SameState(from));
    }
    if can_transition(from, to) {
        Ok((from, to))
    } else {
        Err(TransitionError::Forbidden { from, to })
    }
}

/// One app tracked by the daemon (D3 section 4 contract).
#[derive(Debug, Clone)]
pub struct AppProcess {
    pub id: AppId,
    pub scope: PathBuf,
    pub pids: Vec<u32>,
    pub state: AppState,
    pub protect: Vec<ProtectReason>,
    pub memory_current: u64,
    pub last_focused: Instant,
}

impl AppProcess {
    pub fn new(id: AppId, scope: PathBuf, now: Instant) -> Self {
        Self {
            id,
            scope,
            pids: Vec::new(),
            state: AppState::Background,
            protect: Vec::new(),
            memory_current: 0,
            last_focused: now,
        }
    }

    pub fn has_protection(&self) -> bool {
        !self.protect.is_empty()
    }

    pub fn blocks_freeze(&self) -> bool {
        self.protect.iter().any(|r| r.blocks_freeze())
    }

    pub fn blocks_reclaim(&self) -> bool {
        self.protect.iter().any(|r| r.blocks_reclaim())
    }

    pub fn add_protection(&mut self, reason: ProtectReason) {
        if !self.protect.contains(&reason) {
            self.protect.push(reason);
            self.protect.sort();
        }
    }

    pub fn remove_protection(&mut self, reason: ProtectReason) {
        self.protect.retain(|r| *r != reason);
    }

    /// The priority band from D3 section 6. Lower value = more protected.
    pub fn band(&self) -> Band {
        match self.state {
            AppState::Foreground => Band::Foreground,
            AppState::Visible => Band::Visible,
            AppState::Suspended => Band::Suspended,
            AppState::Hibernated => Band::Hibernated,
            AppState::Background => {
                if self.has_protection() {
                    Band::Protected
                } else {
                    Band::Background
                }
            }
            AppState::Terminated => Band::Terminated,
        }
    }
}

/// Priority bands (D3 section 6), ordered most protected first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    System = 0,
    Foreground = 1,
    Visible = 2,
    Protected = 3,
    Background = 4,
    Suspended = 5,
    Hibernated = 6,
    Terminated = 7,
}

impl Band {
    /// Bands the killer may pick at Elevated pressure: background only.
    pub fn killable_at_elevated(self) -> bool {
        self == Band::Background
    }

    /// Bands the killer may pick at Critical pressure, in kill order after
    /// band 4 is empty (D3 section 6).
    pub fn killable_at_critical(self) -> bool {
        matches!(
            self,
            Band::Background | Band::Suspended | Band::Hibernated | Band::Protected | Band::Visible
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of D3 section 3, plus the rows that must stay forbidden.
    #[test]
    fn every_transition_row_is_encoded() {
        use AppState::*;
        let allowed = [
            (Foreground, Visible),
            (Foreground, Background),
            (Visible, Foreground),
            (Visible, Background),
            (Background, Foreground),
            (Background, Suspended),
            (Suspended, Foreground),
            (Suspended, Visible),
            (Suspended, Hibernated),
            (Suspended, Background),
            (Hibernated, Foreground),
            (Hibernated, Visible),
            (Hibernated, Background),
            (Visible, Terminated),
            (Background, Terminated),
            (Suspended, Terminated),
            (Hibernated, Terminated),
        ];
        for (from, to) in allowed {
            assert!(can_transition(from, to), "{from} -> {to} must be allowed");
            assert_eq!(validate_transition(from, to), Ok((from, to)));
        }
    }

    #[test]
    fn forbidden_transitions_are_rejected() {
        use AppState::*;
        let forbidden = [
            (Foreground, Suspended),
            (Foreground, Hibernated),
            (Foreground, Terminated),
            (Visible, Suspended),
            (Visible, Hibernated),
            (Background, Hibernated),
        ];
        for (from, to) in forbidden {
            assert!(!can_transition(from, to), "{from} -> {to} must be rejected");
            assert_eq!(
                validate_transition(from, to),
                Err(TransitionError::Forbidden { from, to })
            );
        }
    }

    #[test]
    fn same_state_and_terminated_are_rejected() {
        use AppState::*;
        assert_eq!(
            validate_transition(Background, Background),
            Err(TransitionError::SameState(Background))
        );
        assert_eq!(
            validate_transition(Terminated, Foreground),
            Err(TransitionError::AlreadyTerminated)
        );
    }

    #[test]
    fn termination_is_terminal() {
        for state in [
            AppState::Foreground,
            AppState::Visible,
            AppState::Background,
            AppState::Suspended,
            AppState::Hibernated,
        ] {
            assert!(!can_transition(AppState::Terminated, state));
        }
        assert!(AppState::Terminated.is_alive() == false);
    }

    #[test]
    fn bands_follow_state_and_protection() {
        let now = Instant::now();
        let id = AppId::new("org.example.App").unwrap();
        let mut app = AppProcess::new(id, PathBuf::from("/sys/fs/cgroup/x"), now);

        app.state = AppState::Foreground;
        assert_eq!(app.band(), Band::Foreground);
        app.state = AppState::Visible;
        assert_eq!(app.band(), Band::Visible);
        app.state = AppState::Background;
        assert_eq!(app.band(), Band::Background);
        app.add_protection(ProtectReason::Download);
        assert_eq!(app.band(), Band::Protected);
        app.state = AppState::Suspended;
        app.state = AppState::Hibernated;
        assert_eq!(app.band(), Band::Hibernated);
    }

    #[test]
    fn protection_reasons_block_the_right_operations() {
        let now = Instant::now();
        let id = AppId::new("org.example.App").unwrap();
        let mut app = AppProcess::new(id, PathBuf::from("/sys/fs/cgroup/x"), now);

        app.add_protection(ProtectReason::Audio);
        assert!(app.blocks_freeze());
        assert!(app.blocks_reclaim());

        app.remove_protection(ProtectReason::Audio);
        app.add_protection(ProtectReason::Download);
        assert!(app.blocks_freeze());
        assert!(!app.blocks_reclaim());

        app.add_protection(ProtectReason::UserPinned);
        assert!(!ProtectReason::UserPinned.blocks_reclaim());
        assert_eq!(app.protect, vec![ProtectReason::Download, ProtectReason::UserPinned]);
    }

    #[test]
    fn inactive_memory_accounting_uses_inactive_states() {
        assert!(AppState::Suspended.is_inactive());
        assert!(AppState::Hibernated.is_inactive());
        for state in [
            AppState::Foreground,
            AppState::Visible,
            AppState::Background,
            AppState::Terminated,
        ] {
            assert!(!state.is_inactive());
        }
    }
}
