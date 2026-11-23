//! Lua hooks with a time limit and automatic disable (P7-T04, F4 section 5).
//!
//! Scripts live in `scripts/*.lua`; the event comes from the file name
//! (`on_login.lua`) or a `-- event: on_login` header. Each hook runs with a
//! default 200 ms limit. A failing or over-time hook is disabled, reported,
//! and never stops the shell. Execution uses the system `lua` interpreter
//! (no embedded runtime yet), so a missing interpreter is reported, not
//! silently ignored.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::HookError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    Login,
    FocusChange,
    WindowOpen,
    WindowClose,
    NetworkChange,
    PowerChange,
}

impl HookEvent {
    pub const ALL: [HookEvent; 6] = [
        Self::Login,
        Self::FocusChange,
        Self::WindowOpen,
        Self::WindowClose,
        Self::NetworkChange,
        Self::PowerChange,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "on_login",
            Self::FocusChange => "on_focus_change",
            Self::WindowOpen => "on_window_open",
            Self::WindowClose => "on_window_close",
            Self::NetworkChange => "on_network_change",
            Self::PowerChange => "on_power_change",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().trim_end_matches("()");
        Self::ALL.into_iter().find(|event| event.as_str() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookScript {
    pub name: String,
    pub path: PathBuf,
    pub event: HookEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookResult {
    pub script: String,
    pub event: HookEvent,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum HooksError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("state file error: {0}")]
    State(String),
}

/// Runs one script. Implementations decide how (external Lua, embedded VM).
pub trait HookRunner {
    fn run(&self, script: &Path, event: HookEvent, timeout: Duration) -> Result<(), HookError>;
}

/// Runs scripts with the system `lua` binary and a hard time limit.
#[derive(Debug, Clone)]
pub struct LuaCliRunner {
    binary: PathBuf,
}

impl LuaCliRunner {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn system() -> Self {
        Self::new("lua")
    }

    pub fn available(&self) -> bool {
        Command::new(&self.binary)
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

impl HookRunner for LuaCliRunner {
    fn run(&self, script: &Path, event: HookEvent, timeout: Duration) -> Result<(), HookError> {
        let mut child = Command::new(&self.binary)
            .arg(script)
            .arg(event.as_str())
            .env("AETHER_EVENT", event.as_str())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    HookError::Unavailable(format!(
                        "{} was not found; Lua hooks are disabled",
                        self.binary.display()
                    ))
                } else {
                    HookError::Io(error.to_string())
                }
            })?;

        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stderr = String::new();
                    if let Some(mut stream) = child.stderr.take() {
                        use std::io::Read;
                        let _ = stream.read_to_string(&mut stderr);
                    }
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(HookError::Failed {
                            code: status.code(),
                            stderr: stderr.trim().to_string(),
                        })
                    };
                }
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(HookError::Timeout {
                            millis: timeout.as_millis() as u64,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(HookError::Io(error.to_string())),
            }
        }
    }
}

/// Discovers and dispatches hooks, keeping disabled hooks across restarts.
#[derive(Debug)]
pub struct HookManager {
    scripts_dir: PathBuf,
    disabled_path: PathBuf,
    disabled: BTreeMap<String, String>,
    timeout: Duration,
}

impl HookManager {
    pub fn load(scripts_dir: impl Into<PathBuf>, disabled_path: impl Into<PathBuf>) -> Self {
        let disabled_path = disabled_path.into();
        let disabled = fs::read_to_string(&disabled_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            scripts_dir: scripts_dir.into(),
            disabled_path,
            disabled,
            timeout: Duration::from_millis(200),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn disabled(&self) -> &BTreeMap<String, String> {
        &self.disabled
    }

    pub fn scripts(&self) -> Vec<HookScript> {
        discover(&self.scripts_dir)
    }

    fn persist(&self) -> Result<(), HooksError> {
        if let Some(parent) = self.disabled_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.disabled)
            .map_err(|error| HooksError::State(error.to_string()))?;
        fs::write(&self.disabled_path, text)?;
        Ok(())
    }

    /// Run every enabled hook for one event. Failures are recorded and the
    /// script is disabled for future events.
    pub fn dispatch(&mut self, event: HookEvent, runner: &dyn HookRunner) -> Vec<HookResult> {
        let mut results = Vec::new();
        for script in self.scripts() {
            if script.event != event {
                continue;
            }
            if self.disabled.contains_key(&script.name) {
                continue;
            }
            match runner.run(&script.path, event, self.timeout) {
                Ok(()) => results.push(HookResult {
                    script: script.name,
                    event,
                    ok: true,
                    error: None,
                }),
                Err(error) => {
                    let message = error.to_string();
                    self.disabled.insert(script.name.clone(), message.clone());
                    results.push(HookResult {
                        script: script.name,
                        event,
                        ok: false,
                        error: Some(message),
                    });
                }
            }
        }
        let _ = self.persist();
        results
    }

    pub fn enable(&mut self, name: &str) -> Result<(), HooksError> {
        self.disabled.remove(name);
        self.persist()
    }

    pub fn disable(&mut self, name: &str, reason: &str) -> Result<(), HooksError> {
        self.disabled.insert(name.to_string(), reason.to_string());
        self.persist()
    }
}

/// Find `.lua` scripts and their event.
pub fn discover(dir: &Path) -> Vec<HookScript> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut scripts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("lua") {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let header = fs::read_to_string(&path)
            .ok()
            .and_then(|text| {
                text.lines().take(5).find_map(|line| {
                    line.trim()
                        .strip_prefix("-- event:")
                        .map(|value| value.trim().to_string())
                })
            });
        let event = HookEvent::parse(&stem)
            .or_else(|| header.as_deref().and_then(HookEvent::parse));
        let Some(event) = event else { continue };
        scripts.push(HookScript {
            name: stem,
            path,
            event,
        });
    }
    scripts.sort_by(|a, b| a.name.cmp(&b.name));
    scripts
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRunner {
        fail: Vec<&'static str>,
        timeout: Vec<&'static str>,
    }

    impl HookRunner for FakeRunner {
        fn run(&self, script: &Path, _event: HookEvent, timeout: Duration) -> Result<(), HookError> {
            let name = script.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let _ = timeout;
            if self.fail.contains(&name.as_str()) {
                return Err(HookError::Failed {
                    code: Some(1),
                    stderr: "boom".into(),
                });
            }
            if self.timeout.contains(&name.as_str()) {
                return Err(HookError::Timeout { millis: 200 });
            }
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-hooks-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn discovery_uses_names_and_headers() {
        let dir = temp_dir("discover");
        fs::write(dir.join("on_login.lua"), "-- nothing\n").unwrap();
        fs::write(dir.join("custom.lua"), "-- event: on_network_change\nprint('x')\n").unwrap();
        fs::write(dir.join("ignored.lua"), "print('x')\n").unwrap();
        let scripts = discover(&dir);
        assert_eq!(scripts.len(), 2);
        assert!(scripts
            .iter()
            .any(|script| script.name == "on_login" && script.event == HookEvent::Login));
        assert!(scripts
            .iter()
            .any(|script| script.name == "custom" && script.event == HookEvent::NetworkChange));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn failing_hook_is_disabled_and_persisted() {
        let dir = temp_dir("disable");
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("on_login.lua"), "print('x')\n").unwrap();
        let disabled_path = dir.join("disabled.json");
        let mut manager = HookManager::load(&scripts, &disabled_path);
        let runner = FakeRunner {
            fail: Vec::new(),
            timeout: vec!["on_login"],
        };
        let results = manager.dispatch(HookEvent::Login, &runner);
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(manager.disabled().contains_key("on_login"));

        let reloaded = HookManager::load(&scripts, &disabled_path);
        assert!(reloaded.disabled().contains_key("on_login"));

        // Second dispatch skips the disabled hook.
        let results = manager.dispatch(HookEvent::Login, &runner);
        assert!(results.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn only_matching_events_run() {
        let dir = temp_dir("events");
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("on_login.lua"), "").unwrap();
        fs::write(scripts.join("on_focus_change.lua"), "").unwrap();
        let mut manager = HookManager::load(&scripts, dir.join("disabled.json"));
        let runner = FakeRunner {
            fail: Vec::new(),
            timeout: Vec::new(),
        };
        let results = manager.dispatch(HookEvent::FocusChange, &runner);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].script, "on_focus_change");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn real_lua_hook_runs_and_times_out() {
        let runner = LuaCliRunner::system();
        if !runner.available() {
            return;
        }
        let dir = temp_dir("lua");
        let ok = dir.join("ok.lua");
        fs::write(&ok, "io.write('ok')\n").unwrap();
        runner
            .run(&ok, HookEvent::Login, Duration::from_secs(2))
            .expect("hook should succeed");

        let slow = dir.join("slow.lua");
        fs::write(&slow, "while true do end\n").unwrap();
        let started = Instant::now();
        let result = runner.run(&slow, HookEvent::Login, Duration::from_millis(150));
        assert!(matches!(result, Err(HookError::Timeout { .. })));
        assert!(started.elapsed() < Duration::from_secs(3));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_lua_is_reported() {
        let dir = temp_dir("nolua");
        let script = dir.join("x.lua");
        fs::write(&script, "").unwrap();
        let runner = LuaCliRunner::new("/definitely/not/lua");
        assert!(matches!(
            runner.run(&script, HookEvent::Login, Duration::from_secs(1)),
            Err(HookError::Unavailable(_))
        ));
        let _ = fs::remove_dir_all(dir);
    }
}
