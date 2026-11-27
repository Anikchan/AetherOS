//! Layer-shell host contract for the desktop chrome (P3-T02 → P4, S3).
//!
//! [`crate::layers`] says *what* the shell wants on screen; this module is the
//! bridge to the compositor host: it turns every descriptor into the exact
//! `zwlr_layer_surface_v1` request the device host sends, mirrors the
//! compositor's size negotiation so a surface is drawn at the size it will be
//! configured to, and reads the compositor's own markers back to prove that
//! every surface bound and configured.
//!
//! Protocol values are inlined from `wlr-layer-shell-unstable-v1.xml` so the
//! shell side can be built and tested without a Wayland client library (the
//! Linux image links `wayland-client`; the macOS dev host does not have
//! `libwayland`). If a value here disagreed with the protocol, the compositor's
//! `AETHER_COMPOSITOR_LAYER_SURFACE` / `LAYER_CONFIGURE` markers would not match
//! the plan and [`BindingReport`] would fail.
//!
//! `aether-shell --host-check <compositor.log>` prints the report for the
//! on-device acceptance harness.

use crate::layers::{self, Keyboard, Layer, LayerSurfaceSpec};
#[cfg(test)]
use crate::layers::specs;

/// `zwlr_layer_surface_v1.anchor` bitfield values.
pub const ANCHOR_TOP: u32 = 1;
pub const ANCHOR_BOTTOM: u32 = 2;
pub const ANCHOR_LEFT: u32 = 4;
pub const ANCHOR_RIGHT: u32 = 8;

/// `zwlr_layer_surface_v1.keyboard_interactivity` values.
pub const KEYBOARD_NONE: u32 = 0;
pub const KEYBOARD_EXCLUSIVE: u32 = 1;
pub const KEYBOARD_ON_DEMAND: u32 = 2;

/// The `zwlr_layer_shell_v1.layer` enum value for a descriptor.
pub fn layer_value(layer: Layer) -> u32 {
    match layer {
        Layer::Background => 0,
        Layer::Bottom => 1,
        Layer::Top => 2,
        Layer::Overlay => 3,
    }
}

/// The layer name the compositor prints in its markers (`Layer`'s `Debug`).
pub fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::Background => "Background",
        Layer::Bottom => "Bottom",
        Layer::Top => "Top",
        Layer::Overlay => "Overlay",
    }
}

/// One `get_layer_surface` + setup request, ready to be sent by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRequest {
    pub name: &'static str,
    pub namespace: &'static str,
    /// `zwlr_layer_shell_v1.layer` value.
    pub layer: u32,
    /// Compositor marker spelling of the layer.
    pub layer_name: &'static str,
    /// `zwlr_layer_surface_v1.anchor` bitfield.
    pub anchors: u32,
    pub width: i32,
    pub height: i32,
    pub exclusive_zone: i32,
    /// Margin from the anchored edges, in logical pixels.
    pub margin_top: i32,
    /// `zwlr_layer_surface_v1.keyboard_interactivity` value.
    pub keyboard_interactivity: u32,
}

impl HostRequest {
    pub fn from_spec(spec: &LayerSurfaceSpec) -> HostRequest {
        let mut anchors = 0;
        if spec.anchor_top {
            anchors |= ANCHOR_TOP;
        }
        if spec.anchor_bottom {
            anchors |= ANCHOR_BOTTOM;
        }
        if spec.anchor_left {
            anchors |= ANCHOR_LEFT;
        }
        if spec.anchor_right {
            anchors |= ANCHOR_RIGHT;
        }
        HostRequest {
            name: spec.name,
            namespace: spec.namespace,
            layer: layer_value(spec.layer),
            layer_name: layer_name(spec.layer),
            anchors,
            width: spec.width,
            height: spec.height,
            exclusive_zone: spec.exclusive_zone,
            margin_top: spec.margin_top,
            keyboard_interactivity: match spec.keyboard {
                Keyboard::None => KEYBOARD_NONE,
                Keyboard::Exclusive => KEYBOARD_EXCLUSIVE,
                Keyboard::OnDemand => KEYBOARD_ON_DEMAND,
            },
        }
    }

    /// Size the compositor will configure for this request on `output` (logical
    /// pixels). Mirrors `layer_surface_size` in the compositor, margins
    /// included.
    pub fn negotiate(&self, output: (i32, i32)) -> (i32, i32) {
        let width = if self.anchors & ANCHOR_LEFT != 0 && self.anchors & ANCHOR_RIGHT != 0 {
            output.0
        } else if self.width > 0 {
            self.width
        } else {
            output.0
        };
        let available = output.1 - self.margin_top;
        let height = if self.anchors & ANCHOR_TOP != 0 && self.anchors & ANCHOR_BOTTOM != 0 {
            available
        } else if self.height > 0 {
            self.height
        } else {
            available
        };
        (width.max(1), height.max(1))
    }

    /// Where the compositor places this surface on `output` (logical pixels).
    ///
    /// Mirrors the layer-shell anchoring rules for the anchors the plan uses:
    /// a stretched axis covers the output, a sized axis sits at the anchored
    /// edge (or is centred when nothing is anchored), and margins offset the
    /// anchored edge.
    pub fn placement(&self, output: (i32, i32)) -> (i32, i32) {
        let (width, height) = self.negotiate(output);
        let x = if self.anchors & ANCHOR_LEFT != 0 {
            0
        } else if self.anchors & ANCHOR_RIGHT != 0 {
            output.0 - width
        } else {
            (output.0 - width) / 2
        };
        let y = if self.anchors & ANCHOR_TOP != 0 {
            self.margin_top
        } else if self.anchors & ANCHOR_BOTTOM != 0 {
            output.1 - height
        } else {
            (output.1 - height) / 2
        };
        (x, y)
    }
}

/// The request the host sends for every shell surface, in creation order.
pub fn plan() -> Vec<HostRequest> {
    plan_at(layers::MIN_TEXT_SCALE)
}

/// The same plan at an accessibility text scale: the menu bar grows, the
/// right-hand panels widen, and their top margin follows the menu bar.
pub fn plan_at(text_scale: f32) -> Vec<HostRequest> {
    layers::specs_at(text_scale)
        .iter()
        .map(HostRequest::from_spec)
        .collect()
}

/// Size the compositor will configure for `spec` on `output`.
pub fn negotiate(spec: &LayerSurfaceSpec, output: (i32, i32)) -> (i32, i32) {
    HostRequest::from_spec(spec).negotiate(output)
}

/// What the compositor reported about one planned surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceBinding {
    pub name: &'static str,
    pub namespace: &'static str,
    /// Layer the plan asks for (`Top`, `Overlay`, ...).
    pub expected_layer: &'static str,
    /// `AETHER_COMPOSITOR_LAYER_SURFACE` was seen for this namespace.
    pub bound: bool,
    /// Layer the compositor reported, when it reported one.
    pub reported_layer: Option<String>,
    /// `AETHER_COMPOSITOR_LAYER_CONFIGURE` size, when the compositor sent one.
    pub configured_size: Option<(i32, i32)>,
    /// Size the plan expects on the reported output.
    pub expected_size: (i32, i32),
}

impl SurfaceBinding {
    /// Bound, configured, on the expected layer and at the expected size.
    pub fn is_ok(&self) -> bool {
        self.problem().is_none()
    }

    /// Why this surface is not ok, in harness wording.
    pub fn problem(&self) -> Option<String> {
        if !self.bound {
            return Some(format!(
                "{} was never created (no AETHER_COMPOSITOR_LAYER_SURFACE for namespace={})",
                self.name, self.namespace
            ));
        }
        if let Some(layer) = &self.reported_layer {
            if layer != self.expected_layer {
                return Some(format!(
                    "{} is on layer {layer}, the plan wants {}",
                    self.name, self.expected_layer
                ));
            }
        }
        match self.configured_size {
            None => Some(format!(
                "{} bound but was never configured (no AETHER_COMPOSITOR_LAYER_CONFIGURE)",
                self.name
            )),
            Some(size) if size != self.expected_size => Some(format!(
                "{} configured at {}x{}, the plan expects {}x{}",
                self.name, size.0, size.1, self.expected_size.0, self.expected_size.1
            )),
            Some(_) => None,
        }
    }
}

/// Primary output the compositor reported, and its logical size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    pub name: String,
    pub logical_size: (i32, i32),
}

/// The result of reading a compositor log: every planned surface, bound or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingReport {
    pub output: Option<OutputInfo>,
    pub surfaces: Vec<SurfaceBinding>,
}

impl BindingReport {
    /// Parse the compositor's stdout/journal text.
    pub fn scan(log: &str) -> BindingReport {
        let mut output: Option<OutputInfo> = None;
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_OUTPUT: ") {
                // `name=… mode=WxH scale=S logical=WxH position=x,y`
                let name = field(rest, "name").unwrap_or_default();
                let logical = field(rest, "logical")
                    .or_else(|| field(rest, "mode"))
                    .and_then(parse_dimensions)
                    .unwrap_or((0, 0));
                if output.is_none() {
                    output = Some(OutputInfo {
                        name: name.to_string(),
                        logical_size: logical,
                    });
                }
            }
        }
        let output_size = output
            .as_ref()
            .map(|output| output.logical_size)
            .unwrap_or((0, 0));

        let surfaces = plan()
            .into_iter()
            .map(|request| {
                let expected_size = request.negotiate(output_size);
                let mut bound = false;
                let mut reported_layer = None;
                let mut configured_size = None;
                for line in log.lines() {
                    if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_LAYER_SURFACE: ") {
                        if field(rest, "namespace") == Some(request.namespace) {
                            bound = true;
                            reported_layer = field(rest, "layer").map(str::to_string);
                        }
                    } else if let Some(rest) =
                        line.strip_prefix("AETHER_COMPOSITOR_LAYER_CONFIGURE: ")
                    {
                        if field(rest, "namespace") == Some(request.namespace) {
                            if let Some(size) = field(rest, "size").and_then(parse_dimensions) {
                                configured_size = Some(size);
                            }
                        }
                    }
                }
                SurfaceBinding {
                    name: request.name,
                    namespace: request.namespace,
                    expected_layer: request.layer_name,
                    bound,
                    reported_layer,
                    configured_size,
                    expected_size,
                }
            })
            .collect();

        BindingReport { output, surfaces }
    }

    /// Every planned surface bound, configured, on the right layer and at the
    /// negotiated size, on a reported output.
    pub fn is_complete(&self) -> bool {
        self.output.is_some() && self.surfaces.iter().all(SurfaceBinding::is_ok)
    }

    /// Harness-readable reasons, empty when the report is complete.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        match &self.output {
            None => problems.push(
                "no AETHER_COMPOSITOR_OUTPUT marker: is this a compositor log?".to_string(),
            ),
            Some(output) => {
                if output.logical_size.0 <= 0 || output.logical_size.1 <= 0 {
                    problems.push(format!(
                        "output {} reports no logical size",
                        output.name
                    ));
                }
            }
        }
        problems.extend(self.surfaces.iter().filter_map(SurfaceBinding::problem));
        problems
    }

    /// One line per surface plus a verdict, for `--host-check`.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        match &self.output {
            Some(output) => out.push_str(&format!(
                "LAYER_HOST output={} logical={}x{}\n",
                output.name, output.logical_size.0, output.logical_size.1
            )),
            None => out.push_str("LAYER_HOST output=<none>\n"),
        }
        for surface in &self.surfaces {
            out.push_str(&format!(
                "LAYER_SURFACE name={} namespace={} bound={} configured={} expected={}x{} ok={}\n",
                surface.name,
                surface.namespace,
                surface.bound,
                surface
                    .configured_size
                    .map(|size| format!("{}x{}", size.0, size.1))
                    .unwrap_or_else(|| "none".to_string()),
                surface.expected_size.0,
                surface.expected_size.1,
                surface.is_ok()
            ));
        }
        let problems = self.problems();
        if problems.is_empty() {
            out.push_str("AETHER_SHELL_LAYER_HOST_OK\n");
        } else {
            for problem in &problems {
                out.push_str(&format!("AETHER_SHELL_LAYER_HOST_PROBLEM: {problem}\n"));
            }
            out.push_str("AETHER_SHELL_LAYER_HOST_FAILED\n");
        }
        out
    }
}

/// Value of `key=value` inside a marker line (`value` ends at the next space).
/// Shared with [`crate::effects`], which reads the effect/frame markers.
pub(crate) fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find(' ').unwrap_or(rest.len());
    let value = &rest[..end];
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// `1920x1080` → `(1920, 1080)`.
fn parse_dimensions(value: &str) -> Option<(i32, i32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_line() -> &'static str {
        "AETHER_COMPOSITOR_OUTPUT: name=aether-headless-0 mode=1920x1080 scale=1 logical=1920x1080 position=0,0"
    }

    fn complete_log() -> String {
        let mut log = String::from(output_line());
        log.push('\n');
        for request in plan() {
            log.push_str(&format!(
                "AETHER_COMPOSITOR_LAYER_SURFACE: namespace={} layer={} output=aether-headless-0 scale=1\n",
                request.namespace, request.layer_name
            ));
            let size = request.negotiate((1920, 1080));
            log.push_str(&format!(
                "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace={} size={}x{}\n",
                request.namespace, size.0, size.1
            ));
        }
        log
    }

    #[test]
    fn plan_encodes_the_protocol_values_of_every_descriptor() {
        let plan = plan();
        assert_eq!(plan.len(), specs().len());

        let menubar = plan.iter().find(|request| request.name == "menubar").unwrap();
        assert_eq!(menubar.namespace, "aether-shell-menubar");
        assert_eq!(menubar.layer, 2, "Top is 2 in zwlr_layer_shell_v1");
        assert_eq!(menubar.layer_name, "Top");
        assert_eq!(
            menubar.anchors,
            ANCHOR_TOP | ANCHOR_LEFT | ANCHOR_RIGHT,
            "stretched across the top edge"
        );
        assert_eq!((menubar.width, menubar.height), (0, 28));
        assert_eq!(menubar.exclusive_zone, 28, "reserves room for windows");
        assert_eq!(menubar.keyboard_interactivity, KEYBOARD_NONE);

        let launcher = plan.iter().find(|request| request.name == "launcher").unwrap();
        assert_eq!(launcher.layer, 3, "Overlay is 3");
        assert_eq!(launcher.layer_name, "Overlay");
        assert_eq!(launcher.anchors, 0);
        assert_eq!(launcher.keyboard_interactivity, KEYBOARD_EXCLUSIVE);
        let notifications = plan
            .iter()
            .find(|request| request.name == "notifications")
            .unwrap();
        assert_eq!(
            notifications.keyboard_interactivity, KEYBOARD_ON_DEMAND,
            "an always-mapped panel may not take the keyboard from windows"
        );

        let dock = plan.iter().find(|request| request.name == "dock").unwrap();
        assert_eq!(dock.anchors, ANCHOR_BOTTOM);
        assert_eq!(dock.exclusive_zone, 0, "the dock floats over windows");
    }

    #[test]
    fn negotiation_mirrors_the_compositor() {
        let plan = plan();
        let size_of = |name: &str| {
            plan.iter()
                .find(|request| request.name == name)
                .unwrap()
                .negotiate((1920, 1080))
        };
        assert_eq!(size_of("menubar"), (1920, 28));
        assert_eq!(size_of("dock"), (720, 64));
        assert_eq!(size_of("launcher"), (640, 420));
        assert_eq!(
            size_of("notifications"),
            (360, 1052),
            "top+bottom anchors fill the output minus the menu bar margin"
        );
        assert_eq!(size_of("control-center"), (340, 1052));

        // Placement is the layer-shell anchoring rule: stretched axes cover the
        // output, sized axes sit at their edge, margins offset the edge.
        let placement_of = |name: &str| {
            plan.iter()
                .find(|request| request.name == name)
                .unwrap()
                .placement((1920, 1080))
        };
        assert_eq!(placement_of("menubar"), (0, 0));
        assert_eq!(placement_of("dock"), (600, 1016));
        assert_eq!(placement_of("launcher"), (640, 330));
        assert_eq!(placement_of("notifications"), (1560, 28));
        assert_eq!(placement_of("control-center"), (1580, 28));

        // A HiDPI output reports its logical size, so the shell draws logical.
        let hdpi = plan
            .iter()
            .find(|request| request.name == "menubar")
            .unwrap()
            .negotiate((1920, 1200));
        assert_eq!(hdpi, (1920, 28));
    }

    #[test]
    fn a_complete_log_binds_every_surface() {
        let report = BindingReport::scan(&complete_log());
        assert_eq!(
            report.output.as_ref().map(|output| output.logical_size),
            Some((1920, 1080))
        );
        assert_eq!(report.surfaces.len(), plan().len());
        assert!(report.problems().is_empty(), "{:?}", report.problems());
        assert!(report.is_complete());
        let summary = report.summary();
        assert!(summary.contains("LAYER_SURFACE name=menubar"));
        assert!(summary.contains("configured=1920x28 expected=1920x28 ok=true"));
        assert!(summary.ends_with("AETHER_SHELL_LAYER_HOST_OK\n"));
    }

    #[test]
    fn missing_configure_and_wrong_size_are_reported() {
        let mut log = complete_log();
        // Drop the dock's configure event and configure the menubar 12 px tall.
        log = log
            .lines()
            .filter(|line| !(line.contains("namespace=aether-shell-dock") && line.contains("CONFIGURE")))
            .collect::<Vec<_>>()
            .join("\n");
        log = log.replace(
            "namespace=aether-shell-menubar size=1920x28",
            "namespace=aether-shell-menubar size=1920x12",
        );

        let report = BindingReport::scan(&log);
        assert!(!report.is_complete());
        let problems = report.problems();
        assert!(
            problems.iter().any(|problem| problem.contains("dock bound but was never configured")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|problem| problem.contains("menubar configured at 1920x12")),
            "{problems:?}"
        );
        assert!(report.summary().contains("AETHER_SHELL_LAYER_HOST_FAILED"));
    }

    #[test]
    fn an_unbound_surface_and_a_missing_output_are_reported() {
        let log = complete_log().replace(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-launcher layer=Overlay output=aether-headless-0 scale=1\n",
            "",
        );
        let report = BindingReport::scan(&log);
        assert!(!report.is_complete());
        assert!(report
            .problems()
            .iter()
            .any(|problem| problem.contains("launcher was never created")));

        let empty = BindingReport::scan("nothing here\n");
        assert!(!empty.is_complete());
        assert!(empty.output.is_none());
        assert_eq!(empty.surfaces.len(), plan().len());
        assert!(empty
            .problems()
            .iter()
            .any(|problem| problem.contains("no AETHER_COMPOSITOR_OUTPUT marker")));
    }

    #[test]
    fn a_wrong_layer_is_reported() {
        let log = complete_log().replace(
            "namespace=aether-shell-launcher layer=Overlay",
            "namespace=aether-shell-launcher layer=Top",
        );
        let report = BindingReport::scan(&log);
        assert!(!report.is_complete());
        assert!(report
            .problems()
            .iter()
            .any(|problem| problem.contains("launcher is on layer Top")));
    }
}
