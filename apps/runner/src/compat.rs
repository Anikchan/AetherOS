//! Windows compatibility database (P6-T10, 04 section 7, F3 tiers).
//!
//! The 40 test slots are fixed by 04 §7; results are recorded as they are
//! measured. Nothing is pre-filled with a passing tier: a fresh database is
//! all `Untested` and the gate stays closed until 30 of 40 reach Tier A or B.

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatEntry {
    pub name: String,
    pub category: String,
    pub tier: CompatTier,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tested_on: Option<String>,
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
        let slots: &[(&str, usize)] = &[
            ("Browser", 3),
            ("Office and productivity", 8),
            ("Media", 6),
            ("Development", 7),
            ("Games", 8),
            ("Utilities", 8),
        ];
        let mut entries = Vec::new();
        for (category, count) in slots {
            for index in 1..=*count {
                entries.push(CompatEntry {
                    name: format!("{category} app {index}"),
                    category: (*category).to_string(),
                    tier: CompatTier::Untested,
                    notes: String::new(),
                    tested_on: None,
                });
            }
        }
        Self { entries }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
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

    pub fn summary(&self) -> CompatSummary {
        let mut summary = CompatSummary {
            total: self.entries.len(),
            ..Default::default()
        };
        for entry in &self.entries {
            match entry.tier {
                CompatTier::A => summary.tier_a += 1,
                CompatTier::B => summary.tier_b += 1,
                CompatTier::C => summary.tier_c += 1,
                CompatTier::D => summary.tier_d += 1,
                CompatTier::Untested => summary.untested += 1,
            }
        }
        summary.gate_a_or_b = summary.tier_a + summary.tier_b;
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
            "A: {} | B: {} | C: {} | D: {} | Untested: {}\n\n",
            summary.tier_a, summary.tier_b, summary.tier_c, summary.tier_d, summary.untested
        ));
        out.push_str("| App | Category | Tier | Notes | Tested on |\n|---|---|---|---|---|\n");
        for entry in &self.entries {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                entry.name,
                entry.category,
                entry.tier.label(),
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

    #[test]
    fn plan_has_exactly_forty_slots() {
        let db = CompatDb::test_plan();
        assert_eq!(db.entries.len(), 40);
        let summary = db.summary();
        assert_eq!(summary.untested, 40);
        assert_eq!(summary.gate_a_or_b, 0);
        assert!(!summary.gate_passes());
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
}
