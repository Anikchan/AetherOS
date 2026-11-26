//! Windows compatibility database (P6-T10, 04 section 7, F3 tiers).
//!
//! The 40 test slots are fixed by 04 §7; results are recorded as they are
//! measured. Nothing is pre-filled with a passing tier: a fresh database is
//! all `Untested` and the gate stays closed until 30 of 40 reach Tier A or B.
//!
//! A run is stored as a list of measured [`CheckResult`]s plus the tweak that
//! was needed (if any). [`grade`] turns those observations into a tier, so the
//! tier is always reproducible from the evidence instead of typed in by hand.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompatTier {
    /// Works with no changes.
    A,
    /// Works after a documented setting or prefix tweak.
    B,
    /// Partly works.
    C,
    /// Does not work.
    D,
    /// Not tested yet.
    Untested,
}

impl CompatTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::A => "A (works)",
            Self::B => "B (tweak needed)",
            Self::C => "C (partly works)",
            Self::D => "D (does not work)",
            Self::Untested => "Untested",
        }
    }

    /// Tier A or B counts towards the 30-of-40 gate (04 §7).
    pub fn passes_gate(self) -> bool {
        matches!(self, Self::A | Self::B)
    }

    /// Parse `A`/`a`/`tier a`/`untested`; used by the CLI and stored reports.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "a" | "tier a" => Some(Self::A),
            "b" | "tier b" => Some(Self::B),
            "c" | "tier c" => Some(Self::C),
            "d" | "tier d" => Some(Self::D),
            "untested" | "?" | "" => Some(Self::Untested),
            _ => None,
        }
    }

    /// Tier D apps point the user at dual boot (APP-AC-08).
    pub fn needs_dual_boot(self) -> bool {
        matches!(self, Self::D)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::C => "c",
            Self::D => "d",
            Self::Untested => "untested",
        }
    }
}

/// One measurable behaviour of a Windows app under the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    Install,
    Launch,
    Window,
    Input,
    Audio,
    Video,
    Files,
    Network,
    Persistence,
    Stability,
}

impl CheckKind {
    pub const ALL: [Self; 10] = [
        Self::Install,
        Self::Launch,
        Self::Window,
        Self::Input,
        Self::Audio,
        Self::Video,
        Self::Files,
        Self::Network,
        Self::Persistence,
        Self::Stability,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Launch => "launch",
            Self::Window => "window",
            Self::Input => "input",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Files => "files",
            Self::Network => "network",
            Self::Persistence => "persistence",
            Self::Stability => "stability",
        }
    }

    /// Points out of 100 when every check passes. `Install`, `Launch`,
    /// `Window` and `Stability` dominate because the app is unusable without
    /// them; the rest describe partial behaviour.
    pub fn weight(self) -> u32 {
        match self {
            Self::Install | Self::Launch | Self::Window | Self::Stability => 20,
            Self::Input => 10,
            Self::Video => 10,
            Self::Audio | Self::Files | Self::Network | Self::Persistence => 5,
        }
    }

    /// A failure here cannot be a Tier A/B app.
    pub fn is_gate(self) -> bool {
        matches!(
            self,
            Self::Install | Self::Launch | Self::Window | Self::Stability
        )
    }
}

/// A single observation from a test run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    #[serde(rename = "check")]
    pub kind: CheckKind,
    pub passed: bool,
    #[serde(default)]
    pub detail: String,
}

impl CheckResult {
    pub fn new(kind: CheckKind, passed: bool, detail: impl Into<String>) -> Self {
        Self {
            kind,
            passed,
            detail: detail.into(),
        }
    }

    pub fn pass(kind: CheckKind) -> Self {
        Self::new(kind, true, "")
    }

    pub fn fail(kind: CheckKind, detail: impl Into<String>) -> Self {
        Self::new(kind, false, detail)
    }
}

/// What one test run measured. The tier is derived from this, never typed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatRun {
    /// Engine that ran the app, for example `wine-10.0`.
    #[serde(default)]
    pub engine: String,
    /// Machine or date the run happened on, for example `dell-xps (2026-12-01)`.
    #[serde(default)]
    pub tested_on: String,
    /// Documented prefix tweak the app needed. Any tweak means Tier B at best.
    #[serde(default)]
    pub tweak: Option<String>,
    #[serde(default)]
    pub checks: Vec<CheckResult>,
}

impl CompatRun {
    pub fn new(engine: impl Into<String>, tested_on: impl Into<String>) -> Self {
        Self {
            engine: engine.into(),
            tested_on: tested_on.into(),
            tweak: None,
            checks: Vec::new(),
        }
    }

    pub fn with_checks(mut self, checks: Vec<CheckResult>) -> Self {
        self.checks = checks;
        self
    }

    pub fn with_tweak(mut self, tweak: impl Into<String>) -> Self {
        self.tweak = Some(tweak.into());
        self
    }
}

/// Tier plus the arithmetic behind it, so a report can be audited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatVerdict {
    pub tier: CompatTier,
    /// 0-100 over the full check set; missing checks score 0.
    pub score: u32,
    #[serde(default)]
    pub failed: Vec<CheckKind>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

/// Turn measured checks into a tier (F3 §3, 04 §7).
///
/// - no checks: `Untested`
/// - a gate check (install/launch/window/stability) failed: `D`
/// - all supplied checks pass and no tweak was needed: `A`
/// - a documented tweak was needed: `B`
/// - some non-gate check failed without a documented tweak: `C`
pub fn grade(checks: &[CheckResult], tweak: Option<&str>) -> CompatVerdict {
    if checks.is_empty() {
        return CompatVerdict {
            tier: CompatTier::Untested,
            score: 0,
            failed: Vec::new(),
            reasons: vec!["no checks recorded".into()],
        };
    }

    let full_weight: u32 = CheckKind::ALL.iter().map(|kind| kind.weight()).sum();
    let mut earned = 0;
    let mut failed = Vec::new();
    let mut reasons = Vec::new();
    let mut gate_failed: Option<CheckKind> = None;
    let mut duplicate_detail: Vec<String> = Vec::new();

    for result in checks {
        if result.passed {
            earned += result.kind.weight();
        } else if !failed.contains(&result.kind) {
            failed.push(result.kind);
            if gate_failed.is_none() && result.kind.is_gate() {
                gate_failed = Some(result.kind);
            }
        }
        if !result.passed && !result.detail.trim().is_empty() {
            duplicate_detail.push(format!("{}: {}", result.kind.label(), result.detail.trim()));
        }
    }

    let score = ((earned as f64 / full_weight as f64) * 100.0).round() as u32;

    let tier = if gate_failed.is_some() {
        CompatTier::D
    } else if failed.is_empty() && tweak.is_none() {
        CompatTier::A
    } else if tweak.is_some() {
        CompatTier::B
    } else {
        CompatTier::C
    };

    if let Some(kind) = gate_failed {
        reasons.push(format!("{} failed; the app cannot be used", kind.label()));
    }
    if let Some(tweak) = tweak {
        reasons.push(format!("needed the documented tweak: {tweak}"));
    }
    if !failed.is_empty() && failed.iter().any(|kind| !kind.is_gate()) && tweak.is_none() {
        reasons.push(format!(
            "partial: {} failed with no documented tweak",
            failed
                .iter()
                .map(|kind| kind.label())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    reasons.extend(duplicate_detail);
    if reasons.is_empty() {
        reasons.push("all checks passed with no tweak".into());
    }

    CompatVerdict {
        tier,
        score,
        failed,
        reasons,
    }
}

/// 04 §7 slot table: (category, archetype) in test order. Exactly 40 slots.
pub const TEST_SLOTS: [(&str, &str); 40] = [
    ("Browser", "Chromium-based browser"),
    ("Browser", "Firefox-based browser"),
    ("Browser", "Other browser engine (WebKit/Blink fork)"),
    ("Office and productivity", "Word processor"),
    ("Office and productivity", "Spreadsheet"),
    ("Office and productivity", "Presentation"),
    ("Office and productivity", "PDF reader"),
    ("Office and productivity", "Note app"),
    ("Office and productivity", "Email client"),
    ("Office and productivity", "Calendar/task manager"),
    ("Office and productivity", "PDF printer/driver-dependent app"),
    ("Media", "Video player"),
    ("Media", "Audio player"),
    ("Media", "Video editor"),
    ("Media", "Image editor"),
    ("Media", "Media converter"),
    ("Media", "Streaming client"),
    ("Development", "Code editor"),
    ("Development", "IDE with debugger"),
    ("Development", "Terminal tool (SSH/serial)"),
    ("Development", "Database client"),
    ("Development", "Version-control GUI"),
    ("Development", "Container GUI"),
    ("Development", "API/HTTP client"),
    ("Games", "DirectX 11 game"),
    ("Games", "DirectX 12 game"),
    ("Games", "Indie game (OpenGL)"),
    ("Games", "Launcher-based game"),
    ("Games", "Unity game"),
    ("Games", "Unreal Engine game"),
    ("Games", "Older 32-bit game"),
    ("Games", "Anti-cheat multiplayer game"),
    ("Utilities", "Archive tool"),
    ("Utilities", "File sync client"),
    ("Utilities", "Password manager"),
    ("Utilities", "System monitor"),
    ("Utilities", "Screenshot/annotation app"),
    ("Utilities", "Remote desktop client"),
    ("Utilities", "VPN client"),
    ("Utilities", "Backup utility"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatEntry {
    /// Stable key used by the CLI and the harness, for example `Browser app 1`.
    pub name: String,
    pub category: String,
    /// What the slot represents (04 §7 examples), for example a Chromium browser.
    #[serde(default)]
    pub archetype: String,
    pub tier: CompatTier,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tested_on: Option<String>,
    /// Engine version the result was measured with.
    #[serde(default)]
    pub engine: Option<String>,
    /// Documented prefix tweak that made the app work (Tier B).
    #[serde(default)]
    pub tweak: Option<String>,
    /// 0-100 score from [`grade`].
    #[serde(default)]
    pub score: Option<u32>,
    #[serde(default)]
    pub evidence: Vec<CheckResult>,
}

impl CompatEntry {
    pub fn slot(&self) -> usize {
        self.name
            .rsplit(' ')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatDb {
    pub entries: Vec<CompatEntry>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatSummary {
    pub total: usize,
    pub tier_a: usize,
    pub tier_b: usize,
    pub tier_c: usize,
    pub tier_d: usize,
    pub untested: usize,
    pub gate_a_or_b: usize,
    /// Mean score over the tested slots.
    pub avg_score: u32,
}

impl CompatSummary {
    /// P6-T10: 30 of 40 in Tier A or B.
    pub fn gate_passes(&self) -> bool {
        self.total == 40 && self.gate_a_or_b >= 30
    }
}

impl CompatDb {
    /// The 40 slots from 04 §7, all Untested.
    pub fn test_plan() -> Self {
        let mut counters: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        let mut entries = Vec::new();
        for (category, archetype) in TEST_SLOTS {
            let counter = counters.entry(category).or_insert(0);
            *counter += 1;
            entries.push(CompatEntry {
                name: format!("{category} app {counter}"),
                category: category.to_string(),
                archetype: archetype.to_string(),
                tier: CompatTier::Untested,
                notes: String::new(),
                tested_on: None,
                engine: None,
                tweak: None,
                score: None,
                evidence: Vec::new(),
            });
        }
        Self { entries }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Load a recorded database, or start from the empty 40-slot plan.
    pub fn load_or_test_plan(path: &Path) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::test_plan())
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }

    pub fn find(&self, name: &str) -> Option<&CompatEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub fn set_result(
        &mut self,
        name: &str,
        tier: CompatTier,
        notes: &str,
        tested_on: &str,
    ) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.tier = tier;
            entry.notes = notes.to_string();
            entry.tested_on = Some(tested_on.to_string());
            true
        } else {
            false
        }
    }

    /// Record a measured run: grade it, then store the evidence and the tier.
    pub fn record_run(&mut self, name: &str, run: &CompatRun) -> Option<CompatVerdict> {
        let verdict = grade(&run.checks, run.tweak.as_deref());
        let entry = self.entries.iter_mut().find(|e| e.name == name)?;
        entry.tier = verdict.tier;
        entry.score = Some(verdict.score);
        entry.evidence = run.checks.clone();
        entry.tweak = run.tweak.clone();
        entry.engine = if run.engine.is_empty() {
            None
        } else {
            Some(run.engine.clone())
        };
        entry.tested_on = if run.tested_on.is_empty() {
            None
        } else {
            Some(run.tested_on.clone())
        };
        entry.notes = verdict.reasons.join("; ");
        Some(verdict)
    }

    pub fn summary(&self) -> CompatSummary {
        let mut summary = CompatSummary {
            total: self.entries.len(),
            ..Default::default()
        };
        let mut scored = 0usize;
        let mut score_total = 0u32;
        for entry in &self.entries {
            match entry.tier {
                CompatTier::A => summary.tier_a += 1,
                CompatTier::B => summary.tier_b += 1,
                CompatTier::C => summary.tier_c += 1,
                CompatTier::D => summary.tier_d += 1,
                CompatTier::Untested => summary.untested += 1,
            }
            if let Some(score) = entry.score {
                if entry.tier != CompatTier::Untested {
                    scored += 1;
                    score_total += score;
                }
            }
        }
        summary.gate_a_or_b = summary.tier_a + summary.tier_b;
        summary.avg_score = if scored == 0 {
            0
        } else {
            (score_total as f64 / scored as f64).round() as u32
        };
        summary
    }

    /// Markdown result table for F3/evidence.
    pub fn report(&self) -> String {
        let summary = self.summary();
        let mut out = String::from("# Windows compatibility results (P6-T10)\n\n");
        out.push_str(&format!(
            "Gate: 30 of 40 in Tier A or B — **{}** ({} of 40)\n\n",
            if summary.gate_passes() { "PASS" } else { "OPEN" },
            summary.gate_a_or_b
        ));
        out.push_str(&format!(
            "A: {} | B: {} | C: {} | D: {} | Untested: {} | Mean score: {}\n\n",
            summary.tier_a,
            summary.tier_b,
            summary.tier_c,
            summary.tier_d,
            summary.untested,
            summary.avg_score
        ));
        out.push_str(
            "| Slot | Category | Archetype | Tier | Score | Tweak | Notes | Tested on |\n|---|---|---|---|---|---|---|---|\n",
        );
        for entry in &self.entries {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                entry.name,
                entry.category,
                if entry.archetype.is_empty() {
                    "—"
                } else {
                    &entry.archetype
                },
                entry.tier.label(),
                entry
                    .score
                    .map(|score| score.to_string())
                    .unwrap_or_else(|| "—".into()),
                entry.tweak.clone().unwrap_or_else(|| "—".into()),
                entry.notes,
                entry.tested_on.clone().unwrap_or_else(|| "—".into())
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_pass() -> Vec<CheckResult> {
        CheckKind::ALL
            .iter()
            .map(|kind| CheckResult::pass(*kind))
            .collect()
    }

    #[test]
    fn plan_has_exactly_forty_slots() {
        let db = CompatDb::test_plan();
        assert_eq!(db.entries.len(), 40);
        assert_eq!(db.entries.len(), TEST_SLOTS.len());
        let summary = db.summary();
        assert_eq!(summary.untested, 40);
        assert_eq!(summary.gate_a_or_b, 0);
        assert_eq!(summary.avg_score, 0);
        assert!(!summary.gate_passes());
        assert!(db.entries.iter().all(|entry| !entry.archetype.is_empty()));
    }

    #[test]
    fn plan_matches_the_forty_slot_categories() {
        let mut counts = std::collections::BTreeMap::new();
        for entry in CompatDb::test_plan().entries {
            *counts.entry(entry.category).or_insert(0) += 1;
        }
        assert_eq!(counts["Browser"], 3);
        assert_eq!(counts["Office and productivity"], 8);
        assert_eq!(counts["Media"], 6);
        assert_eq!(counts["Development"], 7);
        assert_eq!(counts["Games"], 8);
        assert_eq!(counts["Utilities"], 8);
    }

    #[test]
    fn gate_opens_at_thirty_a_or_b() {
        let mut db = CompatDb::test_plan();
        for entry in db.entries.iter_mut().take(30) {
            entry.tier = CompatTier::A;
        }
        let summary = db.summary();
        assert_eq!(summary.gate_a_or_b, 30);
        assert!(summary.gate_passes());

        db.entries[0].tier = CompatTier::C;
        assert!(!db.summary().gate_passes());

        db.entries[0].tier = CompatTier::B;
        assert!(db.summary().gate_passes());
    }

    #[test]
    fn grading_covers_every_tier() {
        let a = grade(&all_pass(), None);
        assert_eq!(a.tier, CompatTier::A);
        assert_eq!(a.score, 100);

        let b = grade(&all_pass(), Some("winetricks vcrun2019"));
        assert_eq!(b.tier, CompatTier::B);
        assert_eq!(b.score, 100);
        assert!(b.reasons.iter().any(|reason| reason.contains("vcrun2019")));

        let with_audio_missing = CheckKind::ALL
            .iter()
            .map(|kind| {
                if *kind == CheckKind::Audio {
                    CheckResult::fail(*kind, "no device in prefix")
                } else {
                    CheckResult::pass(*kind)
                }
            })
            .collect::<Vec<_>>();
        let c = grade(&with_audio_missing, None);
        assert_eq!(c.tier, CompatTier::C);
        assert_eq!(c.score, 96);
        assert_eq!(c.failed, vec![CheckKind::Audio]);

        let d = grade(&[CheckResult::fail(CheckKind::Launch, "exit 53")], None);
        assert_eq!(d.tier, CompatTier::D);
        assert!(d.score < 100);

        let untested = grade(&[], None);
        assert_eq!(untested.tier, CompatTier::Untested);
        assert_eq!(untested.score, 0);
    }

    #[test]
    fn gate_failure_beats_a_tweak() {
        let checks = CheckKind::ALL
            .iter()
            .map(|kind| {
                if *kind == CheckKind::Window {
                    CheckResult::fail(*kind, "no window")
                } else {
                    CheckResult::pass(*kind)
                }
            })
            .collect::<Vec<_>>();
        let verdict = grade(&checks, Some("dxvk"));
        assert_eq!(verdict.tier, CompatTier::D);
        assert!(verdict.reasons[0].contains("window"));
    }

    #[test]
    fn score_drops_for_missing_checks() {
        // Only install passed: the run proves nothing about the rest.
        let verdict = grade(&[CheckResult::pass(CheckKind::Install)], None);
        assert_eq!(verdict.tier, CompatTier::A);
        assert_eq!(verdict.score, 17, "20 of 120 possible points");
    }

    #[test]
    fn tier_parsing_and_dual_boot_flag() {
        assert_eq!(CompatTier::parse("A"), Some(CompatTier::A));
        assert_eq!(CompatTier::parse("tier b"), Some(CompatTier::B));
        assert_eq!(CompatTier::parse("?"), Some(CompatTier::Untested));
        assert_eq!(CompatTier::parse("x"), None);
        assert!(CompatTier::D.needs_dual_boot());
        assert!(!CompatTier::B.needs_dual_boot());
        assert!(CompatTier::B.passes_gate());
    }

    #[test]
    fn results_round_trip_and_report() {
        let mut db = CompatDb::test_plan();
        assert!(db.set_result("Browser app 1", CompatTier::B, "needs --no-sandbox", "Dell"));
        assert!(!db.set_result("Missing", CompatTier::A, "", ""));
        let dir = std::env::temp_dir().join(format!("aether-compat-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("compat.json");
        db.save(&path).unwrap();
        let loaded = CompatDb::load(&path).unwrap();
        assert_eq!(loaded.entries[0].tier, CompatTier::B);
        let report = loaded.report();
        assert!(report.contains("Gate: 30 of 40"));
        assert!(report.contains("needs --no-sandbox"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn record_run_stores_the_evidence_and_tier() {
        let mut db = CompatDb::test_plan();
        let run = CompatRun::new("wine-10.0", "dell-xps (2026-12-01)").with_checks(all_pass());
        let verdict = db.record_run("Games app 1", &run).unwrap();
        assert_eq!(verdict.tier, CompatTier::A);
        let entry = db.find("Games app 1").unwrap();
        assert_eq!(entry.tier, CompatTier::A);
        assert_eq!(entry.score, Some(100));
        assert_eq!(entry.engine.as_deref(), Some("wine-10.0"));
        assert_eq!(entry.evidence.len(), 10);
        assert_eq!(db.summary().tier_a, 1);
        assert_eq!(db.summary().avg_score, 100);
        assert_eq!(db.summary().total, 40);
        assert!(db.record_run("Nope", &run).is_none());
    }
}
