//! Audio engine adapters for the Player app (P4-T14, DA-AC-06).
//!
//! # Codec policy
//!
//! AetherOS ships the **GStreamer** stack (`gstreamer`, `gst-plugins-base`,
//! `gst-plugins-good`) as the audio and video backend, because it is packaged
//! by every supported distribution, has the widest format coverage and keeps
//! patent-encumbered codecs optional. **H.264/AAC are deliberately kept out of
//! the public base image** (decision P0-T03): they are not installed by default
//! and are only reachable through the optional codec bundle (or the distro's
//! own package), so the image can be distributed without a patent licence.
//!
//! Consequence for this prototype: the player plays exactly what the system
//! provides. FLAC, Vorbis, Opus, MP3 (where the distro enables it) and WAV work
//! through `gst-play-1.0`; a file whose codec is missing fails in the engine and
//! the UI reports it instead of pretending to play.
//!
//! # Adapters
//!
//! * [`GStreamerEngine`] shells out to `gst-play-1.0` when it is on `PATH` and
//!   returns [`EngineError::NotAvailable`] otherwise (never panics).
//! * [`MockEngine`] records calls for the tests and for the headless renderer.
//!
//! `gst-play-1.0` is a command-line player without a control socket, so the
//! adapter can start and stop playback but cannot pause or seek inside a
//! running pipeline: those calls are recorded and the transport state in
//! [`crate::transport`] stays the source of truth. A `playbin` binding replaces
//! the shell-out when the audio service lands.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The backend binary is not installed.
    NotAvailable(String),
    /// The backend exists but the call failed.
    Failed(String),
}

impl EngineError {
    pub fn is_not_available(&self) -> bool {
        matches!(self, EngineError::NotAvailable(_))
    }

    pub fn message(&self) -> String {
        match self {
            EngineError::NotAvailable(what) => format!("{what} is not available"),
            EngineError::Failed(message) => format!("playback failed: {message}"),
        }
    }
}

/// A command the adapter would run (built separately so it is testable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl EngineCommand {
    pub fn command_line(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

pub trait Engine {
    /// Human name shown in the UI ("GStreamer" or "Mock").
    fn name(&self) -> &'static str;

    /// Start playing `uri`.
    fn play(&mut self, uri: &str) -> Result<(), EngineError>;

    fn pause(&mut self) -> Result<(), EngineError>;

    fn stop(&mut self) -> Result<(), EngineError>;

    fn seek(&mut self, position: Duration) -> Result<(), EngineError>;

    /// Last position the engine was told about.
    fn position(&self) -> Duration;
}

/// `gst-play-1.0` adapter.
#[derive(Debug)]
pub struct GStreamerEngine {
    program: PathBuf,
    child: Option<Child>,
    position: Duration,
    /// URI currently loaded into the running (or last) pipeline, so a seek or
    /// pause can relaunch `gst-play-1.0` at a new position.
    current_uri: Option<String>,
}

impl GStreamerEngine {
    pub fn detect() -> Result<GStreamerEngine, EngineError> {
        let path = std::env::var("PATH").unwrap_or_default();
        GStreamerEngine::detect_in(&path)
    }

    /// Testable detection against an explicit `PATH` value.
    pub fn detect_in(path_var: &str) -> Result<GStreamerEngine, EngineError> {
        let program = find_program(path_var, "gst-play-1.0").ok_or_else(|| {
            EngineError::NotAvailable("gst-play-1.0 (GStreamer plugins)".to_string())
        })?;
        Ok(GStreamerEngine {
            program,
            child: None,
            position: Duration::ZERO,
            current_uri: None,
        })
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The command used to play a URI: `gst-play-1.0 <uri>`, with
    /// `--start-position` when resuming or seeking inside a track.
    pub fn play_command(&self, uri: &str) -> EngineCommand {
        let mut args = Vec::new();
        if !self.position.is_zero() {
            args.push(format!("--start-position={}", self.position.as_secs_f64()));
        }
        args.push(uri.to_string());
        EngineCommand {
            program: self.program.clone(),
            args,
        }
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Engine for GStreamerEngine {
    fn name(&self) -> &'static str {
        "GStreamer"
    }

    fn play(&mut self, uri: &str) -> Result<(), EngineError> {
        self.stop_child();
        // The caller (PlayerController) records the desired start position
        // through `seek` before calling play, so `play_command` can pass it to
        // `gst-play-1.0 --start-position`.
        let command = self.play_command(uri);
        let child = Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    EngineError::NotAvailable(command.program.display().to_string())
                } else {
                    EngineError::Failed(format!("{}: {error}", command.command_line()))
                }
            })?;
        self.child = Some(child);
        self.current_uri = Some(uri.to_string());
        Ok(())
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        // gst-play-1.0 has no control socket; the honest pause stops the
        // process. The position recorded by the last `seek` (the transport's
        // position, recorded by PlayerController) is kept so playback resumes
        // there.
        self.stop_child();
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.stop_child();
        self.position = Duration::ZERO;
        self.current_uri = None;
        Ok(())
    }

    fn seek(&mut self, position: Duration) -> Result<(), EngineError> {
        self.position = position;
        // While playing, relaunch at the new position so the seek is real
        // instead of being recorded and ignored.
        if let Some(uri) = self.current_uri.clone() {
            if self.child.is_some() {
                self.stop_child();
                let command = self.play_command(&uri);
                match Command::new(&command.program)
                    .args(&command.args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => {
                        self.child = Some(child);
                        self.current_uri = Some(uri);
                    }
                    Err(error) => {
                        return Err(if error.kind() == std::io::ErrorKind::NotFound {
                            EngineError::NotAvailable(command.program.display().to_string())
                        } else {
                            EngineError::Failed(format!("{}: {error}", command.command_line()))
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn position(&self) -> Duration {
        self.position
    }
}

impl Drop for GStreamerEngine {
    fn drop(&mut self) {
        self.stop_child();
    }
}

/// Engine used by tests and the headless renderer: records every call.
#[derive(Debug)]
pub struct MockEngine {
    calls: Vec<String>,
    position: Duration,
    available: bool,
}

impl Default for MockEngine {
    fn default() -> Self {
        MockEngine {
            calls: Vec::new(),
            position: Duration::ZERO,
            available: true,
        }
    }
}

impl MockEngine {
    pub fn new() -> MockEngine {
        MockEngine::default()
    }

    /// A mock that reports the backend as missing.
    pub fn unavailable() -> MockEngine {
        MockEngine {
            available: false,
            ..MockEngine::default()
        }
    }

    pub fn calls(&self) -> &[String] {
        &self.calls
    }

    fn record(&mut self, call: String) -> Result<(), EngineError> {
        if !self.available {
            return Err(EngineError::NotAvailable("mock engine".to_string()));
        }
        self.calls.push(call);
        Ok(())
    }
}

impl Engine for MockEngine {
    fn name(&self) -> &'static str {
        "Mock"
    }

    fn play(&mut self, uri: &str) -> Result<(), EngineError> {
        self.record(format!("play {uri}"))
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.record("pause".to_string())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.record("stop".to_string())
    }

    fn seek(&mut self, position: Duration) -> Result<(), EngineError> {
        self.position = position;
        self.record(format!("seek {}", position.as_secs()))
    }

    fn position(&self) -> Duration {
        self.position
    }
}

/// Find an executable file called `name` in a `PATH` value.
pub fn find_program(path_var: &str, name: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|directory| !directory.is_empty())
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_bin(tag: &str, programs: &[&str]) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "aether-gst-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        for program in programs {
            let path = base.join(program);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&path).unwrap().permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&path, permissions).unwrap();
            }
        }
        base
    }

    #[test]
    fn availability_detection_uses_path_without_panicking() {
        let empty = fake_bin("empty", &[]);
        let error = GStreamerEngine::detect_in(&empty.display().to_string()).unwrap_err();
        assert!(error.is_not_available());
        assert!(error.message().contains("gst-play-1.0"));
        assert!(GStreamerEngine::detect_in("").is_err());

        let bin = fake_bin("full", &["gst-play-1.0"]);
        let engine =
            GStreamerEngine::detect_in(&format!("/nonexistent:{}", bin.display())).unwrap();
        assert!(engine.program().ends_with("gst-play-1.0"));
        assert_eq!(engine.name(), "GStreamer");

        // A non-executable file with the right name is not a backend.
        let plain = fake_bin("plain", &[]);
        std::fs::write(plain.join("gst-play-1.0"), "not a program").unwrap();
        assert!(GStreamerEngine::detect_in(&plain.display().to_string()).is_err());

        let _ = std::fs::remove_dir_all(empty);
        let _ = std::fs::remove_dir_all(bin);
        let _ = std::fs::remove_dir_all(plain);
    }

    #[test]
    fn play_command_contains_the_uri() {
        let bin = fake_bin("command", &["gst-play-1.0"]);
        let engine = GStreamerEngine::detect_in(&bin.display().to_string()).unwrap();
        let command = engine.play_command("file:///home/aether/Music/glass.flac");
        assert!(command.program.ends_with("gst-play-1.0"));
        assert_eq!(command.args, vec!["file:///home/aether/Music/glass.flac"]);
        assert_eq!(
            command.command_line(),
            format!(
                "{} file:///home/aether/Music/glass.flac",
                command.program.display()
            )
        );

        let _ = std::fs::remove_dir_all(bin);
    }

    #[test]
    fn mock_engine_records_the_transport_calls() {
        let mut engine = MockEngine::new();
        engine.play("file:///a.flac").unwrap();
        engine.pause().unwrap();
        engine.seek(Duration::from_secs(42)).unwrap();
        engine.stop().unwrap();
        assert_eq!(
            engine.calls(),
            [
                "play file:///a.flac",
                "pause",
                "seek 42",
                "stop"
            ]
        );
        assert_eq!(engine.position(), Duration::from_secs(42));
        assert_eq!(engine.name(), "Mock");

        // A missing backend reports NotAvailable instead of panicking.
        let mut missing = MockEngine::unavailable();
        let error = missing.play("file:///a.flac").unwrap_err();
        assert!(error.is_not_available());
        assert!(missing.calls().is_empty());
    }
}
