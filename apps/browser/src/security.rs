//! Browser security posture and the security update channel (P4-T18b).
//!
//! The browser has no updater of its own: its rendering engine (WebKitGTK) is
//! part of the OS image and therefore ships through the A/B update channel
//! described in D4 (P9). What this module owns is the part that is testable
//! without the image:
//!
//! * the **channel contract** — `stable` or `beta`, persisted in
//!   `~/.config/aether/browser/security.toml` and reported together with the
//!   update source, so a support run can tell which image stream the engine
//!   came from;
//! * the **security floor** — the minimum WebKitGTK version that carries the
//!   fixes the browser relies on ([`MIN_WEBKITGTK`]); an older engine is a
//!   failed check, not a warning;
//! * the **hardening policy** — HTTPS-only, mixed content blocked, third-party
//!   cookies blocked. These are not preferences: [`SecurityPolicy::problems`]
//!   treats a disabled baseline as a failure.
//!
//! `aether-browser --security-status` prints the posture and
//! `--security-check` adds the requirement that the engine is actually
//! installed (the development host has only the adapter, the image has the
//! engine).

use std::path::{Path, PathBuf};

/// Oldest WebKitGTK release the browser accepts (the stable-channel security
/// floor from D5; bump with the image's WebKitGTK update).
pub const MIN_WEBKITGTK: &str = "2.44";

/// Config file below the browser profile root.
pub const SETTINGS_FILE: &str = "security.toml";

/// The update stream the browser and its engine follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::Stable, Channel::Beta];

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    /// Unknown values fall back to Stable: the safer stream wins.
    pub fn parse(value: &str) -> Channel {
        match value.trim().to_ascii_lowercase().as_str() {
            "beta" => Channel::Beta,
            _ => Channel::Stable,
        }
    }
}

/// How engine security updates reach a running system (P9 A/B updates).
pub const UPDATE_SOURCE: &str = "os-image";

/// The WebKitGTK engine in use. `version` is `None` when only the adapter is
/// present (the macOS development host and headless CI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebEngine {
    pub name: &'static str,
    pub version: Option<String>,
}

impl WebEngine {
    pub fn from_version(version: impl Into<String>) -> WebEngine {
        WebEngine {
            name: "webkitgtk",
            version: Some(version.into()),
        }
    }

    /// Adapter-only host: no engine installed.
    pub fn absent() -> WebEngine {
        WebEngine {
            name: "webkitgtk",
            version: None,
        }
    }

    /// Detect the engine: `AETHER_WEBKITGTK_VERSION` first (the image and the
    /// acceptance harness set it), then `pkg-config`. Never panics.
    pub fn detect() -> WebEngine {
        if let Ok(version) = std::env::var("AETHER_WEBKITGTK_VERSION") {
            if !version.trim().is_empty() {
                return WebEngine::from_version(version.trim());
            }
        }
        for package in ["webkit2gtk-4.1", "webkit2gtk-4.0"] {
            let output = std::process::Command::new("pkg-config")
                .args(["--modversion", package])
                .output();
            if let Ok(output) = output {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !version.is_empty() {
                        return WebEngine::from_version(version);
                    }
                }
            }
        }
        WebEngine::absent()
    }

    /// Present and at or above [`MIN_WEBKITGTK`].
    pub fn is_supported(&self) -> bool {
        self.version
            .as_deref()
            .is_some_and(|version| version_at_least(version, MIN_WEBKITGTK))
    }

    /// `supported`, `outdated` or `absent`, for the status line.
    pub fn verdict(&self) -> &'static str {
        match &self.version {
            None => "absent",
            Some(version) if version_at_least(version, MIN_WEBKITGTK) => "supported",
            Some(_) => "outdated",
        }
    }
}

/// Compare dotted versions numerically (`2.48.1-1` → `2.48.1`). Missing
/// segments count as zero, so `2.44` equals `2.44.0`.
pub fn version_at_least(version: &str, minimum: &str) -> bool {
    let parse = |text: &str| -> Vec<u64> {
        text.split(['.', '-', '+'])
            .take(3)
            .map(|part| {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().unwrap_or(0)
            })
            .collect()
    };
    let mut have = parse(version);
    let mut want = parse(minimum);
    have.resize(3, 0);
    want.resize(3, 0);
    have >= want
}

/// Hardening baseline plus the update channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityPolicy {
    pub channel: Channel,
    pub https_only: bool,
    pub block_mixed_content: bool,
    pub block_third_party_cookies: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        SecurityPolicy {
            channel: Channel::Stable,
            https_only: true,
            block_mixed_content: true,
            block_third_party_cookies: true,
        }
    }
}

fn default_root() -> PathBuf {
    let config = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
        });
    config.join("aether").join("browser")
}

impl SecurityPolicy {
    /// Read `security.toml` below `root`. A missing file yields the defaults;
    /// an unreadable or malformed file is an error the caller reports instead
    /// of silently running with a weaker policy.
    pub fn load(root: &Path) -> Result<SecurityPolicy, String> {
        Self::load_from(&root.join(SETTINGS_FILE))
    }

    pub fn load_from(path: &Path) -> Result<SecurityPolicy, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SecurityPolicy::default())
            }
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        let table: toml::Table = toml::from_str(&text)
            .map_err(|error| format!("{} is not valid TOML ({error})", path.display()))?;
        let browser = table.get("browser").and_then(toml::Value::as_table);
        let field = |key: &str| browser.and_then(|table| table.get(key));
        let mut policy = SecurityPolicy::default();
        if let Some(channel) = field("channel").and_then(toml::Value::as_str) {
            policy.channel = Channel::parse(channel);
        }
        if let Some(value) = field("https-only").and_then(toml::Value::as_bool) {
            policy.https_only = value;
        }
        if let Some(value) = field("mixed-content").and_then(toml::Value::as_str) {
            policy.block_mixed_content = !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "allow" | "off" | "false"
            );
        }
        if let Some(value) = field("third-party-cookies").and_then(toml::Value::as_str) {
            policy.block_third_party_cookies = !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "allow" | "off" | "false"
            );
        }
        Ok(policy)
    }

    /// The profile root the browser reads (`$XDG_CONFIG_HOME/aether/browser`).
    pub fn default_root() -> PathBuf {
        default_root()
    }

    /// Security problems that fail the check regardless of the host: a
    /// disabled baseline or an outdated engine.
    pub fn problems(&self, engine: &WebEngine) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.https_only {
            problems.push("HTTPS-only is disabled: cleartext pages would be allowed".to_string());
        }
        if !self.block_mixed_content {
            problems.push(
                "mixed content is allowed: HTTPS pages may load insecure subresources".to_string(),
            );
        }
        if !self.block_third_party_cookies {
            problems.push(
                "third-party cookies are allowed: cross-site tracking is not blocked".to_string(),
            );
        }
        if engine.verdict() == "outdated" {
            problems.push(format!(
                "WebKitGTK {} is older than the {MIN_WEBKITGTK} security floor — update the image ({} channel)",
                engine.version.as_deref().unwrap_or("?"),
                self.channel.as_str()
            ));
        }
        problems
    }

    /// Strict check (`--security-check`): the engine must be installed, which
    /// is exactly what the acceptance harness verifies on the image.
    pub fn strict_problems(&self, engine: &WebEngine) -> Vec<String> {
        let mut problems = self.problems(engine);
        if engine.version.is_none() {
            problems.push(format!(
                "WebKitGTK is not installed: only the engine adapter is present (set AETHER_WEBKITGTK_VERSION or install webkit2gtk)"
            ));
        }
        problems
    }

    /// Status lines, in the order the harness expects them.
    pub fn status_lines(&self, engine: &WebEngine) -> Vec<String> {
        vec![
            format!(
                "channel={} updates={}",
                self.channel.as_str(),
                UPDATE_SOURCE
            ),
            format!(
                "https-only={} mixed-content={} third-party-cookies={}",
                if self.https_only { "on" } else { "off" },
                if self.block_mixed_content {
                    "blocked"
                } else {
                    "allowed"
                },
                if self.block_third_party_cookies {
                    "blocked"
                } else {
                    "allowed"
                }
            ),
            format!(
                "engine={} version={} minimum={} verdict={}",
                engine.name,
                engine.version.as_deref().unwrap_or("absent"),
                MIN_WEBKITGTK,
                engine.verdict()
            ),
            "private-window=no-profile-writes".to_string(),
            "downloads=prompt".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aether-browser-security-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn channels_round_trip_and_unknown_falls_back_to_stable() {
        for channel in Channel::ALL {
            assert_eq!(Channel::parse(channel.as_str()), channel);
        }
        assert_eq!(Channel::parse("BETA"), Channel::Beta);
        assert_eq!(Channel::parse("nightly"), Channel::Stable);
    }

    #[test]
    fn version_comparison_handles_dotted_and_packaged_versions() {
        assert!(version_at_least("2.44", "2.44"));
        assert!(version_at_least("2.44.0", "2.44"));
        assert!(version_at_least("2.48.1-1", "2.44"));
        assert!(version_at_least("3.0", "2.44"));
        assert!(!version_at_least("2.43.9", "2.44"));
        assert!(!version_at_least("2.4", "2.44"));
        assert!(!version_at_least("1.99", "2.0"));
    }

    #[test]
    fn the_default_policy_passes_and_names_the_update_source() {
        let policy = SecurityPolicy::default();
        let engine = WebEngine::from_version("2.48.1");
        assert!(policy.problems(&engine).is_empty());
        assert!(policy.strict_problems(&engine).is_empty());
        let lines = policy.status_lines(&engine);
        assert_eq!(lines[0], "channel=stable updates=os-image");
        assert!(lines[1].contains("https-only=on"));
        assert!(lines[2].contains("version=2.48.1 minimum=2.44 verdict=supported"));
    }

    #[test]
    fn a_missing_settings_file_uses_the_defaults() {
        let root = temp_dir("missing");
        let policy = SecurityPolicy::load(&root).unwrap();
        assert_eq!(policy, SecurityPolicy::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn settings_round_trip_the_channel_and_the_baseline() {
        let root = temp_dir("file");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(SETTINGS_FILE),
            "[browser]\nchannel = \"beta\"\nhttps-only = true\nmixed-content = \"block\"\nthird-party-cookies = \"allow\"\n",
        )
        .unwrap();
        let policy = SecurityPolicy::load(&root).unwrap();
        assert_eq!(policy.channel, Channel::Beta);
        assert!(policy.https_only);
        assert!(policy.block_mixed_content);
        assert!(!policy.block_third_party_cookies);
        // Allowing third-party cookies is a failing baseline, not a preference.
        assert!(policy
            .problems(&WebEngine::from_version("2.48.1"))
            .iter()
            .any(|problem| problem.contains("third-party cookies")));

        std::fs::write(root.join(SETTINGS_FILE), "this is not toml = = =").unwrap();
        assert!(SecurityPolicy::load(&root).is_err(), "malformed files are reported");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_outdated_engine_is_a_failure_with_the_floor_named() {
        let policy = SecurityPolicy::default();
        let engine = WebEngine::from_version("2.42.3");
        assert_eq!(engine.verdict(), "outdated");
        let problems = policy.problems(&engine);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("2.42.3 is older than the 2.44 security floor"));
        assert!(problems[0].contains("update the image (stable channel)"));
    }

    #[test]
    fn the_strict_check_requires_the_engine() {
        let policy = SecurityPolicy::default();
        let absent = WebEngine::absent();
        assert!(policy.problems(&absent).is_empty(), "the dev host is not a failure");
        assert_eq!(absent.verdict(), "absent");
        let problems = policy.strict_problems(&absent);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("WebKitGTK is not installed"));
        assert!(policy.status_lines(&absent)[2]
            .contains("version=absent minimum=2.44 verdict=absent"));
    }

    #[test]
    fn disabling_https_only_fails_the_baseline() {
        let policy = SecurityPolicy {
            https_only: false,
            block_mixed_content: false,
            ..SecurityPolicy::default()
        };
        let problems = policy.problems(&WebEngine::from_version("2.48.1"));
        assert!(problems.iter().any(|p| p.contains("HTTPS-only is disabled")));
        assert!(problems.iter().any(|p| p.contains("mixed content")));
    }
}
