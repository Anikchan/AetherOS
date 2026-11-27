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

/// `zwlr_layer_surface_v1.keyboard_interactivity`: who gets the keyboard while
/// the surface is mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    /// Never (menu bar, dock: they are chrome, not a focus target).
    None,
    /// Only while the user asked for it, instead of taking the keyboard away
    /// from the focused window for as long as the surface is mapped. The
    /// notification column and the control centre are always mapped, so they
    /// must not be `Exclusive` or typing in a window would stop working.
    OnDemand,
    /// Always; only the launcher, which is an explicit modal overlay.
    Exclusive,
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
    /// Margin from the anchored edges in logical pixels. The right-hand panels
    /// reserve the menu bar height at the top, and the compositor subtracts it
    /// from the negotiated size.
    pub margin_top: i32,
    /// Whether the surface takes keyboard focus while visible.
    pub keyboard: Keyboard,
}

impl LayerSurfaceSpec {
    pub fn anchored_top(&self) -> bool {
        self.anchor_top
    }
}

/// The accessibility text scale the shell clamps to, as a multiplier.
pub const MIN_TEXT_SCALE: f32 = 1.0;
pub const MAX_TEXT_SCALE: f32 = 2.0;

/// Menu bar height in logical pixels at 100 % text: `Tokens.menubar-height`.
pub const MENUBAR_HEIGHT: i32 = 28;

/// The shell surfaces that become layer surfaces on the device, at 100 % text.
pub fn specs() -> Vec<LayerSurfaceSpec> {
    specs_at(MIN_TEXT_SCALE)
}

/// The same table at an accessibility text scale (F1 SHL-18).
///
/// Panel geometry follows the `.slint` components so a layer surface is never
/// narrower or shorter than the panel it carries (`shell/ui/surfaces.slint`):
/// the menu bar is `28px × text-scale`, the control centre and the notification
/// centre grow by `140px` per extra scale step, and the launcher by `100px` in
/// height. Anything the surface cannot grow with is clipped on the device.
pub fn specs_at(text_scale: f32) -> Vec<LayerSurfaceSpec> {
    let scale = if text_scale.is_finite() {
        text_scale.clamp(MIN_TEXT_SCALE, MAX_TEXT_SCALE)
    } else {
        MIN_TEXT_SCALE
    };
    let scaled = |px: f32| (px * scale).round() as i32;
    let grow = |base: i32, per_step: f32| base + (per_step * (scale - 1.0)).round() as i32;
    let menubar_height = scaled(MENUBAR_HEIGHT as f32);
    let panel_width = |base: i32| grow(base, 140.0);

    vec![
        // The desktop itself. A full-output surface on the Background layer so
        // the first boot shows the design wallpaper instead of whatever the
        // compositor clears to; `exclusive_zone: -1` means "ignore my exclusive
        // zone, cover the whole output" (layer-shell's negative value), and
        // `keyboard: None` means it never takes input.
        LayerSurfaceSpec {
            name: "background",
            namespace: "aether-shell-background",
            layer: Layer::Background,
            anchor_top: true,
            anchor_bottom: true,
            anchor_left: true,
            anchor_right: true,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            margin_top: 0,
            keyboard: Keyboard::None,
        },
        LayerSurfaceSpec {
            name: "menubar",
            namespace: "aether-shell-menubar",
            layer: Layer::Top,
            anchor_top: true,
            anchor_bottom: false,
            anchor_left: true,
            anchor_right: true,
            width: 0,
            height: menubar_height,
            exclusive_zone: menubar_height,
            margin_top: 0,
            keyboard: Keyboard::None,
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
            margin_top: 0,
            keyboard: Keyboard::None,
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
            // `Launcher` height in the .slint file.
            height: grow(420, 100.0),
            exclusive_zone: 0,
            margin_top: 0,
            keyboard: Keyboard::Exclusive,
        },
        LayerSurfaceSpec {
            name: "notifications",
            namespace: "aether-shell-notifications",
            layer: Layer::Top,
            anchor_top: true,
            // A 0 size along an axis is only legal when the surface is anchored
            // to both ends of it; otherwise the compositor raises a protocol
            // error ("height 0 requested without setting top and bottom
            // anchors"). The notification column fills the output height.
            anchor_bottom: true,
            anchor_left: false,
            anchor_right: true,
            width: panel_width(360),
            height: 0,
            exclusive_zone: 0,
            margin_top: menubar_height,
            keyboard: Keyboard::OnDemand,
        },
        LayerSurfaceSpec {
            name: "control-center",
            namespace: "aether-shell-control-center",
            layer: Layer::Top,
            anchor_top: true,
            anchor_bottom: true,
            anchor_left: false,
            anchor_right: true,
            width: panel_width(340),
            height: 0,
            exclusive_zone: 0,
            margin_top: menubar_height,
            keyboard: Keyboard::OnDemand,
        },
    ]
}

/// The shell surfaces the acceptance harness must see bound and configured.
pub fn required_namespaces() -> &'static [&'static str] {
    &[
        "aether-shell-background",
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
    fn the_background_covers_the_output_on_the_background_layer() {
        let specs = specs();
        let background = specs.iter().find(|spec| spec.name == "background").unwrap();
        assert_eq!(background.namespace, "aether-shell-background");
        assert_eq!(background.layer, Layer::Background);
        assert!(
            background.anchor_top
                && background.anchor_bottom
                && background.anchor_left
                && background.anchor_right,
            "anchored to all four edges"
        );
        assert_eq!(
            (background.width, background.height),
            (0, 0),
            "fills the output"
        );
        assert_eq!(
            background.exclusive_zone, -1,
            "covers the output, reserves nothing"
        );
        assert_eq!(background.keyboard, Keyboard::None, "never takes input");
        assert_eq!(
            specs.first().map(|spec| spec.name),
            Some("background"),
            "the background is created first so it sits under the chrome"
        );
        // It is part of the surface contract the harness checks.
        assert!(required_namespaces().contains(&"aether-shell-background"));
    }

    #[test]
    fn panel_geometry_follows_the_text_scale() {
        let specs = specs_at(1.5);
        let find = |name: &str| specs.iter().find(|spec| spec.name == name).unwrap();
        // `Tokens.menubar-height` = 28px × text-scale.
        assert_eq!(find("menubar").height, 42);
        assert_eq!(find("menubar").exclusive_zone, 42);
        // `ControlCenter` = 340 + (ts-1)*140, `NotificationCenter` 360 + …
        assert_eq!(find("control-center").width, 410);
        assert_eq!(find("notifications").width, 430);
        // `Launcher` height = 420 + (ts-1)*100.
        assert_eq!(find("launcher").height, 470);
        // The right-hand panels stay below the (taller) menu bar.
        assert_eq!(find("notifications").margin_top, 42);
        assert_eq!(find("control-center").margin_top, 42);
        // The scale is clamped to 100–200 %, so a bogus value cannot make the
        // shell ask for a surface bigger than the design.
        assert_eq!(
            specs_at(9.0)
                .iter()
                .find(|spec| spec.name == "menubar")
                .unwrap()
                .height,
            56
        );
        assert_eq!(
            specs_at(0.2)
                .iter()
                .find(|spec| spec.name == "menubar")
                .unwrap()
                .height,
            28
        );
        assert_eq!(
            specs_at(f32::NAN)
                .iter()
                .find(|spec| spec.name == "menubar")
                .unwrap()
                .height,
            28
        );
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
        assert_eq!(
            launcher.keyboard,
            Keyboard::Exclusive,
            "the launcher is modal"
        );
        // The panels are always mapped, so taking the keyboard exclusively
        // would stop every window from receiving typing.
        for name in ["notifications", "control-center"] {
            let panel = specs.iter().find(|spec| spec.name == name).unwrap();
            assert_eq!(panel.keyboard, Keyboard::OnDemand, "{name}");
        }
    }
}
