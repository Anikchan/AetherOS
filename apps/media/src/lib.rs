//! Shared media policy for AetherOS (P4-T14 Player, P4-T18a Browser).
//!
//! One decision table, two consumers. Decision P0-T03 keeps H.264, H.265/HEVC
//! and AAC **out of the public base image**: they live in the optional codec
//! bundle because of patent licensing. Owner answer Q24 then asks the browser
//! for "HD or reduced playback" rather than a hard failure:
//!
//! * the **Player** refuses a restricted file with a clear message telling the
//!   user to install the optional bundle ([`blocked_message`]),
//! * the **Browser** picks the best playable rung from the site's rendition
//!   ladder, uses VA-API when it is available, and labels the result "reduced"
//!   when a higher rung needs the bundle ([`plan_playback`]).
//!
//! Everything here is pure Rust over text and numbers, so both apps test the
//! same policy without GStreamer, a GPU or a network.

use std::path::{Path, PathBuf};

/// Codecs deliberately left out of the public image (P0-T03).
pub const RESTRICTED_CODECS: [&str; 4] = ["H.264", "H.265/HEVC", "AAC", "MPEG-4 AAC"];

/// Video codecs shipped in the base image.
pub const BASE_VIDEO_CODECS: [&str; 4] = ["VP8", "VP9", "AV1", "Theora"];

/// Audio codecs shipped in the base image.
pub const BASE_AUDIO_CODECS: [&str; 6] = ["Opus", "Vorbis", "FLAC", "MP3", "WAV", "PCM"];

/// Identify a restricted codec from one GStreamer caps / MIME / manifest string.
pub fn restricted_codec(caps: &str) -> Option<String> {
    let caps = caps.to_ascii_lowercase();
    if caps.contains("x-h264") || caps.contains("x-h.264") || caps.contains("h264") {
        return Some("H.264".to_string());
    }
    // RFC 6381 manifest strings used by HLS/DASH and Media Source Extensions
    // (`avc1.640028`, `hvc1.1.6.L93`, `mp4a.40.2`).
    if caps.contains("avc1") || caps.contains("avc3") {
        return Some("H.264".to_string());
    }
    if caps.contains("x-h265") || caps.contains("x-h.265") || caps.contains("hevc") {
        return Some("H.265/HEVC".to_string());
    }
    if caps.contains("hvc1") || caps.contains("hev1") {
        return Some("H.265/HEVC".to_string());
    }
    // AAC: `audio/mpeg, mpegversion=(int)4`, an explicit AAC caps name, or the
    // MPEG-4 audio object types 40 (AAC-LC) / 67 (AAC, MPEG-2).
    let is_mpeg_audio = caps.contains("audio/mpeg") || caps.contains("audio/aac");
    if is_mpeg_audio && (caps.contains("mpegversion=(int)4") || caps.contains("audio/aac")) {
        return Some("AAC".to_string());
    }
    if caps.contains("mp4a.40") || caps.contains("mp4a.67") {
        return Some("AAC".to_string());
    }
    None
}

/// Which restricted codecs a media description carries (deduplicated).
pub fn restricted_codecs<'a, I>(caps: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a String>,
{
    let mut found: Vec<String> = Vec::new();
    for caps in caps {
        if let Some(codec) = restricted_codec(caps) {
            if !found.contains(&codec) {
                found.push(codec);
            }
        }
    }
    found
}

/// Policy verdict for a media description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecPolicy {
    /// Everything the file needs is in the base image.
    Allowed,
    /// A restricted codec was found; playback needs the optional bundle.
    NeedsCodecBundle { codecs: Vec<String> },
}

/// Verdict for an already-identified set of restricted codecs.
pub fn codec_policy(codecs: &[String]) -> CodecPolicy {
    if codecs.is_empty() {
        CodecPolicy::Allowed
    } else {
        CodecPolicy::NeedsCodecBundle {
            codecs: codecs.to_vec(),
        }
    }
}

/// Message the Player shows for a file the base image cannot decode.
pub fn blocked_message(codecs: &[String]) -> Option<String> {
    match codec_policy(codecs) {
        CodecPolicy::Allowed => None,
        CodecPolicy::NeedsCodecBundle { codecs } => Some(format!(
            "{} playback needs the optional codec bundle (P0-T03)",
            codecs.join(", ")
        )),
    }
}

/// True when the codec is in the royalty-free base set.
pub fn is_base_codec(codec: &str) -> bool {
    let codec = codec.to_ascii_lowercase();
    if restricted_codec(&codec).is_some() {
        return false;
    }
    const FRAGMENTS: [&str; 13] = [
        "vp8", "vp9", "vp09", "av1", "av01", "theora", "opus", "vorbis", "flac", "x-wav", "pcm",
        "mpegversion=(int)1", "mp4a.6b",
    ];
    FRAGMENTS.iter().any(|fragment| codec.contains(fragment))
}

/// Friendly label for a codec string (`video/x-vp9` → `VP9`).
pub fn codec_label(codec: &str) -> String {
    if let Some(restricted) = restricted_codec(codec) {
        return restricted;
    }
    let codec = codec.to_ascii_lowercase();
    let matches = |needle: &str| codec.contains(needle);
    if matches("vp09") || matches("vp9") {
        "VP9".to_string()
    } else if matches("vp8") {
        "VP8".to_string()
    } else if matches("av01") || matches("av1") {
        "AV1".to_string()
    } else if matches("theora") {
        "Theora".to_string()
    } else if matches("opus") {
        "Opus".to_string()
    } else if matches("vorbis") {
        "Vorbis".to_string()
    } else if matches("flac") {
        "FLAC".to_string()
    } else if matches("mpegversion=(int)1") || matches("mp4a.6b") {
        "MP3".to_string()
    } else if matches("x-wav") || matches("pcm") {
        "PCM".to_string()
    } else {
        codec.trim().to_string()
    }
}

// ------------------------------------------------------------- acceleration ---

/// Hardware video decode backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccelBackend {
    /// VA-API on Intel/AMD GPUs (the reference laptop).
    Vaapi,
    None,
}

impl AccelBackend {
    pub fn label(self) -> &'static str {
        match self {
            AccelBackend::Vaapi => "vaapi",
            AccelBackend::None => "none",
        }
    }
}

/// Whether media playback can use hardware decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Acceleration {
    pub video_decode: bool,
    pub backend: AccelBackend,
}

impl Acceleration {
    pub fn software() -> Acceleration {
        Acceleration {
            video_decode: false,
            backend: AccelBackend::None,
        }
    }

    pub fn hardware() -> Acceleration {
        Acceleration {
            video_decode: true,
            backend: AccelBackend::Vaapi,
        }
    }

    /// Detect on this machine: `AETHER_MEDIA_HW_DECODE` overrides, otherwise a
    /// `vainfo` binary or `LIBVA_DRIVER_NAME` means VA-API is present.
    pub fn detect() -> Acceleration {
        Acceleration::from_environment(
            |key| std::env::var(key).ok(),
            find_program("vainfo").is_some(),
        )
    }

    /// Testable detection: the environment is injected.
    ///
    /// `on` forces hardware decode (used by the acceptance harness to prove the
    /// accelerated path is wired), `off` forces software, `auto`/unset uses the
    /// VA-API probe.
    pub fn from_environment(
        env: impl Fn(&str) -> Option<String>,
        vaapi_available: bool,
    ) -> Acceleration {
        let forced = env("AETHER_MEDIA_HW_DECODE")
            .map(|value| value.trim().to_ascii_lowercase());
        match forced.as_deref() {
            Some("on") | Some("1") | Some("true") | Some("force") => Acceleration::hardware(),
            Some("off") | Some("0") | Some("false") | Some("disable") => Acceleration::software(),
            _ => {
                let driver = env("LIBVA_DRIVER_NAME").unwrap_or_default();
                if vaapi_available || !driver.trim().is_empty() {
                    Acceleration::hardware()
                } else {
                    Acceleration::software()
                }
            }
        }
    }
}

/// First matching executable on `PATH` (no path separators in `name`).
pub fn find_program(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    find_program_in(&path_var, name)
}

/// [`find_program`] against an explicit `PATH` value (tests).
pub fn find_program_in(path_var: &str, name: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| candidate.is_file())
}

// -------------------------------------------------------------- capabilities ---

/// What this session can play, and how (Q24 + P0-T03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaCapabilities {
    pub acceleration: Acceleration,
    /// The optional restricted-codec bundle is installed.
    pub codec_bundle: bool,
    /// The owner enabled protected content (DRM/EME); off by default.
    pub drm: bool,
}

impl MediaCapabilities {
    /// Base image on a GPU-less machine: software decode, no bundle, no DRM.
    pub fn base() -> MediaCapabilities {
        MediaCapabilities {
            acceleration: Acceleration::software(),
            codec_bundle: false,
            drm: false,
        }
    }

    /// Detect this session from the environment and the codec marker file.
    pub fn detect() -> MediaCapabilities {
        MediaCapabilities::from_environment(
            |key| std::env::var(key).ok(),
            Acceleration::detect(),
            bundle_marker().is_file(),
        )
    }

    /// Testable detection: environment, acceleration and marker injected.
    pub fn from_environment(
        env: impl Fn(&str) -> Option<String>,
        acceleration: Acceleration,
        bundle_marker: bool,
    ) -> MediaCapabilities {
        MediaCapabilities {
            acceleration,
            codec_bundle: bundle_marker || truthy(env("AETHER_CODEC_BUNDLE").as_deref()),
            drm: truthy(env("AETHER_DRM").as_deref()),
        }
    }

    /// Restricted codecs play only when the optional bundle is installed;
    /// everything else is left to the engine.
    pub fn can_decode(&self, codec: &str) -> bool {
        restricted_codec(codec).is_none() || self.codec_bundle
    }

    /// One line per capability, for `aether-browser --media-status`.
    pub fn status_lines(&self) -> Vec<String> {
        vec![
            format!(
                "acceleration={} backend={}",
                if self.acceleration.video_decode {
                    "hardware"
                } else {
                    "software"
                },
                self.acceleration.backend.label()
            ),
            format!(
                "codec-bundle={}",
                if self.codec_bundle { "present" } else { "absent" }
            ),
            format!("drm={}", if self.drm { "enabled" } else { "disabled" }),
            format!("restricted={}", RESTRICTED_CODECS.join(",")),
            format!("video-base={}", BASE_VIDEO_CODECS.join(",")),
            format!("audio-base={}", BASE_AUDIO_CODECS.join(",")),
        ]
    }
}

/// `$XDG_CONFIG_HOME/aether/codecs/restricted.toml` (or `~/.config/...`), the
/// marker the codec-bundle installer writes.
pub fn bundle_marker() -> PathBuf {
    config_root().join("codecs").join("restricted.toml")
}

fn config_root() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("aether");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Path::new(&home).join(".config").join("aether")
}

fn truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(|value| value.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("on") | Some("true") | Some("yes") | Some("enabled")
    )
}

// ------------------------------------------------------------ playback plan ---

/// One rung of a stream's rendition ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendition {
    /// Codec as the manifest names it (`h264`, `vp09`, `audio/mp4; codecs=mp4a.40.2`).
    pub codec: String,
    pub height: u32,
    pub bandwidth_kbps: u32,
}

impl Rendition {
    pub fn new(codec: impl Into<String>, height: u32, bandwidth_kbps: u32) -> Rendition {
        Rendition {
            codec: codec.into(),
            height,
            bandwidth_kbps,
        }
    }
}

/// Decoder path chosen for a playable rendition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodePath {
    Hardware,
    Software,
}

impl DecodePath {
    pub fn label(self) -> &'static str {
        match self {
            DecodePath::Hardware => "hardware",
            DecodePath::Software => "software",
        }
    }
}

/// Why a stream cannot play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackBlock {
    /// Protected content needs the owner's opt-in.
    DrmNotEnabled,
    /// Every rung needs a codec out of the base image.
    NeedsCodecBundle { codecs: Vec<String> },
}

/// Outcome of [`plan_playback`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackPlan {
    /// The rung the browser should play, when one is playable.
    pub rendition: Option<Rendition>,
    pub decode: DecodePath,
    /// True when a higher rung is only available with the codec bundle.
    pub reduced: bool,
    pub block: Option<PlaybackBlock>,
    /// One line for the status bar / the acceptance harness.
    pub message: String,
}

impl PlaybackPlan {
    pub fn is_blocked(&self) -> bool {
        self.block.is_some()
    }
}

/// Choose the best playable rung of a stream (Q24: HD or reduced quality).
///
/// Rules, in order:
/// 1. DRM content is refused unless the owner enabled protected content.
/// 2. Restricted codecs are playable only with the optional bundle.
/// 3. The highest playable rung wins (resolution first, then bandwidth).
/// 4. The plan is "reduced" when a higher rung exists but needs the bundle.
/// 5. Hardware decode is used whenever VA-API is available.
pub fn plan_playback(
    renditions: &[Rendition],
    drm: bool,
    caps: &MediaCapabilities,
) -> PlaybackPlan {
    let decode = if caps.acceleration.video_decode {
        DecodePath::Hardware
    } else {
        DecodePath::Software
    };
    if drm && !caps.drm {
        return PlaybackPlan {
            rendition: None,
            decode,
            reduced: false,
            block: Some(PlaybackBlock::DrmNotEnabled),
            message:
                "protected content is off by default (Settings → Browser → Protected content)"
                    .to_string(),
        };
    }
    let best_offered = renditions
        .iter()
        .max_by_key(|rendition| (rendition.height, rendition.bandwidth_kbps));
    let chosen = renditions
        .iter()
        .filter(|rendition| caps.can_decode(&rendition.codec))
        .max_by_key(|rendition| (rendition.height, rendition.bandwidth_kbps));

    let Some(chosen) = chosen else {
        let mut codecs: Vec<String> = Vec::new();
        for rendition in renditions {
            let label = codec_label(&rendition.codec);
            if !codecs.contains(&label) {
                codecs.push(label);
            }
        }
        let message = blocked_message(&codecs).unwrap_or_else(|| "no playable rendition".into());
        return PlaybackPlan {
            rendition: None,
            decode,
            reduced: false,
            block: Some(PlaybackBlock::NeedsCodecBundle { codecs }),
            message,
        };
    };

    let reduced = best_offered
        .map(|best| chosen.height < best.height)
        .unwrap_or(false);
    let quality = if chosen.height > 0 {
        format!("{}p", chosen.height)
    } else {
        "audio".to_string()
    };
    let mut message = format!(
        "playing {} {} — {} decode",
        codec_label(&chosen.codec),
        quality,
        decode.label()
    );
    if reduced {
        message.push_str("; reduced quality (the codec bundle adds the HD rung)");
    }
    PlaybackPlan {
        rendition: Some(chosen.clone()),
        decode,
        reduced,
        block: None,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn restricted_codecs_match_the_player_policy() {
        assert_eq!(
            restricted_codec("video/x-h264, profile=(string)high"),
            Some("H.264".into())
        );
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

        let caps = vec![
            "video/x-h264".to_string(),
            "audio/mpeg, mpegversion=(int)4".to_string(),
            "video/x-h264, width=(int)1280".to_string(),
        ];
        assert_eq!(
            restricted_codecs(caps.iter()),
            vec!["H.264".to_string(), "AAC".to_string()],
            "deduplicated, in discovery order"
        );
        assert_eq!(codec_policy(&[]), CodecPolicy::Allowed);
        assert!(blocked_message(&[]).is_none());
        let blocked = blocked_message(&["H.264".to_string(), "AAC".to_string()]).unwrap();
        assert!(blocked.contains("H.264") && blocked.contains("AAC"));
        assert!(blocked.contains("codec bundle"));
        // RFC 6381 manifest strings (HLS/DASH/MSE) use the same policy.
        assert_eq!(restricted_codec("avc1.640028"), Some("H.264".into()));
        assert_eq!(restricted_codec("hvc1.1.6.L93"), Some("H.265/HEVC".into()));
        assert_eq!(restricted_codec("mp4a.40.2"), Some("AAC".into()));
        assert_eq!(restricted_codec("mp4a.6b"), None, "MP3 in MP4 is not restricted");
    }

    #[test]
    fn base_codec_classification_and_labels() {
        for base in ["vp09.00.10.08", "video/x-vp9", "av01.0.04M", "theora", "opus"] {
            assert!(is_base_codec(base), "{base} is royalty-free");
        }
        for restricted in ["video/x-h264", "hvc1.1.6.L93", "audio/mp4; codecs=mp4a.40.2"] {
            assert!(!is_base_codec(restricted), "{restricted} is restricted");
        }
        assert_eq!(codec_label("video/x-vp9"), "VP9");
        assert_eq!(codec_label("av01.0.04M.08"), "AV1");
        assert_eq!(codec_label("audio/mp4; codecs=mp4a.40.2"), "AAC");
        assert_eq!(codec_label("h264"), "H.264");
        assert_eq!(codec_label("audio/x-opus"), "Opus");
    }

    #[test]
    fn acceleration_honours_the_override_then_the_probe() {
        let _ = Acceleration::detect(); // never panics without a GPU
        assert_eq!(
            Acceleration::from_environment(env_from(&[("AETHER_MEDIA_HW_DECODE", "on")]), false),
            Acceleration::hardware()
        );
        assert_eq!(
            Acceleration::from_environment(env_from(&[("AETHER_MEDIA_HW_DECODE", "off")]), true),
            Acceleration::software()
        );
        assert_eq!(
            Acceleration::from_environment(env_from(&[]), true),
            Acceleration::hardware(),
            "a VA-API probe means hardware decode"
        );
        assert_eq!(
            Acceleration::from_environment(env_from(&[]), false),
            Acceleration::software()
        );
        assert_eq!(
            Acceleration::from_environment(
                env_from(&[("LIBVA_DRIVER_NAME", "iHD")]),
                false
            ),
            Acceleration::hardware(),
            "an explicit driver also enables hardware decode"
        );
        assert_eq!(AccelBackend::Vaapi.label(), "vaapi");
        assert_eq!(AccelBackend::None.label(), "none");
    }

    #[test]
    fn capabilities_gate_the_restricted_codecs() {
        let base = MediaCapabilities::base();
        assert!(!base.codec_bundle);
        assert!(!base.drm);
        assert!(base.can_decode("video/x-vp9"));
        assert!(!base.can_decode("video/x-h264"));
        assert!(!base.can_decode("audio/mpeg, mpegversion=(int)4"));

        let bundle = MediaCapabilities::from_environment(
            env_from(&[("AETHER_CODEC_BUNDLE", "1"), ("AETHER_DRM", "on")]),
            Acceleration::hardware(),
            false,
        );
        assert!(bundle.codec_bundle);
        assert!(bundle.drm);
        assert!(bundle.can_decode("video/x-h264"));

        let marker = MediaCapabilities::from_environment(
            env_from(&[]),
            Acceleration::software(),
            true,
        );
        assert!(marker.codec_bundle, "the installer marker enables the bundle");
        assert!(!marker.drm, "DRM stays off unless the owner opts in");

        let lines = base.status_lines();
        assert!(lines[0].starts_with("acceleration=software"));
        assert!(lines.iter().any(|line| line == "codec-bundle=absent"));
        assert!(lines.iter().any(|line| line == "drm=disabled"));
        assert!(lines.iter().any(|line| line.contains("H.264")));
        assert!(bundle_marker().ends_with("codecs/restricted.toml"));
    }

    fn ladder() -> Vec<Rendition> {
        vec![
            Rendition::new("audio/mp4; codecs=mp4a.40.2", 0, 128),
            Rendition::new("avc1.640028", 1080, 5_000),
            Rendition::new("vp09.00.10.08", 720, 2_500),
            Rendition::new("vp09.00.10.08", 1080, 4_000),
        ]
    }

    #[test]
    fn streaming_ladder_prefers_the_best_playable_rung() {
        let caps = MediaCapabilities {
            acceleration: Acceleration::hardware(),
            codec_bundle: false,
            drm: false,
        };
        let plan = plan_playback(&ladder(), false, &caps);
        let chosen = plan
            .rendition
            .as_ref()
            .expect("a royalty-free rung is playable");
        assert_eq!(chosen.height, 1080);
        assert_eq!(codec_label(&chosen.codec), "VP9");
        assert_eq!(plan.decode, DecodePath::Hardware);
        assert!(!plan.reduced, "the best rung was already playable");
        assert!(!plan.is_blocked());
        assert!(plan.message.contains("VP9 1080p"));
        assert!(plan.message.contains("hardware decode"));
    }

    #[test]
    fn reduced_quality_is_labelled_when_only_the_bundle_has_hd() {
        // Software-only GPU, no bundle: the 1080p H.264 rung is out (restricted
        // codec) and the best playable rung is VP9 720p, so the plan is the
        // reduced-quality fallback from Q24.
        let caps = MediaCapabilities::base();
        let stream = vec![
            Rendition::new("avc1.640028", 1080, 5_000),
            Rendition::new("vp09.00.10.08", 720, 2_500),
        ];
        let plan = plan_playback(&stream, false, &caps);
        assert_eq!(plan.rendition.unwrap().height, 720);
        assert!(plan.reduced);
        assert_eq!(plan.decode, DecodePath::Software);
        assert!(plan.message.contains("reduced quality"));
    }

    #[test]
    fn restricted_only_streams_need_the_bundle() {
        let caps = MediaCapabilities::base();
        let stream = vec![
            Rendition::new("avc1.640028", 1080, 5_000),
            Rendition::new("audio/mp4; codecs=mp4a.40.2", 0, 128),
        ];
        let plan = plan_playback(&stream, false, &caps);
        assert!(plan.is_blocked());
        assert!(plan.rendition.is_none());
        assert_eq!(
            plan.block,
            Some(PlaybackBlock::NeedsCodecBundle {
                codecs: vec!["H.264".to_string(), "AAC".to_string()]
            })
        );
        assert!(plan.message.contains("codec bundle"));

        // With the bundle the same stream plays at full quality.
        let bundled = MediaCapabilities {
            acceleration: Acceleration::hardware(),
            codec_bundle: true,
            drm: false,
        };
        let plan = plan_playback(&stream, false, &bundled);
        assert_eq!(plan.rendition.unwrap().height, 1080);
        assert!(!plan.reduced);
    }

    #[test]
    fn drm_is_refused_unless_the_owner_enabled_it() {
        let caps = MediaCapabilities::base();
        let stream = vec![Rendition::new("vp09.00.10.08", 1080, 4_000)];
        let plan = plan_playback(&stream, true, &caps);
        assert_eq!(plan.block, Some(PlaybackBlock::DrmNotEnabled));
        assert!(plan.message.contains("protected content"));

        let enabled = MediaCapabilities {
            acceleration: Acceleration::software(),
            codec_bundle: true,
            drm: true,
        };
        let plan = plan_playback(&stream, true, &enabled);
        assert!(plan.rendition.is_some());
        assert!(!plan.is_blocked());
    }

    #[test]
    fn empty_ladders_report_an_error_instead_of_panicking() {
        let caps = MediaCapabilities::base();
        let plan = plan_playback(&[], false, &caps);
        assert!(plan.is_blocked());
        assert_eq!(plan.message, "no playable rendition");
        assert!(!plan.reduced);
    }

    #[test]
    fn equal_heights_break_ties_on_bandwidth() {
        let caps = MediaCapabilities::base();
        let stream = vec![
            Rendition::new("vp09.00.10.08", 720, 1_500),
            Rendition::new("av01.0.04M.08", 720, 3_000),
        ];
        let plan = plan_playback(&stream, false, &caps);
        assert_eq!(plan.rendition.unwrap().bandwidth_kbps, 3_000);
    }
}
