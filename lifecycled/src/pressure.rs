//! Memory pressure signals (P5-T05, D3 section 6).
//!
//! Reads `/proc/pressure/memory` (PSI). Kernel note from spike S3 (P0-T09):
//! PSI can be absent when `CONFIG_PSI` is off. A missing file is treated as
//! "pressure unavailable" and everything falls back to reclaim-only behavior
//! instead of crashing (D3 section 2 note).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PressureLevel {
    Normal,
    Elevated,
    Critical,
}

impl PressureLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::Critical => "critical",
        }
    }
}

/// One PSI sample: the `avg10` values of the `some` and `full` lines.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PsiSample {
    pub some_avg10: f32,
    pub some_avg60: f32,
    pub some_avg300: f32,
    pub full_avg10: f32,
    pub full_avg60: f32,
    pub full_avg300: f32,
}

/// Thresholds from D3 section 6 (tuned in P5-T05 on the 4 GB machine).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureThresholds {
    /// `some avg10` at or above this value means Elevated.
    pub elevated_some: f32,
    /// `full avg10` at or above this value also means Elevated.
    pub elevated_full: f32,
    /// `full avg10` at or above this value means Critical.
    pub critical_full: f32,
}

impl Default for PressureThresholds {
    fn default() -> Self {
        Self {
            elevated_some: 10.0,
            elevated_full: 1.0,
            critical_full: 5.0,
        }
    }
}

impl PressureThresholds {
    pub fn classify(&self, sample: PsiSample) -> PressureLevel {
        if sample.full_avg10 >= self.critical_full {
            PressureLevel::Critical
        } else if sample.some_avg10 >= self.elevated_some || sample.full_avg10 >= self.elevated_full
        {
            PressureLevel::Elevated
        } else {
            PressureLevel::Normal
        }
    }
}

#[derive(Debug, Error)]
pub enum PressureError {
    #[error("PSI is not available (missing {0}); CONFIG_PSI may be off")]
    Unavailable(PathBuf),
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("malformed PSI line: {0:?}")]
    Malformed(String),
}

/// Parse the contents of `/proc/pressure/memory`.
///
/// Format:
/// ```text
/// some avg10=0.00 avg60=0.00 avg300=0.00 total=0
/// full avg10=0.00 avg60=0.00 avg300=0.00 total=0
/// ```
pub fn parse_pressure(text: &str) -> Result<PsiSample, PressureError> {
    let mut sample = PsiSample::default();
    let mut saw_some = false;
    let mut saw_full = false;

    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(kind) = parts.next() else { continue };
        let mut values = [0f32; 3];
        let mut idx = 0;
        for part in parts {
            if let Some(rest) = part.strip_prefix("avg") {
                let Some((_, value)) = rest.split_once('=') else {
                    continue;
                };
                if idx < 3 {
                    values[idx] = value
                        .parse::<f32>()
                        .map_err(|_| PressureError::Malformed(line.to_string()))?;
                    idx += 1;
                }
            }
        }
        match kind {
            "some" => {
                sample.some_avg10 = values[0];
                sample.some_avg60 = values[1];
                sample.some_avg300 = values[2];
                saw_some = true;
            }
            "full" => {
                sample.full_avg10 = values[0];
                sample.full_avg60 = values[1];
                sample.full_avg300 = values[2];
                saw_full = true;
            }
            _ => {}
        }
    }

    if !saw_some && !saw_full {
        return Err(PressureError::Malformed(text.to_string()));
    }
    Ok(sample)
}

/// Reader for the PSI memory file. `path` is configurable for tests.
#[derive(Debug, Clone)]
pub struct PsiReader {
    path: PathBuf,
    thresholds: PressureThresholds,
}

impl PsiReader {
    pub fn system() -> Self {
        Self::at("/proc/pressure/memory")
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            thresholds: PressureThresholds::default(),
        }
    }

    pub fn with_thresholds(mut self, thresholds: PressureThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn available(&self) -> bool {
        self.path.exists()
    }

    pub fn sample(&self) -> Result<PsiSample, PressureError> {
        if !self.path.exists() {
            return Err(PressureError::Unavailable(self.path.clone()));
        }
        let text = fs::read_to_string(&self.path).map_err(|source| PressureError::Io {
            path: self.path.clone(),
            source,
        })?;
        parse_pressure(&text)
    }

    /// Current level. Missing PSI is Normal (reclaim-only fallback) and the
    /// caller is told through [`Self::available`].
    pub fn level(&self) -> PressureLevel {
        match self.sample() {
            Ok(sample) => self.thresholds.classify(sample),
            Err(_) => PressureLevel::Normal,
        }
    }

    /// Simple polling loop used until the event-driven `poll()` integration
    /// lands with the daemon's epoll loop. Returns when `stop` is set.
    pub fn watch(
        &self,
        interval: Duration,
        mut stop: impl FnMut() -> bool,
        mut on_change: impl FnMut(PressureLevel),
    ) {
        let mut last = PressureLevel::Normal;
        while !stop() {
            let level = self.level();
            if level != last {
                on_change(level);
                last = level;
            }
            std::thread::sleep(interval);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aether-psi-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn parses_real_psi_output() {
        let text = "some avg10=0.42 avg60=0.10 avg300=0.01 total=1234\n\
                    full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        let sample = parse_pressure(text).unwrap();
        assert!((sample.some_avg10 - 0.42).abs() < 1e-6);
        assert!((sample.some_avg60 - 0.10).abs() < 1e-6);
        assert!((sample.full_avg10 - 0.0).abs() < 1e-6);
    }

    #[test]
    fn classifies_thresholds() {
        let t = PressureThresholds::default();
        let normal = PsiSample {
            some_avg10: 9.9,
            full_avg10: 0.9,
            ..Default::default()
        };
        assert_eq!(t.classify(normal), PressureLevel::Normal);

        let elevated = PsiSample {
            some_avg10: 10.0,
            ..Default::default()
        };
        assert_eq!(t.classify(elevated), PressureLevel::Elevated);

        let elevated_full = PsiSample {
            full_avg10: 1.0,
            ..Default::default()
        };
        assert_eq!(t.classify(elevated_full), PressureLevel::Elevated);

        let critical = PsiSample {
            full_avg10: 5.0,
            ..Default::default()
        };
        assert_eq!(t.classify(critical), PressureLevel::Critical);
    }

    #[test]
    fn missing_psi_is_normal_and_reported_unavailable() {
        let reader = PsiReader::at("/definitely/not/a/psi/file");
        assert!(!reader.available());
        assert_eq!(reader.level(), PressureLevel::Normal);
        assert!(matches!(
            reader.sample(),
            Err(PressureError::Unavailable(_))
        ));
    }

    #[test]
    fn reads_and_classifies_a_file() {
        let path = temp_file("some avg10=12.0 avg60=4.0 avg300=1.0 total=9\n");
        let reader = PsiReader::at(&path);
        assert!(reader.available());
        assert_eq!(reader.level(), PressureLevel::Elevated);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn displays_levels() {
        assert_eq!(PressureLevel::Normal.as_str(), "normal");
        assert_eq!(PressureLevel::Elevated.as_str(), "elevated");
        assert_eq!(PressureLevel::Critical.as_str(), "critical");
    }
}
