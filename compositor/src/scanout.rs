//! P3-T07 direct scanout for full-screen apps.
//!
//! Pure data model (no Wayland/DRM) so it unit-tests on the dev host and in CI.
//! `AetherCompositor` feeds it the visible toplevels, their managed geometry,
//! whether their committed buffer is opaque, the number of shell overlays
//! (layer-shell surfaces in the Top/Overlay layers) and the primary output size.
//!
//! The rule is deliberately strict: composition is bypassed only when the
//! primary output shows exactly **one** visible window, that window covers the
//! whole output exactly, its buffer is opaque (XRGB/opaque-region), and no
//! panel/dock/notification overlay sits on top of it. Anything else keeps the
//! composited path, so a full-screen video can be scanned out while a menu bar
//! still forces composition.
//!
//! Markers (printed by `AetherCompositor`):
//! - `AETHER_COMPOSITOR_SCANOUT: mode=direct window=… output=… geometry=…`
//!   on the transition into direct scanout;
//! - `AETHER_COMPOSITOR_SCANOUT: mode=composite reason=… windows=… output=…`
//!   on the transition back to composition (also the initial state);
//! - `AETHER_COMPOSITOR_SCANOUT_FRAME: n=… window=… damage_px=…` for the
//!   frames presented on the bypass path;
//! - `AETHER_COMPOSITOR_SCANOUT_STATS: mode=… direct_frames=… transitions=…`.

use crate::wm::Rect;

/// One visible toplevel as the scanout decision sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanoutSurface {
    /// Stable window id (the xdg-toplevel index in the compositor).
    pub id: u64,
    /// Managed geometry in logical output coordinates.
    pub rect: Rect,
    /// The committed buffer is guaranteed to be fully opaque.
    pub opaque: bool,
    /// The window is on the active workspace and its surface is alive.
    pub visible: bool,
}

/// Why composition cannot be bypassed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeReason {
    NoWindows,
    MultipleWindows,
    NotFullscreen,
    NotOpaque,
    OverlayPresent,
}

impl CompositeReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoWindows => "no_windows",
            Self::MultipleWindows => "multiple_windows",
            Self::NotFullscreen => "not_fullscreen",
            Self::NotOpaque => "not_opaque",
            Self::OverlayPresent => "overlay_present",
        }
    }
}

/// The routing decision for the next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanoutDecision {
    /// Bypass composition: the client buffer can go straight to the output.
    Direct { window: u64 },
    /// Compose the frame as usual.
    Composite {
        reason: CompositeReason,
        windows: usize,
    },
}

/// Decide whether the primary output can scan a client buffer out directly.
///
/// `overlays` is the number of visible layer-shell surfaces in the Top or
/// Overlay layer: any of them needs composition above the full-screen window.
pub fn decide_scanout(
    surfaces: &[ScanoutSurface],
    output: Rect,
    overlays: usize,
) -> ScanoutDecision {
    let visible: Vec<&ScanoutSurface> = surfaces.iter().filter(|s| s.visible).collect();

    if visible.is_empty() {
        return ScanoutDecision::Composite {
            reason: CompositeReason::NoWindows,
            windows: 0,
        };
    }
    if overlays > 0 {
        return ScanoutDecision::Composite {
            reason: CompositeReason::OverlayPresent,
            windows: visible.len(),
        };
    }
    if visible.len() > 1 {
        return ScanoutDecision::Composite {
            reason: CompositeReason::MultipleWindows,
            windows: visible.len(),
        };
    }

    let surface = visible[0];
    if surface.rect != output {
        return ScanoutDecision::Composite {
            reason: CompositeReason::NotFullscreen,
            windows: 1,
        };
    }
    if !surface.opaque {
        return ScanoutDecision::Composite {
            reason: CompositeReason::NotOpaque,
            windows: 1,
        };
    }
    ScanoutDecision::Direct {
        window: surface.id,
    }
}

/// Scanout state with change detection for marker emission.
#[derive(Debug, Clone)]
pub struct DirectScanout {
    decision: ScanoutDecision,
    reported: Option<ScanoutDecision>,
    /// Frames presented on the bypass path.
    pub direct_frames: u64,
    /// Times the decision changed (direct <-> composited).
    pub transitions: u32,
}

impl Default for DirectScanout {
    fn default() -> Self {
        Self::new()
    }
}

impl DirectScanout {
    pub fn new() -> Self {
        Self {
            decision: ScanoutDecision::Composite {
                reason: CompositeReason::NoWindows,
                windows: 0,
            },
            reported: None,
            direct_frames: 0,
            transitions: 0,
        }
    }

    pub fn decision(&self) -> ScanoutDecision {
        self.decision
    }

    pub fn is_direct(&self) -> bool {
        matches!(self.decision, ScanoutDecision::Direct { .. })
    }

    pub fn direct_window(&self) -> Option<u64> {
        match self.decision {
            ScanoutDecision::Direct { window } => Some(window),
            ScanoutDecision::Composite { .. } => None,
        }
    }

    /// Update the decision. Returns `Some(previous)` when the marker should be
    /// printed (every change, including the first evaluation).
    pub fn update(&mut self, next: ScanoutDecision) -> Option<ScanoutDecision> {
        if self.reported == Some(next) {
            return None;
        }
        let previous = self.decision;
        self.decision = next;
        self.reported = Some(next);
        self.transitions += 1;
        Some(previous)
    }

    /// The marker line for the current decision. `output_rect` is the logical
    /// size the full-screen window must match.
    pub fn marker_line(&self, output: &str, output_rect: Rect) -> String {
        match self.decision {
            ScanoutDecision::Direct { window } => format!(
                "AETHER_COMPOSITOR_SCANOUT: mode=direct window={window} output={output} geometry={},{},{}x{} bypass=composition",
                output_rect.x, output_rect.y, output_rect.w, output_rect.h
            ),
            ScanoutDecision::Composite { reason, windows } => format!(
                "AETHER_COMPOSITOR_SCANOUT: mode=composite reason={} windows={windows} output={output}",
                reason.as_str()
            ),
        }
    }

    pub fn stats_line(&self) -> String {
        format!(
            "mode={} direct_frames={} transitions={}",
            if self.is_direct() {
                "direct"
            } else {
                "composite"
            },
            self.direct_frames,
            self.transitions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
    };

    fn fullscreen_window(id: u64, opaque: bool) -> ScanoutSurface {
        ScanoutSurface {
            id,
            rect: OUTPUT,
            opaque,
            visible: true,
        }
    }

    #[test]
    fn single_fullscreen_opaque_window_is_scanned_out() {
        let decision = decide_scanout(&[fullscreen_window(7, true)], OUTPUT, 0);
        assert_eq!(decision, ScanoutDecision::Direct { window: 7 });
    }

    #[test]
    fn single_window_covers_the_output_exactly() {
        // A window one pixel short still needs composition (no scaling path
        // modelled yet), and so does one larger than the output.
        let mut surface = fullscreen_window(1, true);
        surface.rect.w -= 1;
        assert_eq!(
            decide_scanout(&[surface], OUTPUT, 0),
            ScanoutDecision::Composite {
                reason: CompositeReason::NotFullscreen,
                windows: 1
            }
        );
        surface.rect = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1081,
        };
        assert!(matches!(
            decide_scanout(&[surface], OUTPUT, 0),
            ScanoutDecision::Composite {
                reason: CompositeReason::NotFullscreen,
                ..
            }
        ));
    }

    #[test]
    fn multiple_visible_windows_need_composition() {
        let decision = decide_scanout(
            &[fullscreen_window(1, true), fullscreen_window(2, true)],
            OUTPUT,
            0,
        );
        assert_eq!(
            decision,
            ScanoutDecision::Composite {
                reason: CompositeReason::MultipleWindows,
                windows: 2
            }
        );
    }

    #[test]
    fn transparent_fullscreen_window_needs_composition() {
        assert_eq!(
            decide_scanout(&[fullscreen_window(3, false)], OUTPUT, 0),
            ScanoutDecision::Composite {
                reason: CompositeReason::NotOpaque,
                windows: 1
            }
        );
    }

    #[test]
    fn shell_overlay_disables_scanout() {
        assert_eq!(
            decide_scanout(&[fullscreen_window(4, true)], OUTPUT, 1),
            ScanoutDecision::Composite {
                reason: CompositeReason::OverlayPresent,
                windows: 1
            }
        );
    }

    #[test]
    fn hidden_windows_are_ignored() {
        let mut hidden = fullscreen_window(9, true);
        hidden.visible = false;
        let decision = decide_scanout(&[hidden, fullscreen_window(5, true)], OUTPUT, 0);
        assert_eq!(decision, ScanoutDecision::Direct { window: 5 });
    }

    #[test]
    fn empty_output_is_composited() {
        assert_eq!(
            decide_scanout(&[], OUTPUT, 0),
            ScanoutDecision::Composite {
                reason: CompositeReason::NoWindows,
                windows: 0
            }
        );
    }

    #[test]
    fn non_fullscreen_single_window_is_composited() {
        let mut surface = fullscreen_window(6, true);
        surface.rect = Rect {
            x: 100,
            y: 100,
            w: 640,
            h: 480,
        };
        assert_eq!(
            decide_scanout(&[surface], OUTPUT, 0),
            ScanoutDecision::Composite {
                reason: CompositeReason::NotFullscreen,
                windows: 1
            }
        );
    }

    #[test]
    fn markers_and_transition_counting() {
        let mut scanout = DirectScanout::new();
        assert!(!scanout.is_direct());
        // First evaluation always reports (initial composite state).
        assert!(scanout
            .update(decide_scanout(&[], OUTPUT, 0))
            .is_some());
        assert_eq!(scanout.transitions, 1);
        let line = scanout.marker_line("aether-headless-0", OUTPUT);
        assert!(line.contains("mode=composite reason=no_windows"), "{line}");

        let direct = decide_scanout(&[fullscreen_window(9, true)], OUTPUT, 0);
        assert!(scanout.update(direct).is_some());
        assert!(scanout.is_direct());
        assert_eq!(scanout.direct_window(), Some(9));
        let line = scanout.marker_line("aether-headless-0", OUTPUT);
        assert!(line.contains("mode=direct window=9"), "{line}");
        assert!(line.contains("geometry=0,0,1920x1080"), "{line}");

        // Unchanged decisions do not re-print.
        assert!(scanout.update(direct).is_none());
        assert_eq!(scanout.transitions, 2);
        assert!(scanout.stats_line().contains("mode=direct"));
    }
}
