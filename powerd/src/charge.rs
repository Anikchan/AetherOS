//! Charge-limit control (P5-T12, F7 PWR-08, D5 section 2).
//!
//! Writes `/sys/class/power_supply/BAT*/charge_control_end_threshold` where
//! supported. Values are clamped to 40–100%; unsupported hardware hides the
//! control instead of failing (D5 section 10).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::types::ChargeLimiter;

/// File-based charge limiter.
#[derive(Debug, Clone)]
pub struct SysfsChargeLimiter {
    path: PathBuf,
}

impl SysfsChargeLimiter {
    pub fn system() -> Self {
        Self::at("/sys/class/power_supply/BAT0/charge_control_end_threshold")
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ChargeLimiter for SysfsChargeLimiter {
    fn supported(&self) -> bool {
        self.path.exists()
    }

    fn set_limit(&self, percent: u8) -> io::Result<()> {
        if !self.supported() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("charge limit is not supported ({})", self.path.display()),
            ));
        }
        let clamped = crate::policy::clamp_charge_limit(percent);
        fs::write(&self.path, clamped.to_string())
    }

    fn limit(&self) -> io::Result<Option<u8>> {
        if !self.supported() {
            return Ok(None);
        }
        let text = fs::read_to_string(&self.path)?;
        Ok(text.trim().parse::<u8>().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-charge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("charge_control_end_threshold")
    }

    #[test]
    fn unsupported_hardware_reports_no_limit() {
        let limiter = SysfsChargeLimiter::at("/definitely/not/supported");
        assert!(!limiter.supported());
        assert_eq!(limiter.limit().unwrap(), None);
        assert_eq!(
            limiter.set_limit(80).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn limit_is_applied_clamped_and_read_back() {
        let path = temp_path();
        fs::write(&path, "100").unwrap();
        let limiter = SysfsChargeLimiter::at(&path);
        assert!(limiter.supported());

        limiter.set_limit(80).unwrap();
        assert_eq!(limiter.limit().unwrap(), Some(80));
        assert_eq!(fs::read_to_string(&path).unwrap(), "80");

        limiter.set_limit(10).unwrap();
        assert_eq!(limiter.limit().unwrap(), Some(40));

        limiter.set_limit(200).unwrap();
        assert_eq!(limiter.limit().unwrap(), Some(100));

        let _ = fs::remove_file(&path);
    }
}
