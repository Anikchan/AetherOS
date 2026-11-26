//! Layer-shell surface descriptors for the on-device host (P3-T02, S3).
//!
//! The compositor (S2) advertises `zwlr_layer_shell_v1`; the shell surfaces
//! that were previously composed into one full-screen window (menu bar, dock,
//! launcher, notifications, control centre) are split out into anchored layer
//! surfaces on the device. This module is the single source of truth for their
//! namespace, layer, anchors, requested size and exclusive zone, so the host
//! that creates them and the Slint render sizes cannot drift apart.
//!
//! [`crate::host`] turns these descriptors into the exact protocol requests and
//! verifies the compositor's configure markers (`aether-shell --host-check`);
//! this module stays data-only so both sides read the same table.
//!
//! The host itself (a Wayland client over the software renderer, proven by
//! spike S1) lands with the on-device image; until then these descriptors drive
//! [`crate::host`] and the acceptance harness.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Background,
    Bottom,
    Top,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerSurfaceSpec {
    /// Stable name used in logs and tests.
    pub name: &'static str,
    /// `zwlr_layer_surface_v1` namespace (matched by the compositor log).
    pub namespace: &'static str,
    pub layer: Layer,
    pub anchor_top: bool,
    pub anchor_bottom: bool,
    pub anchor_left: bool,
    pub anchor_right: bool,
    /// Requested width in logical pixels (`0` means "let the compositor fill").
    pub width: i32,
    /// Requested height in logical pixels (`0` means "let the compositor fill").
    pub height: i32,
    /// Exclusive zone in logical pixels (0 for non-exclusive surfaces).
    pub exclusive_zone: i32,
    /// Whether the surface takes keyboard focus while visible.
    pub keyboard_interactivity: bool,
}

impl LayerSurfaceSpec {
    pub fn anchored_top(&self) -> bool {
        self.anchor_top
    }
}

/// The shell surfaces that become layer surfaces on the device.
pub fn specs() -> Vec<LayerSurfaceSpec> {
    vec![
        LayerSurfaceSpec {
            name: "menubar",
            namespace: "aether-shell-menubar",
            layer: Layer::Top,
            anchor_top: true,
            anchor_bottom: false,
            anchor_left: true,
            anchor_right: true,
            width: 0,
            height: 28,
            exclusive_zone: 28,
            keyboard_interactivity: false,
        },
        LayerSurfaceSpec {
            name: "dock",
            namespace: "aether-shell-dock",
            layer: Layer::Top,
            anchor_top: false,
            anchor_bottom: true,
            anchor_left: false,
            anchor_right: false,
            width: 720,
            height: 64,
            exclusive_zone: 0,
            keyboard_interactivity: false,
        },
        LayerSurfaceSpec {
            name: "launcher",
            namespace: "aether-shell-launcher",
            layer: Layer::Overlay,
            anchor_top: false,
            anchor_bottom: false,
            anchor_left: false,
            anchor_right: false,
            width: 640,
            height: 480,
            exclusive_zone: 0,
            keyboard_interactivity: true,
        },
        LayerSurfaceSpec {
            name: "notifications",
            namespace: "aether-shell-notifications",
            layer: Layer::Top,
            anchor_top: true,
            anchor_bottom: false,
            anchor_left: false,
            anchor_right: true,
            width: 360,
            height: 0,
            exclusive_zone: 0,
            keyboard_interactivity: true,
        },
        LayerSurfaceSpec {
            name: "control-center",
            namespace: "aether-shell-control-center",
            layer: Layer::Top,
            anchor_top: true,
            anchor_bottom: false,
            anchor_left: false,
            anchor_right: true,
            width: 340,
            height: 0,
            exclusive_zone: 0,
            keyboard_interactivity: true,
        },
    ]
}

/// The shell surfaces the acceptance harness must see bound and configured.
pub fn required_namespaces() -> &'static [&'static str] {
    &[
        "aether-shell-menubar",
        "aether-shell-dock",
        "aether-shell-launcher",
        "aether-shell-notifications",
        "aether-shell-control-center",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menubar_and_dock_keep_their_token_sizes() {
        let specs = specs();
        let menubar = specs.iter().find(|spec| spec.name == "menubar").unwrap();
        let dock = specs.iter().find(|spec| spec.name == "dock").unwrap();
        assert_eq!(menubar.height, 28);
        assert_eq!(menubar.exclusive_zone, 28);
        assert!(menubar.anchor_top && menubar.anchor_left && menubar.anchor_right);
        assert_eq!(dock.height, 64);
        assert_eq!(dock.width, 720);
    }

    #[test]
    fn required_surfaces_are_described() {
        let specs = specs();
        for namespace in required_namespaces() {
            assert!(
                specs.iter().any(|spec| spec.namespace == *namespace),
                "missing layer surface {namespace}"
            );
        }
        let launcher = specs.iter().find(|spec| spec.name == "launcher").unwrap();
        assert!(launcher.keyboard_interactivity);
    }
}
