//! Health-check service for A/B updates (P9-T02).
//!
//! The slot state machine in [`crate::UpdateState`] decides what a rollback
//! means. This module supplies the missing service layer: it runs the boot
//! health probes, applies the retry policy, confirms the pending slot when the
//! probes pass, and rolls back when they keep failing or the confirmation
//! deadline expires.
//!
//! Probe execution is best-effort and never panics: a probe that cannot run is
//! reported as failed with its reason. A recorded report can be evaluated
//! without running anything, which is how the policy is tested on a host that
//! has no A/B slots.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{health_check, SlotId, SlotState, UpdateError, UpdateState};

/// How a single probe decides pass or fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProbeKind {
    /// Exit code 0 within the timeout.
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// The path exists (file or directory).
    PathExists { path: String },
    /// A systemd unit is active (`systemctl is-active --quiet`).
    UnitActive { unit: String },
    /// At least `min_mib` MiB are free on the filesystem holding `path`.
    FreeSpace { path: String, min_mib: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthProbe {
    pub id: String,
    pub kind: ProbeKind,
    /// A failed required probe fails the whole report.
    #[serde(default)]
    pub required: bool,
    #[serde(default = "default_probe_timeout")]
    pub timeout_secs: u64,
}

fn default_probe_timeout() -> u64 {
    10
}

impl HealthProbe {
    pub fn required(id: impl Into<String>, kind: ProbeKind) -> Self {
        Self {
            id: id.into(),
            kind,
            required: true,
            timeout_secs: default_probe_timeout(),
        }
    }

    pub fn optional(id: impl Into<String>, kind: ProbeKind) -> Self {
        Self {
            id: id.into(),
            kind,
            required: false,
            timeout_secs: default_probe_timeout(),
        }
    }

    pub fn run(&self) -> ProbeOutcome {
        let (passed, detail) = match &self.kind {
            ProbeKind::Command { program, args } => run_command(program, args, self.timeout_secs),
            ProbeKind::PathExists { path } => (
                Path::new(path).exists(),
                format!("{path} does not exist"),
            ),
            ProbeKind::UnitActive { unit } => run_command(
                "systemctl",
                &["is-active".into(), "--quiet".into(), unit.clone()],
                self.timeout_secs,
            ),
            ProbeKind::FreeSpace { path, min_mib } => free_space(path, *min_mib),
        };
        ProbeOutcome {
            id: self.id.clone(),
            required: self.required,
            passed,
            detail: if passed { String::new() } else { detail },
        }
    }
}

fn run_command(program: &str, args: &[String], timeout_secs: u64) -> (bool, String) {
    let child = Command::new(program).args(args).spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return (false, format!("cannot run {program}: {error}")),
    };
    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return (true, String::new()),
            Ok(Some(status)) => return (false, format!("{program} exited with {status}")),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return (false, format!("{program} timed out after {timeout_secs}s"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => return (false, format!("cannot wait for {program}: {error}")),
        }
    }
}

/// Free space on the filesystem that holds `path`, in MiB, via `df -Pk`.
fn free_space(path: &str, min_mib: u64) -> (bool, String) {
    let output = Command::new("df").args(["-Pk", path]).output();
    let output = match output {
        Ok(output) => output,
        Err(error) => return (false, format!("cannot run df: {error}")),
    };
    if !output.status.success() {
        return (false, format!("df failed for {path}"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let available_kib = text
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|value| value.parse::<u64>().ok());
    match available_kib {
        Some(kib) if kib / 1024 >= min_mib => (true, String::new()),
        Some(kib) => (
            false,
            format!("only {} MiB free on {path} (need {min_mib})", kib / 1024),
        ),
        None => (false, format!("cannot parse df output for {path}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub id: String,
    #[serde(default)]
    pub required: bool,
    pub passed: bool,
    #[serde(default)]
    pub detail: String,
}

/// The probes' observations for one boot attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthReport {
    #[serde(default)]
    pub probes: Vec<ProbeOutcome>,
}

impl HealthReport {
    pub fn new(probes: Vec<ProbeOutcome>) -> Self {
        Self { probes }
    }

    /// A report with no probes cannot confirm an update.
    pub fn passed(&self) -> bool {
        !self.probes.is_empty()
            && self
                .probes
                .iter()
                .all(|probe| probe.passed || !probe.required)
    }

    pub fn failed(&self) -> Vec<&ProbeOutcome> {
        self.probes.iter().filter(|probe| !probe.passed).collect()
    }

    pub fn failed_required(&self) -> Vec<&ProbeOutcome> {
        self.probes
            .iter()
            .filter(|probe| !probe.passed && probe.required)
            .collect()
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, text)
    }
}

/// Run a probe list and collect the report.
pub fn run_probes(probes: &[HealthProbe]) -> HealthReport {
    HealthReport::new(probes.iter().map(HealthProbe::run).collect())
}

/// The probes the boot service runs on a pending slot.
///
/// `boot-services` and `storage` are required; the rest describe a healthy
/// session and only lower the score/notes if they fail.
pub fn default_probes() -> Vec<HealthProbe> {
    vec![
        HealthProbe::required(
            "boot-services",
            ProbeKind::UnitActive {
                unit: "multi-user.target".into(),
            },
        ),
        HealthProbe::required(
            "storage",
            ProbeKind::FreeSpace {
                path: "/var".into(),
                min_mib: 200,
            },
        ),
        HealthProbe::required(
            "update-state",
            ProbeKind::PathExists {
                path: "/var/lib/aether/update".into(),
            },
        ),
        HealthProbe::optional(
            "socket",
            ProbeKind::Command {
                program: "systemctl".into(),
                args: vec!["is-active".into(), "--quiet".into(), "graphical.target".into()],
            },
        ),
        HealthProbe::optional(
            "audio",
            ProbeKind::Command {
                program: "pgrep".into(),
                args: vec!["-x".into(), "pipewire".into()],
            },
        ),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthPolicy {
    /// A pending slot must confirm within this many seconds of boot.
    pub deadline_secs: u64,
    /// Failed health checks allowed before rolling back.
    pub max_retries: u32,
    /// Cooldown between health checks.
    pub retry_after_secs: u64,
}

impl Default for HealthPolicy {
    fn default() -> Self {
        Self {
            deadline_secs: 120,
            max_retries: 2,
            retry_after_secs: 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RollbackReason {
    /// Required probes kept failing.
    FailedProbe,
    /// The slot did not confirm before the deadline.
    Deadline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum HealthAction {
    /// The running slot is already confirmed (or nothing is pending).
    NotPending,
    /// Mark-good happened; the update is now the active, good image.
    Confirmed { version: Option<String> },
    /// The bootloader must switch back to the previous good slot.
    RolledBack {
        to: SlotId,
        #[serde(rename = "because")]
        reason: RollbackReason,
    },
    /// Nothing decided yet; run the probes again after the cooldown.
    Waiting {
        attempts: u32,
        next_retry_in_secs: u64,
    },
}

/// Applies the health policy to an update state.
///
/// The service writes the retry counters into [`UpdateState`] so a separate
/// CLI invocation (or a reboot) continues where the last attempt stopped. The
/// slot state machine itself stays in [`UpdateState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthService {
    pub state: UpdateState,
    pub policy: HealthPolicy,
}

impl HealthService {
    pub fn new(state: UpdateState) -> Self {
        Self {
            state,
            policy: HealthPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: HealthPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Health-check attempts recorded for the pending slot.
    pub fn attempts(&self) -> u32 {
        self.state.health_attempts
    }

    pub fn last_attempt_at(&self) -> Option<u64> {
        self.state.last_health_at
    }

    /// True while the running slot is pending confirmation.
    pub fn pending(&self) -> bool {
        self.state
            .active_slot()
            .map(|slot| slot.state == SlotState::Pending)
            .unwrap_or(false)
    }

    /// Apply one report. `now` is Unix time in seconds.
    pub fn evaluate(
        &mut self,
        report: &HealthReport,
        now: u64,
    ) -> Result<HealthAction, UpdateError> {
        if !self.pending() {
            return Ok(HealthAction::NotPending);
        }
        self.state.health_attempts += 1;
        self.state.last_health_at = Some(now);

        if report.passed() {
            let version = self
                .state
                .active_slot()
                .and_then(|slot| slot.version.clone());
            health_check(&mut self.state, true)?;
            return Ok(HealthAction::Confirmed { version });
        }

        if self.state.health_attempts >= self.policy.max_retries.max(1) {
            health_check(&mut self.state, false)?;
            let to = self.state.active;
            return Ok(HealthAction::RolledBack {
                to,
                reason: RollbackReason::FailedProbe,
            });
        }

        Ok(HealthAction::Waiting {
            attempts: self.state.health_attempts,
            next_retry_in_secs: self.policy.retry_after_secs,
        })
    }

    /// Enforce the confirmation deadline. `boot_at` is when the pending slot
    /// started booting.
    pub fn enforce_deadline(
        &mut self,
        boot_at: u64,
        now: u64,
    ) -> Result<HealthAction, UpdateError> {
        if !self.pending() {
            return Ok(HealthAction::NotPending);
        }
        let elapsed = now.saturating_sub(boot_at);
        if elapsed >= self.policy.deadline_secs {
            // Deadline expiry is a health failure: the same rollback path runs.
            health_check(&mut self.state, false)?;
            let to = self.state.active;
            return Ok(HealthAction::RolledBack {
                to,
                reason: RollbackReason::Deadline,
            });
        }
        Ok(HealthAction::Waiting {
            attempts: self.state.health_attempts,
            next_retry_in_secs: self.policy.deadline_secs - elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_service() -> HealthService {
        // Mirrors the lib tests: slot A active and good, slot B pending.
        let mut state = UpdateState::default();
        {
            let slot = state.slots.get_mut("a").unwrap();
            slot.version = Some("1.0.0".into());
            slot.state = SlotState::Active;
        }
        state.last_good_version = Some("1.0.0".into());
        let target = state.active.other();
        crate::stage_pending(&mut state, target).unwrap();
        let slot = state.slots.get_mut("b").unwrap();
        slot.version = Some("1.1.0".into());
        state.active = target;
        HealthService::new(state)
    }

    fn report(passed: bool) -> HealthReport {
        HealthReport::new(vec![
            ProbeOutcome {
                id: "boot-services".into(),
                required: true,
                passed,
                detail: if passed { String::new() } else { "not reached".into() },
            },
            ProbeOutcome {
                id: "audio".into(),
                required: false,
                passed: false,
                detail: "no pipewire".into(),
            },
        ])
    }

    #[test]
    fn report_passes_with_only_optional_failures() {
        assert!(report(true).passed());
        assert!(!report(false).passed());
        assert_eq!(report(false).failed_required().len(), 1);
        assert_eq!(report(false).failed().len(), 2);
        assert!(!HealthReport::default().passed(), "an empty report confirms nothing");
    }

    #[test]
    fn passing_report_confirms_the_update() {
        let mut service = pending_service();
        assert!(service.pending());
        let action = service.evaluate(&report(true), 1_000).unwrap();
        assert_eq!(
            action,
            HealthAction::Confirmed {
                version: Some("1.1.0".into())
            }
        );
        assert!(!service.pending());
        assert_eq!(service.state.active, SlotId::B);
        assert_eq!(service.state.active_slot().unwrap().state, SlotState::Active);
        assert_eq!(service.state.slot(SlotId::A).unwrap().state, SlotState::Good);
        assert_eq!(service.attempts(), 0);
    }

    #[test]
    fn failing_report_retries_then_rolls_back() {
        let mut service = pending_service();
        assert_eq!(
            service.evaluate(&report(false), 1_000).unwrap(),
            HealthAction::Waiting {
                attempts: 1,
                next_retry_in_secs: 15
            }
        );
        assert_eq!(service.last_attempt_at(), Some(1_000));
        assert_eq!(
            service.evaluate(&report(false), 1_020).unwrap(),
            HealthAction::RolledBack {
                to: SlotId::A,
                reason: RollbackReason::FailedProbe
            }
        );
        assert_eq!(service.state.active, SlotId::A);
        assert_eq!(service.state.active_slot().unwrap().state, SlotState::Active);
        assert_eq!(service.state.slot(SlotId::B).unwrap().state, SlotState::Bad);
        assert!(!service.state.update_in_progress);
    }

    #[test]
    fn a_late_passing_report_still_confirms() {
        let mut service = pending_service();
        service.evaluate(&report(false), 1_000).unwrap();
        assert!(matches!(
            service.evaluate(&report(true), 1_010).unwrap(),
            HealthAction::Confirmed { .. }
        ));
    }

    #[test]
    fn deadline_rolls_back_a_silent_slot() {
        let mut service = pending_service();
        service.policy = HealthPolicy {
            deadline_secs: 120,
            ..HealthPolicy::default()
        };
        assert_eq!(
            service.enforce_deadline(500, 560).unwrap(),
            HealthAction::Waiting {
                attempts: 0,
                next_retry_in_secs: 60
            }
        );
        assert_eq!(
            service.enforce_deadline(500, 620).unwrap(),
            HealthAction::RolledBack {
                to: SlotId::A,
                reason: RollbackReason::Deadline
            }
        );
        assert_eq!(service.state.active, SlotId::A);
    }

    #[test]
    fn confirmed_slot_ignores_later_reports() {
        let mut service = HealthService::new(UpdateState::default());
        assert_eq!(
            service.evaluate(&report(false), 10).unwrap(),
            HealthAction::NotPending
        );
        assert_eq!(
            service.enforce_deadline(0, 10_000).unwrap(),
            HealthAction::NotPending
        );
        assert_eq!(service.attempts(), 0, "not-pending reports are not counted");
    }

    #[test]
    fn max_retries_of_one_rolls_back_immediately() {
        let mut service = pending_service().with_policy(HealthPolicy {
            max_retries: 1,
            ..HealthPolicy::default()
        });
        assert!(matches!(
            service.evaluate(&report(false), 1).unwrap(),
            HealthAction::RolledBack { .. }
        ));
    }

    #[test]
    fn retry_counter_survives_a_save_and_load() {
        let mut service = pending_service();
        service.evaluate(&report(false), 100).unwrap();
        assert_eq!(service.attempts(), 1);

        // A fresh process (the CLI) reloads the state and continues.
        let dir = std::env::temp_dir().join(format!("aether-health-retry-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        service.state.save(&path).unwrap();
        let loaded = UpdateState::load(&path).unwrap();
        assert_eq!(loaded.health_attempts, 1);
        assert_eq!(loaded.last_health_at, Some(100));

        let mut resumed = HealthService::new(loaded);
        assert!(matches!(
            resumed.evaluate(&report(false), 115).unwrap(),
            HealthAction::RolledBack {
                reason: RollbackReason::FailedProbe,
                ..
            }
        ));
        assert_eq!(resumed.state.health_attempts, 0);
        assert_eq!(resumed.state.last_health_at, None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn probes_really_run_on_this_host() {
        let probes = vec![
            HealthProbe::required(
                "existing",
                ProbeKind::PathExists {
                    path: ".".into(),
                },
            ),
            HealthProbe::required(
                "missing",
                ProbeKind::PathExists {
                    path: "/nonexistent/aether-health-probe".into(),
                },
            ),
            HealthProbe::required(
                "ok-command",
                ProbeKind::Command {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "exit 0".into()],
                },
            ),
            HealthProbe::optional(
                "bad-command",
                ProbeKind::Command {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "exit 4".into()],
                },
            ),
        ];
        let report = run_probes(&probes);
        let by_id = |id: &str| report.probes.iter().find(|p| p.id == id).unwrap();
        assert!(by_id("existing").passed);
        assert!(!by_id("missing").passed);
        assert!(by_id("ok-command").passed);
        assert!(!by_id("bad-command").passed);
        assert!(!report.passed(), "a failed required probe fails the report");
        let failed_ids: Vec<&str> = report
            .failed_required()
            .iter()
            .map(|probe| probe.id.as_str())
            .collect();
        assert_eq!(
            failed_ids,
            vec!["missing"],
            "the optional failure does not gate the report"
        );
    }

    #[test]
    fn command_timeout_is_a_failure() {
        let probe = HealthProbe {
            id: "slow".into(),
            kind: ProbeKind::Command {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "sleep 5".into()],
            },
            required: true,
            timeout_secs: 1,
        };
        let outcome = probe.run();
        assert!(!outcome.passed);
        assert!(outcome.detail.contains("timed out"));
    }

    #[test]
    fn free_space_probe_runs_df() {
        let outcome = HealthProbe::required(
            "disk",
            ProbeKind::FreeSpace {
                path: ".".into(),
                min_mib: 1,
            },
        )
        .run();
        assert!(outcome.passed, "detail: {}", outcome.detail);

        let huge = HealthProbe::required(
            "disk",
            ProbeKind::FreeSpace {
                path: ".".into(),
                min_mib: u64::MAX / 2048,
            },
        )
        .run();
        assert!(!huge.passed);
    }

    #[test]
    fn report_round_trips() {
        let report = report(false);
        let dir = std::env::temp_dir().join(format!("aether-health-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("report.json");
        report.save(&path).unwrap();
        assert_eq!(HealthReport::load(&path).unwrap(), report);
        let action = HealthAction::RolledBack {
            to: SlotId::B,
            reason: RollbackReason::Deadline,
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"decision\":\"rolled-back\""));
        assert!(json.contains("\"because\":\"deadline\""));
        let _ = fs::remove_dir_all(dir);
    }
}
