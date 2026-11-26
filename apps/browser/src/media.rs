//! Browser media and streaming support (P4-T18a, DA-AC-11).
//!
//! The codec/DRM decision table is shared with the Player through
//! [`aether_media`] (P0-T03: H.264/AAC stay in the optional bundle; Q24: the
//! browser plays HD or a reduced rung instead of failing). This module adds the
//! browser-specific half:
//!
//! * [`classify`] — is this URL a page, a progressive media file, HLS or DASH,
//! * [`renditions`] — the rendition ladder such a stream usually offers, so
//!   `--media-check` and the status bar can answer "what would this play as"
//!   without a network round trip,
//! * [`status_line`] — the one-line note the chrome shows after navigating.
//!
//! The embedded WebKitGTK engine performs the real playback; the ladder here is
//! the deterministic policy input the headless tests and the acceptance
//! harness drive.

use aether_media::{plan_playback, MediaCapabilities, PlaybackPlan, Rendition};

/// What a URL points at, as far as media playback is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSource {
    /// A normal web page: the engine renders it, no media policy applies.
    Page,
    /// A single video file (`mp4`, `webm`, `mkv`, ...).
    ProgressiveVideo,
    /// A single audio file (`mp3`, `flac`, `opus`, ...).
    ProgressiveAudio,
    /// An HLS playlist (`.m3u8`).
    Hls,
    /// An MPEG-DASH manifest (`.mpd`).
    Dash,
}

impl MediaSource {
    pub fn is_media(self) -> bool {
        self != MediaSource::Page
    }

    pub fn is_streaming(self) -> bool {
        matches!(self, MediaSource::Hls | MediaSource::Dash)
    }

    pub fn label(self) -> &'static str {
        match self {
            MediaSource::Page => "page",
            MediaSource::ProgressiveVideo => "progressive-video",
            MediaSource::ProgressiveAudio => "progressive-audio",
            MediaSource::Hls => "hls",
            MediaSource::Dash => "dash",
        }
    }

    /// The generic ladder for this kind of stream: H.264/AAC at the top rung
    /// (what most sites serve) with a royalty-free rung below it. That is
    /// exactly the HD-or-reduced decision Q24 describes. [`renditions`] narrows
    /// it by the file extension.
    pub fn typical_renditions(self) -> Vec<Rendition> {
        match self {
            MediaSource::Page => Vec::new(),
            MediaSource::ProgressiveVideo | MediaSource::Hls | MediaSource::Dash => vec![
                Rendition::new("avc1.640028", 1080, 5_000),
                Rendition::new("vp09.00.10.08", 720, 2_500),
            ],
            MediaSource::ProgressiveAudio => vec![Rendition::new("mp4a.40.2", 0, 128)],
        }
    }
}

/// Classify a URL by its path extension. Query strings and fragments are
/// ignored, and `about:`/`data:`/`file:`-without-extension URLs are pages.
pub fn classify(url: &str) -> MediaSource {
    let Some(extension) = asset_extension(url) else {
        return MediaSource::Page;
    };
    match extension.as_str() {
        "m3u8" => MediaSource::Hls,
        "mpd" => MediaSource::Dash,
        "mp4" | "m4v" | "webm" | "mkv" | "mov" | "ogv" => MediaSource::ProgressiveVideo,
        "mp3" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "opus" | "wav" => {
            MediaSource::ProgressiveAudio
        }
        _ => MediaSource::Page,
    }
}

/// Lowercase extension of the URL path, without query or fragment.
pub fn asset_extension(url: &str) -> Option<String> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or_default();
    let (_, extension) = name.rsplit_once('.')?;
    let extension = extension.trim().to_ascii_lowercase();
    if extension.is_empty() || extension.len() > 5 {
        None
    } else {
        Some(extension)
    }
}

/// The rendition ladder a URL usually offers, keyed by extension.
///
/// A real HLS/DASH manifest replaces this with its own ladder; the table here
/// covers the codecs each container normally carries so the policy can be
/// checked headlessly (and by `aether-browser --media-check`).
pub fn renditions(url: &str) -> Vec<Rendition> {
    let source = classify(url);
    let extension = asset_extension(url).unwrap_or_default();
    match (source, extension.as_str()) {
        (MediaSource::ProgressiveAudio, "mp3") => vec![Rendition::new("mp4a.6b", 0, 192)],
        (MediaSource::ProgressiveAudio, "flac") => vec![Rendition::new("audio/x-flac", 0, 900)],
        (MediaSource::ProgressiveAudio, "ogg")
        | (MediaSource::ProgressiveAudio, "oga")
        | (MediaSource::ProgressiveAudio, "opus") => {
            vec![Rendition::new("audio/x-opus", 0, 160)]
        }
        (MediaSource::ProgressiveAudio, "wav") => vec![Rendition::new("audio/x-wav", 0, 1_411)],
        // MP4-family containers carry H.264/AAC in practice.
        (MediaSource::ProgressiveAudio, "m4a") | (MediaSource::ProgressiveAudio, "aac") => {
            vec![Rendition::new("mp4a.40.2", 0, 256)]
        }
        (MediaSource::ProgressiveVideo, "mp4")
        | (MediaSource::ProgressiveVideo, "m4v")
        | (MediaSource::ProgressiveVideo, "mov")
        | (MediaSource::ProgressiveVideo, "mkv") => vec![
            Rendition::new("avc1.640028", 1080, 5_000),
            Rendition::new("mp4a.40.2", 0, 128),
        ],
        (MediaSource::ProgressiveVideo, "webm") => vec![
            Rendition::new("vp09.00.10.08", 1080, 4_000),
            Rendition::new("audio/x-opus", 0, 160),
        ],
        (MediaSource::ProgressiveVideo, "ogv") => vec![
            Rendition::new("theora", 720, 2_000),
            Rendition::new("audio/x-vorbis", 0, 128),
        ],
        (MediaSource::Page, _) => Vec::new(),
        // mp4/m4v/mov/mkv, m4a/aac and every manifest: the generic ladder.
        _ => source.typical_renditions(),
    }
}

/// Playback plan for a URL this session opened.
pub fn plan_for(url: &str, caps: &MediaCapabilities) -> PlaybackPlan {
    plan_playback(&renditions(url), false, caps)
}

/// Status-bar note for a media URL (`None` for ordinary pages).
pub fn status_line(url: &str, caps: &MediaCapabilities) -> Option<String> {
    let source = classify(url);
    if !source.is_media() {
        return None;
    }
    let plan = plan_for(url, caps);
    Some(format!("media: {} · {}", source.label(), plan.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_media::Acceleration;

    fn software_base() -> MediaCapabilities {
        MediaCapabilities::base()
    }

    fn hardware_base() -> MediaCapabilities {
        MediaCapabilities {
            acceleration: Acceleration::hardware(),
            codec_bundle: false,
            drm: false,
        }
    }

    #[test]
    fn urls_classify_by_extension_not_by_query() {
        assert_eq!(classify("https://aether.example/docs"), MediaSource::Page);
        assert_eq!(classify("about:home"), MediaSource::Page);
        assert_eq!(
            classify("https://aether.example/clip.mp4"),
            MediaSource::ProgressiveVideo
        );
        assert_eq!(
            classify("https://aether.example/clip.mp4?token=abc#t=10"),
            MediaSource::ProgressiveVideo
        );
        assert_eq!(
            classify("https://cdn.example/live/index.m3u8?key=1"),
            MediaSource::Hls
        );
        assert_eq!(
            classify("https://cdn.example/manifest.mpd"),
            MediaSource::Dash
        );
        assert_eq!(
            classify("https://aether.example/song.FLAC"),
            MediaSource::ProgressiveAudio
        );
        assert_eq!(
            classify("https://aether.example/archive.zip"),
            MediaSource::Page
        );
        assert_eq!(
            classify("https://aether.example/v1.2/notes"),
            MediaSource::Page
        );
        assert!(MediaSource::Hls.is_streaming());
        assert!(!MediaSource::ProgressiveVideo.is_streaming());
        assert!(MediaSource::ProgressiveAudio.is_media());
        assert!(!MediaSource::Page.is_media());
        assert_eq!(MediaSource::Dash.label(), "dash");
    }

    #[test]
    fn every_media_kind_offers_a_ladder() {
        for source in [
            MediaSource::ProgressiveVideo,
            MediaSource::ProgressiveAudio,
            MediaSource::Hls,
            MediaSource::Dash,
        ] {
            assert!(
                !source.typical_renditions().is_empty(),
                "{}",
                source.label()
            );
        }
        assert!(MediaSource::Page.typical_renditions().is_empty());
        let streaming = MediaSource::Hls.typical_renditions();
        assert_eq!(streaming[0].height, 1080, "H.264 HD is the top rung");
        assert_eq!(streaming[1].height, 720, "VP9 720p is the reduced rung");
    }

    #[test]
    fn royalty_free_files_play_and_h264_mp4_needs_the_bundle() {
        // mp4 = H.264/AAC: refused on the base image with a bundle hint.
        let plan = plan_for("https://a.test/clip.mp4", &software_base());
        assert!(plan.is_blocked());
        assert!(plan.message.contains("codec bundle"));

        // webm = VP9/Opus: playable, and hardware decode when VA-API is there.
        let plan = plan_for("https://a.test/clip.webm", &hardware_base());
        assert!(!plan.is_blocked());
        assert!(plan.message.contains("hardware decode"));
        assert_eq!(plan.rendition.as_ref().unwrap().height, 1080);

        // FLAC/MP3/Opus audio plays without any bundle.
        for url in [
            "https://a.test/song.flac",
            "https://a.test/song.mp3",
            "https://a.test/song.opus",
            "https://a.test/song.wav",
        ] {
            let plan = plan_for(url, &software_base());
            assert!(!plan.is_blocked(), "{url} should play: {}", plan.message);
        }

        // The bundle turns the mp4 into full HD.
        let bundled = MediaCapabilities {
            acceleration: Acceleration::hardware(),
            codec_bundle: true,
            drm: false,
        };
        let plan = plan_for("https://a.test/clip.mp4", &bundled);
        assert_eq!(plan.rendition.unwrap().height, 1080);
        assert!(!plan.reduced);
    }

    #[test]
    fn m4a_audio_needs_the_bundle_like_video() {
        let plan = plan_for("https://a.test/song.m4a", &software_base());
        assert!(plan.is_blocked(), "AAC audio: {}", plan.message);
        assert!(plan.message.contains("AAC"));
    }

    #[test]
    fn status_line_only_covers_media_urls() {
        let caps = hardware_base();
        assert!(status_line("https://aether.example/docs", &caps).is_none());
        let line = status_line("https://cdn.example/live/index.m3u8", &caps).unwrap();
        assert!(line.contains("media: hls"));
        assert!(line.contains("reduced quality"), "H.264 HD is out: {line}");
        let audio = status_line("https://aether.example/song.m4a", &caps).unwrap();
        assert!(audio.contains("progressive-audio"));
        assert!(audio.contains("codec bundle"), "AAC is restricted: {audio}");
    }

    #[test]
    fn extension_parsing_is_defensive() {
        assert_eq!(asset_extension("https://a.test/a.Mp4"), Some("mp4".into()));
        assert_eq!(asset_extension("https://a.test/dir/"), None);
        assert_eq!(asset_extension("https://a.test/no-extension"), None);
        assert_eq!(asset_extension("about:home"), None);
        assert_eq!(asset_extension("https://a.test/a.verylongextension"), None);
    }
}
