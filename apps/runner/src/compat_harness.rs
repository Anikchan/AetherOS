//! Hardware test harness for the 40 Windows compatibility slots (P6-T10).
//!
//! The harness is deliberately hardware-agnostic: a [`HarnessSpec`] lists, per
//! slot, the installer to run and the commands that prove each check. On the
//! reference machine the commands are executed; without hardware the same spec
//! runs in dry-run mode (slots stay `Untested`) or imports observations that
//! were recorded earlier, so the scoring pipeline and the gate are exercised
//! on a normal development host.
//!
//! Commands are opaque to this crate: an install check runs the vendor
//! installer through the Windows engine, a window check uses whatever window
//! probe the machine has (`xdotool`, a screenshot diff, a small helper), and so
//! on. This keeps the harness usable before the engine helpers are final.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::compat::{
    CheckKind, CheckResult, CompatDb, CompatRun, CompatSummary, CompatVerdict, TEST_SLOTS,
};

/// How a slot was filled in this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    /// Nothing was executed; the slot keeps its current result.
    DryRun,
    /// Commands were executed on this machine.
    Executed,
    /// Observations recorded by an earlier (hardware) run were imported.
    Imported,
    /// The spec is an import run and this slot carries no observations.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCheck {
    pub check: CheckKind,
    /// Program to run, for example `wine` or `/usr/bin/xdotool`.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    120
}

impl HarnessCheck {
    pub fn new(check: CheckKind, program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            check,
            program: program.into(),
            args,
            timeout_secs: default_timeout(),
        }
    }

    /// Run the command; success is exit code 0 within the timeout.
    pub fn execute(&self) -> CheckResult {
        let started = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match started {
            Ok(child) => child,
            Err(error) => {
                return CheckResult::fail(self.check, format!("cannot run {}: {error}", self.program))
            }
        };
        let deadline = Instant::now() + Duration::from_secs(self.timeout_secs.max(1));
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    return CheckResult::pass(self.check)
                }
                Ok(Some(status)) => {
                    return CheckResult::fail(
                        self.check,
                        format!("{} exited with {status}", self.program),
                    )
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CheckResult::fail(
                        self.check,
                        format!("{} timed out after {}s", self.program, self.timeout_secs),
                    );
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(error) => {
                    return CheckResult::fail(
                        self.check,
                        format!("cannot wait for {}: {error}", self.program),
                    )
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSlot {
    /// Must match a [`crate::compat::CompatEntry`] name, for example `Games app 1`.
    pub name: String,
    /// The vendor installer the slot ships with.
    pub installer: PathBuf,
    /// Arguments passed to the launch check (the app itself, or its document).
    #[serde(default)]
    pub args: Vec<String>,
    /// Documented tweak applied before the run, if any (Tier B).
    #[serde(default)]
    pub tweak: Option<String>,
    #[serde(default)]
    pub checks: Vec<HarnessCheck>,
    /// Observations from an earlier run; non-empty means "import this slot".
    #[serde(default)]
    pub recorded: Vec<CheckResult>,
}

impl HarnessSlot {
    /// Default command set. These are the commands the reference machine runs;
    /// the paths and probes are meant to be adjusted once the engine helpers
    /// exist and are not executed in dry-run mode.
    pub fn template(name: &str) -> Self {
        Self {
            name: name.to_string(),
            installer: PathBuf::from(format!(
                "~/compat/{}.exe",
                name.to_ascii_lowercase().replace(' ', "-")
            )),
            args: Vec::new(),
            tweak: None,
            checks: vec![
                HarnessCheck::new(CheckKind::Install, "aether-runner", vec![]),
                HarnessCheck::new(CheckKind::Launch, "aether-runner", vec![]),
                HarnessCheck::new(CheckKind::Window, "xdotool", vec!["search".into(), "--name".into()]),
                HarnessCheck::new(CheckKind::Input, "aether-compat-probe", vec!["input".into()]),
                HarnessCheck::new(CheckKind::Audio, "aether-compat-probe", vec!["audio".into()]),
                HarnessCheck::new(CheckKind::Video, "aether-compat-probe", vec!["video".into()]),
                HarnessCheck::new(CheckKind::Files, "aether-compat-probe", vec!["files".into()]),
                HarnessCheck::new(CheckKind::Network, "aether-compat-probe", vec!["network".into()]),
                HarnessCheck::new(
                    CheckKind::Persistence,
                    "aether-compat-probe",
                    vec!["persistence".into()],
                ),
                HarnessCheck::new(
                    CheckKind::Stability,
                    "aether-compat-probe",
                    vec!["stability".into()],
                ),
            ],
            recorded: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSpec {
    /// Engine the machine runs, for example `wine-10.0`.
    #[serde(default)]
    pub engine: String,
    /// Machine label recorded with every result, for example `dell-xps`.
    #[serde(default)]
    pub machine: String,
    pub slots: Vec<HarnessSlot>,
}

impl HarnessSpec {
    /// A spec with one slot per 04 §7 entry and the default probes.
    pub fn template(engine: &str, machine: &str) -> Self {
        Self {
            engine: engine.to_string(),
            machine: machine.to_string(),
            slots: TEST_SLOTS
                .iter()
                .enumerate()
                .map(|(index, (category, _))| {
                    // Slot names mirror `CompatDb::test_plan` numbering.
                    let number = TEST_SLOTS[..=index]
                        .iter()
                        .filter(|(name, _)| name == category)
                        .count();
                    HarnessSlot::template(&format!("{category} app {number}"))
                })
                .collect(),
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessOutcome {
    pub slot: String,
    pub mode: RunMode,
    pub run: CompatRun,
    #[serde(default)]
    pub verdict: Option<CompatVerdict>,
}

/// What the harness does with a slot that has no recorded observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessMode {
    /// Report the slot without running anything.
    DryRun,
    /// Execute the check commands on this machine.
    Execute,
    /// Import-only: leave slots without observations untouched.
    Import,
}

/// Runs a spec against the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harness {
    mode: HarnessMode,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Harness {
    pub fn new(dry_run: bool) -> Self {
        Self::with_mode(if dry_run {
            HarnessMode::DryRun
        } else {
            HarnessMode::Execute
        })
    }

    pub fn with_mode(mode: HarnessMode) -> Self {
        Self { mode }
    }

    pub fn mode(&self) -> HarnessMode {
        self.mode
    }

    pub fn is_dry_run(&self) -> bool {
        self.mode == HarnessMode::DryRun
    }

    /// Execute (or import, or skip) one slot. Never writes to the database.
    pub fn run_slot(&self, slot: &HarnessSlot) -> HarnessOutcome {
        if !slot.recorded.is_empty() {
            return HarnessOutcome {
                slot: slot.name.clone(),
                mode: RunMode::Imported,
                run: CompatRun {
                    engine: String::new(),
                    tested_on: String::new(),
                    tweak: slot.tweak.clone(),
                    checks: slot.recorded.clone(),
                },
                verdict: None,
            };
        }
        if self.mode != HarnessMode::Execute {
            return HarnessOutcome {
                slot: slot.name.clone(),
                mode: if self.mode == HarnessMode::DryRun {
                    RunMode::DryRun
                } else {
                    RunMode::Skipped
                },
                run: CompatRun {
                    engine: String::new(),
                    tested_on: String::new(),
                    tweak: slot.tweak.clone(),
                    checks: Vec::new(),
                },
                verdict: None,
            };
        }
        let checks = slot
            .checks
            .iter()
            .map(HarnessCheck::execute)
            .collect::<Vec<_>>();
        HarnessOutcome {
            slot: slot.name.clone(),
            mode: RunMode::Executed,
            run: CompatRun {
                engine: String::new(),
                tested_on: String::new(),
                tweak: slot.tweak.clone(),
                checks,
            },
            verdict: None,
        }
    }
}

/// Run every slot and record the observations in `db`.
///
/// A spec that carries any recorded observations is treated as an import run:
/// the slots without observations stay untouched instead of executing
/// half-configured placeholder commands. A dry run never executes anything.
/// The machine and engine labels from the spec are attached to every recorded
/// run.
pub fn run(spec: &HarnessSpec, db: &mut CompatDb, dry_run: bool) -> Vec<HarnessOutcome> {
    let mode = if dry_run {
        HarnessMode::DryRun
    } else if spec.slots.iter().any(|slot| !slot.recorded.is_empty()) {
        HarnessMode::Import
    } else {
        HarnessMode::Execute
    };
    let harness = Harness::with_mode(mode);
    let mut outcomes = Vec::new();
    for slot in &spec.slots {
        let mut outcome = harness.run_slot(slot);
        outcome.run.engine = spec.engine.clone();
        outcome.run.tested_on = spec.machine.clone();
        if matches!(outcome.mode, RunMode::Executed | RunMode::Imported) {
            if let Some(verdict) = db.record_run(&slot.name, &outcome.run) {
                outcome.verdict = Some(verdict);
            }
        }
        outcomes.push(outcome);
    }
    outcomes
}

/// Summary of a harness run, for the CLI and evidence logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HarnessSummary {
    pub executed: usize,
    pub imported: usize,
    pub dry_run: usize,
    pub skipped: usize,
    pub unknown_slots: usize,
}

impl HarnessSummary {
    pub fn of(outcomes: &[HarnessOutcome]) -> Self {
        let mut summary = Self::default();
        for outcome in outcomes {
            match outcome.mode {
                RunMode::Executed => summary.executed += 1,
                RunMode::Imported => summary.imported += 1,
                RunMode::DryRun => summary.dry_run += 1,
                RunMode::Skipped => summary.skipped += 1,
            }
            if matches!(outcome.mode, RunMode::Executed | RunMode::Imported)
                && outcome.verdict.is_none()
            {
                summary.unknown_slots += 1;
            }
        }
        summary
    }
}

/// Convenience for the CLI: run a spec file into a database file.
pub fn run_file(spec_path: &Path, db_path: &Path, dry_run: bool) -> io::Result<(HarnessSummary, CompatSummary)> {
    let spec = HarnessSpec::load(spec_path)?;
    let mut db = CompatDb::load_or_test_plan(db_path);
    let outcomes = run(&spec, &mut db, dry_run);
    let harness = HarnessSummary::of(&outcomes);
    if !dry_run {
        db.save(db_path)?;
    }
    Ok((harness, db.summary()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing() -> Vec<CheckResult> {
        CheckKind::ALL
            .iter()
            .map(|kind| CheckResult::pass(*kind))
            .collect()
    }

    #[test]
    fn template_has_one_slot_per_test_plan_entry() {
        let spec = HarnessSpec::template("wine-10.0", "dell-xps");
        assert_eq!(spec.slots.len(), 40);
        let db = CompatDb::test_plan();
        for (slot, entry) in spec.slots.iter().zip(db.entries.iter()) {
            assert_eq!(&slot.name, &entry.name);
            assert!(!slot.checks.is_empty());
        }
    }

    #[test]
    fn dry_run_leaves_every_slot_untested() {
        let spec = HarnessSpec::template("wine-10.0", "dev-host");
        let mut db = CompatDb::test_plan();
        let outcomes = run(&spec, &mut db, true);
        assert_eq!(outcomes.len(), 40);
        assert!(outcomes.iter().all(|o| o.mode == RunMode::DryRun));
        let summary = db.summary();
        assert_eq!(summary.untested, 40);
        assert!(!summary.gate_passes());
        let harness = HarnessSummary::of(&outcomes);
        assert_eq!(harness.dry_run, 40);
        assert_eq!(harness.skipped, 0);
    }

    #[test]
    fn imported_results_open_the_gate_from_thirty_tiers() {
        let mut spec = HarnessSpec::template("wine-10.0", "dell-xps");
        for slot in spec.slots.iter_mut().take(30) {
            slot.recorded = passing();
        }
        let mut db = CompatDb::test_plan();
        let outcomes = run(&spec, &mut db, false);
        let summary = db.summary();
        assert_eq!(summary.tier_a, 30);
        assert_eq!(summary.untested, 10);
        assert!(summary.gate_passes());
        let harness = HarnessSummary::of(&outcomes);
        assert_eq!(harness.imported, 30);
        assert_eq!(harness.skipped, 10);
        assert!(outcomes[0].verdict.as_ref().unwrap().tier.passes_gate());
        assert!(db.find("Browser app 1").unwrap().engine.as_deref() == Some("wine-10.0"));
        assert_eq!(
            db.find("Browser app 1").unwrap().tested_on.as_deref(),
            Some("dell-xps")
        );
    }

    #[test]
    fn imported_partial_result_is_tier_c() {
        let mut spec = HarnessSpec::template("wine-10.0", "dell-xps");
        spec.slots.truncate(1);
        spec.slots[0].recorded = CheckKind::ALL
            .iter()
            .map(|kind| {
                if *kind == CheckKind::Video {
                    CheckResult::fail(*kind, "codec missing")
                } else {
                    CheckResult::pass(*kind)
                }
            })
            .collect();
        let mut db = CompatDb::test_plan();
        let outcomes = run(&spec, &mut db, false);
        assert_eq!(outcomes[0].verdict.as_ref().unwrap().tier, crate::compat::CompatTier::C);
        assert_eq!(db.find("Browser app 1").unwrap().tier, crate::compat::CompatTier::C);
    }

    #[test]
    fn unknown_slot_name_is_counted_and_not_recorded() {
        let mut spec = HarnessSpec::template("wine-10.0", "dell-xps");
        spec.slots = vec![HarnessSlot {
            recorded: passing(),
            ..HarnessSlot::template("Not a slot")
        }];
        let mut db = CompatDb::test_plan();
        let outcomes = run(&spec, &mut db, false);
        assert_eq!(HarnessSummary::of(&outcomes).unknown_slots, 1);
        assert!(db.find("Not a slot").is_none());
        assert_eq!(db.summary().untested, 40);
    }

    #[test]
    fn check_execution_reports_exit_status_and_timeouts() {
        let pass = HarnessCheck::new(CheckKind::Launch, "/bin/sh", vec!["-c".into(), "exit 0".into()]);
        assert!(pass.execute().passed);

        let fail = HarnessCheck::new(CheckKind::Stability, "/bin/sh", vec!["-c".into(), "exit 3".into()]);
        let result = fail.execute();
        assert!(!result.passed);
        assert!(result.detail.contains("exit"));

        let missing = HarnessCheck::new(CheckKind::Install, "/nonexistent/aether-compat", vec![]);
        assert!(!missing.execute().passed);

        let slow = HarnessCheck {
            check: CheckKind::Window,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 5".into()],
            timeout_secs: 1,
        };
        let result = slow.execute();
        assert!(!result.passed);
        assert!(result.detail.contains("timed out"));
    }

    #[test]
    fn spec_round_trips() {
        let spec = HarnessSpec::template("wine-10.0", "dell-xps");
        let dir = std::env::temp_dir().join(format!("aether-harness-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("spec.json");
        spec.save(&path).unwrap();
        let loaded = HarnessSpec::load(&path).unwrap();
        assert_eq!(loaded, spec);
        let _ = fs::remove_dir_all(dir);
    }
}
