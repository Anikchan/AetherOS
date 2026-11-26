//! Codec policy and media discovery for the Player (P4-T14, DA-AC-06).
//!
//! AetherOS ships GStreamer (`gst-plugins-base`/`good`) but **not** H.264/AAC
//! in the public base image (decision P0-T03): those stay in the optional codec
//! bundle. The player therefore inspects a file with `gst-discoverer-1.0`
//! before playing it:
//!
//! * duration from the discoverer replaces the playlist's estimate,
//! * a restricted codec is reported as "needs the optional codec bundle"
//!   instead of failing silently in the pipeline,
//! * a missing discoverer is not an error — playback proceeds and the engine
//!   reports any real failure.
//!
//! Parsing is pure Rust over the discoverer's text output, so the tests use a
//! fabricated `PATH` and never need GStreamer installed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::engine::{find_program, EngineCommand, EngineError};

/// Codecs deliberately left out of the public image (P0-T03).
pub const RESTRICTED_CODECS: [&str; 4] = ["H.264", "H.265/HEVC", "AAC", "MPEG-4 AAC"];

/// What discovery found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaInfo {
    pub duration: Option<Duration>,
    /// Caps lines the discoverer reported, e.g. `audio/x-vorbis` or
    /// `video/x-h264`.
    pub audio: Vec<String>,
    pub video: Vec<String>,
    /// True when the discoverer reported an error for the stream.
    pub analysis_failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecPolicy {
    /// Everything the file needs is in the base image.
    Allowed,
    /// A restricted codec was found; playback is refused with this reason.
    NeedsCodecBundle { codecs: Vec<String> },
}

impl MediaInfo {
    /// Which restricted codecs the file carries.
    pub fn restricted_codecs(&self) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for caps in self.audio.iter().chain(self.video.iter()) {
            if let Some(codec) = restricted_codec(caps) {
                if !found.contains(&codec) {
                    found.push(codec);
                }
            }
        }
        found
    }

    pub fn codec_policy(&self) -> CodecPolicy {
        let codecs = self.restricted_codecs();
        if codecs.is_empty() {
            CodecPolicy::Allowed
        } else {
            CodecPolicy::NeedsCodecBundle { codecs }
        }
    }

    /// Message the player shows for a blocked file.
    pub fn blocked_message(&self) -> Option<String> {
        match self.codec_policy() {
            CodecPolicy::Allowed => None,
            CodecPolicy::NeedsCodecBundle { codecs } => Some(format!(
                "{} playback needs the optional codec bundle (P0-T03)",
                codecs.join(", ")
            )),
        }
    }
}

/// Identify a restricted codec from one GStreamer caps string.
pub fn restricted_codec(caps: &str) -> Option<String> {
    let caps = caps.to_ascii_lowercase();
    if caps.contains("x-h264") || caps.contains("x-h.264") || caps.contains("h264") {
        return Some("H.264".to_string());
    }
    if caps.contains("x-h265") || caps.contains("x-h.265") || caps.contains("hevc") {
        return Some("H.265/HEVC".to_string());
    }
    // AAC: `audio/mpeg, mpegversion=(int)4` or an explicit AAC caps name.
    let is_mpeg_audio = caps.contains("audio/mpeg") || caps.contains("audio/aac");
    if is_mpeg_audio && (caps.contains("mpegversion=(int)4") || caps.contains("audio/aac")) {
        return Some("AAC".to_string());
    }
    None
}

/// Parse `gst-discoverer-1.0` output (plain or `-v`).
///
/// Duration line: `Duration: 0:03:12.345000000`; caps lines start with
/// `audio:` / `video:`. Whitespace and section headers are tolerated.
pub fn parse_discoverer(output: &str) -> MediaInfo {
    let mut info = MediaInfo::default();
    for line in output.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Duration:") {
            info.duration = parse_gst_duration(value.trim());
            continue;
        }
        if let Some(value) = line.strip_prefix("audio:") {
            let caps = value.trim().to_string();
            if !caps.is_empty() {
                info.audio.push(caps);
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("video:") {
            let caps = value.trim().to_string();
            if !caps.is_empty() {
                info.video.push(caps);
            }
            continue;
        }
        if line.contains("Could not") || line.starts_with("Error:") {
            info.analysis_failed = true;
        }
    }
    info
}

/// Parse GStreamer's `H:MM:SS.nnnnnnnnn` duration (hours may exceed 24).
pub fn parse_gst_duration(value: &str) -> Option<Duration> {
    // `gst-discoverer` omits the hour field for clips under an hour in some
    // versions (`00:03.500000000`); accept both forms.
    let negative = value.starts_with('-');
    let value = value.trim_start_matches('-');
    let parts: Vec<&str> = value.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [h, m, s] => (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?, *s),
        [m, s] => (0, m.parse::<u64>().ok()?, *s),
        _ => return None,
    };
    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole.parse::<u64>().ok()?, fraction),
        None => (seconds.parse::<u64>().ok()?, ""),
    };
    let nanos = {
        let digits: String = fraction.chars().take(9).collect();
        if digits.is_empty() {
            0
        } else {
            let value = digits.parse::<u64>().ok()?;
            value * 10u64.pow(9 - digits.len() as u32)
        }
    };
    let total = Duration::from_secs(hours * 3600 + minutes * 60 + whole) + Duration::from_nanos(nanos);
    if negative {
        None
    } else {
        Some(total)
    }
}

pub trait MediaProbe {
    fn name(&self) -> &'static str;
    fn probe(&self, uri: &str) -> Result<MediaInfo, EngineError>;
}

/// `gst-discoverer-1.0 -v <uri>` adapter.
pub struct GstDiscoverer {
    program: PathBuf,
}

impl GstDiscoverer {
    pub fn detect() -> Result<GstDiscoverer, EngineError> {
        GstDiscoverer::detect_in(&std::env::var("PATH").unwrap_or_default())
    }

    pub fn detect_in(path_var: &str) -> Result<GstDiscoverer, EngineError> {
        let program = find_program(path_var, "gst-discoverer-1.0").ok_or_else(|| {
            EngineError::NotAvailable("gst-discoverer-1.0 (GStreamer tools)".to_string())
        })?;
        Ok(GstDiscoverer { program })
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn probe_command(&self, uri: &str) -> EngineCommand {
        EngineCommand {
            program: self.program.clone(),
            args: vec!["-v".to_string(), uri.to_string()],
        }
    }
}

impl MediaProbe for GstDiscoverer {
    fn name(&self) -> &'static str {
        "gst-discoverer-1.0"
    }

    fn probe(&self, uri: &str) -> Result<MediaInfo, EngineError> {
        let command = self.probe_command(uri);
        let output = Command::new(&command.program)
            .args(&command.args)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    EngineError::NotAvailable(command.program.display().to_string())
                } else {
                    EngineError::Failed(format!("{}: {error}", command.command_line()))
                }
            })?;
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if !output.status.success() && text.trim().is_empty() {
            return Err(EngineError::Failed(format!(
                "{} exited with {}",
                command.command_line(),
                output.status
            )));
        }
        Ok(parse_discoverer(&text))
    }
}

/// Probe used when no discoverer is installed: playback proceeds and the
/// engine reports failures.
pub struct NoProbe;

impl MediaProbe for NoProbe {
    fn name(&self) -> &'static str {
        "none"
    }

    fn probe(&self, _uri: &str) -> Result<MediaInfo, EngineError> {
        Err(EngineError::NotAvailable(
            "gst-discoverer-1.0 (GStreamer tools)".to_string(),
        ))
    }
}

/// Scripted probe for tests and the headless renderer.
#[derive(Debug, Default)]
pub struct MockProbe {
    info: MediaInfo,
    available: bool,
    calls: Vec<String>,
}

impl MockProbe {
    pub fn new(info: MediaInfo) -> Self {
        MockProbe {
            info,
            available: true,
            calls: Vec::new(),
        }
    }

    pub fn unavailable() -> Self {
        MockProbe {
            info: MediaInfo::default(),
            available: false,
            calls: Vec::new(),
        }
    }

    pub fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl MediaProbe for MockProbe {
    fn name(&self) -> &'static str {
        "mock probe"
    }

    fn probe(&self, uri: &str) -> Result<MediaInfo, EngineError> {
        if !self.available {
            return Err(EngineError::NotAvailable("mock probe".to_string()));
        }
        let _ = uri;
        Ok(self.info.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_discoverer(tag: &str, output: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "aether-discoverer-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("gst-discoverer-1.0");
        let script = format!("#!/bin/sh\ncat <<'EOF'\n{output}EOF\n");
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
        base
    }

    #[test]
    fn durations_parse_with_and_without_hours() {
        assert_eq!(
            parse_gst_duration("0:03:12.345000000"),
            Some(Duration::from_secs(192) + Duration::from_millis(345))
        );
        assert_eq!(
            parse_gst_duration("1:02:03.5"),
            Some(Duration::from_secs(3723) + Duration::from_millis(500))
        );
        assert_eq!(
            parse_gst_duration("00:03.250000000"),
            Some(Duration::from_millis(3250))
        );
        assert_eq!(parse_gst_duration("0:00:00.000000000"), Some(Duration::ZERO));
        assert_eq!(parse_gst_duration("n/a"), None);
        assert_eq!(parse_gst_duration("-0:00:01.0"), None);
    }

    #[test]
    fn discoverer_output_is_parsed() {
        let info = parse_discoverer(
            "Analyzing file:///music/glass.flac\n\
             Done discovering file:///music/glass.flac\n\
             \n\
             Properties:\n\
               Duration: 0:03:52.000000000\n\
               Seekable: yes\n\
             audio: audio/x-vorbis, channels=(int)2, rate=(int)44100\n",
        );
        assert_eq!(info.duration, Some(Duration::from_secs(232)));
        assert_eq!(info.audio, vec!["audio/x-vorbis, channels=(int)2, rate=(int)44100"]);
        assert!(info.video.is_empty());
        assert_eq!(info.codec_policy(), CodecPolicy::Allowed);
        assert!(info.blocked_message().is_none());
        assert!(!info.analysis_failed);
    }

    #[test]
    fn restricted_codecs_are_identified() {
        assert_eq!(restricted_codec("video/x-h264, profile=(string)high"), Some("H.264".into()));
        assert_eq!(restricted_codec("video/x-h265"), Some("H.265/HEVC".into()));
        assert_eq!(
            restricted_codec("audio/mpeg, mpegversion=(int)4, framed=(boolean)true"),
            Some("AAC".into())
        );
        assert_eq!(
            restricted_codec("audio/mpeg, mpegversion=(int)1, layer=(int)3"),
            None,
            "MP3 is not restricted"
        );
        assert_eq!(restricted_codec("audio/x-opus"), None);

        let info = parse_discoverer(
            "Duration: 0:00:10.000000000\n\
             audio: audio/mpeg, mpegversion=(int)4\n\
             video: video/x-h264, width=(int)1920\n",
        );
        let codecs = info.restricted_codecs();
        assert!(codecs.contains(&"H.264".to_string()));
        assert!(codecs.contains(&"AAC".to_string()));
        let message = info.blocked_message().unwrap();
        assert!(message.contains("H.264"));
        assert!(message.contains("AAC"));
        assert!(message.contains("codec bundle"));
    }

    #[test]
    fn discoverer_detection_uses_path() {
        let empty = fake_discoverer("empty", "");
        // Remove the program: detection must fail without panicking.
        std::fs::remove_file(empty.join("gst-discoverer-1.0")).unwrap();
        assert!(GstDiscoverer::detect_in(&empty.display().to_string()).is_err());

        let bin = fake_discoverer(
            "full",
            "Duration: 0:00:05.000000000\naudio: audio/x-vorbis\n",
        );
        let discoverer = GstDiscoverer::detect_in(&format!("/nonexistent:{}", bin.display()))
            .unwrap();
        assert!(discoverer.program().ends_with("gst-discoverer-1.0"));
        let command = discoverer.probe_command("file:///a.flac");
        assert_eq!(command.args, vec!["-v", "file:///a.flac"]);
        assert_eq!(discoverer.name(), "gst-discoverer-1.0");

        let info = discoverer.probe("file:///a.flac").unwrap();
        assert_eq!(info.duration, Some(Duration::from_secs(5)));
        assert_eq!(info.codec_policy(), CodecPolicy::Allowed);

        let _ = std::fs::remove_dir_all(empty);
        let _ = std::fs::remove_dir_all(bin);
    }

    #[test]
    fn no_probe_reports_not_available() {
        let probe = NoProbe;
        let error = probe.probe("file:///a.flac").unwrap_err();
        assert!(error.is_not_available());
        assert_eq!(probe.name(), "none");

        let mock = MockProbe::unavailable();
        assert!(mock.probe("file:///a.flac").is_err());
    }
}
