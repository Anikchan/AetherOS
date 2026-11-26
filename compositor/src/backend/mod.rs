//! P3-T09 output backend selection.
//!
//! The compositor has two output backends:
//!
//! - [`BackendKind::Headless`] — the graphics-free backend every earlier P3 task
//!   was developed and accepted on. It advertises two `wl_output`s, renders
//!   nothing and never touches a device node, so it runs on the macOS dev host
//!   and on a CI runner without a GPU.
//! - [`BackendKind::Drm`] — the real hardware backend (`backend/drm.rs`, Linux
//!   only, compiled with the `drm` cargo feature): DRM/KMS page flips on the
//!   primary plane through GBM + EGL/GLES, with libinput reading evdev through
//!   a libseat-managed udev seat.
//!
//! Selection is driven by `AETHER_COMPOSITOR_BACKEND`
//! (`headless` | `drm` | `kms` | `auto`, default `auto`). The resolution logic
//! in [`select_backend`] is pure so it unit-tests on any host, including a
//! machine without a GPU; a DRM run itself is hardware-gated and is never
//! claimed by the headless test suite.

#[cfg(all(target_os = "linux", feature = "drm"))]
pub mod drm;

/// Environment variable that selects the output backend.
pub const BACKEND_ENV: &str = "AETHER_COMPOSITOR_BACKEND";

/// Which output backend this process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// No device: two synthetic outputs, no rendering, no input.
    Headless,
    /// DRM/KMS scanout plus libinput input on a real GPU/seat.
    Drm,
}

impl BackendKind {
    /// Stable name used in the `AETHER_COMPOSITOR_BACKEND` marker and docs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::Drm => "drm",
        }
    }

    /// Parse an explicit backend name. `auto` and unknown values are handled by
    /// [`select_backend`], not here.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "headless" => Some(Self::Headless),
            "drm" | "kms" => Some(Self::Drm),
            _ => None,
        }
    }
}

/// Resolve `AETHER_COMPOSITOR_BACKEND` into a backend.
///
/// Rules (all covered by the `p3_t09_backend_selection` tests):
///
/// 1. `headless` always wins, so a machine with a GPU can still be forced onto
///    the backend CI uses.
/// 2. `drm`/`kms` requests DRM even when no device node was found — the backend
///    then fails loudly with a diagnosable error instead of silently falling
///    back to a black headless screen.
/// 3. `auto` (and an unset variable) picks DRM when a DRM device node exists,
///    headless otherwise, so the same image boots on the Dell and in CI.
/// 4. An unrecognised value falls back to headless: a typo must not leave the
///    desktop without any output.
/// 5. If the crate was not built with the `drm` feature, DRM can never be
///    selected; the caller prints why.
pub fn select_backend(
    requested: Option<&str>,
    drm_available: bool,
    drm_compiled: bool,
) -> BackendKind {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    let wants_drm = match requested {
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "headless" => false,
            "drm" | "kms" => true,
            "auto" => drm_available,
            // A typo must not silently pick a backend: fall back to headless
            // (the caller reports the unknown value).
            _ => false,
        },
        None => drm_available,
    };
    if wants_drm && drm_compiled {
        BackendKind::Drm
    } else {
        BackendKind::Headless
    }
}

/// True when an explicitly unparsable `AETHER_COMPOSITOR_BACKEND` value was
/// given, so the caller can warn instead of silently ignoring a typo.
pub fn is_unknown_backend(requested: Option<&str>) -> bool {
    match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => !matches!(
            value.to_ascii_lowercase().as_str(),
            "headless" | "drm" | "kms" | "auto"
        ),
        None => false,
    }
}

/// Whether this machine exposes a DRM device node (`/dev/dri/card*`).
///
/// Only used to break the `auto` tie; a false negative simply means the
/// operator has to pass `AETHER_COMPOSITOR_BACKEND=drm`.
pub fn drm_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::path::Path;
        for index in 0..8 {
            if Path::new(&format!("/dev/dri/card{index}")).exists() {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Total logical area of a layer surface's output, in layer-shell terms.
///
/// `anchors` decides which edges the surface is pinned to; `size` is the size
/// the compositor configured (see `layer_surface_size`). Both axes fall back to
/// centring when the surface is not anchored, which matches how the shell uses
/// the protocol (menu bar anchored top, dock anchored bottom).
pub fn layer_location(
    anchors: (bool, bool, bool, bool),
    size: (i32, i32),
    output: (i32, i32),
) -> (i32, i32) {
    let (left, right, top, bottom) = anchors;
    let x = if left && !right {
        0
    } else if right && !left {
        (output.0 - size.0).max(0)
    } else {
        ((output.0 - size.0) / 2).max(0)
    };
    let y = if top && !bottom {
        0
    } else if bottom && !top {
        (output.1 - size.1).max(0)
    } else {
        ((output.1 - size.1) / 2).max(0)
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_is_always_selectable() {
        assert_eq!(
            select_backend(Some("headless"), true, true),
            BackendKind::Headless
        );
    }

    #[test]
    fn explicit_drm_wins_even_without_a_device() {
        // The backend then reports a diagnosable error; falling back silently
        // would boot the Dell to a black headless screen.
        assert_eq!(select_backend(Some("drm"), false, true), BackendKind::Drm);
        assert_eq!(select_backend(Some("KMS"), false, true), BackendKind::Drm);
    }

    #[test]
    fn auto_follows_device_presence() {
        assert_eq!(select_backend(Some("auto"), true, true), BackendKind::Drm);
        assert_eq!(
            select_backend(Some("auto"), false, true),
            BackendKind::Headless
        );
        // Unset is the same as `auto`.
        assert_eq!(select_backend(None, true, true), BackendKind::Drm);
        assert_eq!(select_backend(None, false, true), BackendKind::Headless);
    }

    #[test]
    fn drm_requires_the_cargo_feature() {
        assert_eq!(
            select_backend(Some("drm"), true, false),
            BackendKind::Headless
        );
        assert_eq!(
            select_backend(Some("auto"), true, false),
            BackendKind::Headless
        );
    }

    #[test]
    fn unknown_values_fall_back_to_headless() {
        assert_eq!(
            select_backend(Some("drmm"), true, true),
            BackendKind::Headless
        );
        assert_eq!(select_backend(Some("   "), true, true), BackendKind::Drm);
        assert!(is_unknown_backend(Some("drmm")));
        assert!(!is_unknown_backend(Some("auto")));
        assert!(!is_unknown_backend(None));
    }

    #[test]
    fn backend_names_round_trip() {
        for kind in [BackendKind::Headless, BackendKind::Drm] {
            assert_eq!(BackendKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(BackendKind::parse("nope"), None);
    }

    #[test]
    fn layer_location_pins_to_anchored_edges() {
        // Menu bar: anchored to the top edge.
        assert_eq!(
            layer_location((false, false, true, false), (1920, 32), (1920, 1080)),
            (0, 0)
        );
        // Dock: anchored to the bottom edge, 64 px tall.
        assert_eq!(
            layer_location((false, false, false, true), (1920, 64), (1920, 1080)),
            (0, 1016)
        );
        // Unanchored: centred.
        assert_eq!(
            layer_location((false, false, false, false), (400, 300), (1920, 1080)),
            (760, 390)
        );
        // Anchored to both horizontal edges: the surface fills the width, so
        // the left edge wins.
        assert_eq!(
            layer_location((true, true, true, false), (1920, 32), (1920, 1080)),
            (0, 0)
        );
    }
}
