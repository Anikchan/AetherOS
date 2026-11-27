//! Pointer path: `wl_pointer` coordinates → the composed Slint scene → a click.
//!
//! The layer host receives pointer events in the coordinates of one surface,
//! while the shell's scene is the whole composed window. [`PointerRouter`] adds
//! the surface's placement and dispatches the standard Slint pointer events, so
//! the handlers the design system already wires (the dock tiles here) run
//! exactly as they do in the composed desktop.
//!
//! The compositor has no headless pointer injection yet
//! (`AETHER_COMPOSITOR_INJECT_POINTER`), so this test drives the router with a
//! synthetic dock click at the surface-local coordinates the compositor would
//! deliver; the socket leg (binding `wl_pointer` and reporting the seat as
//! `keyboard+pointer`) is asserted in `compositor_handshake.rs`.
//!
//! One test function: the software-renderer platform is process-wide.

use std::path::PathBuf;

use aether_shell::config::SettingsStore;
use aether_shell::host::plan;
use aether_shell::paint::LayerPainter;
use aether_shell::pointer::{PointerEvent, PointerKind, PointerRouter, BTN_LEFT};
use aether_shell::services::{Battery, Services};
use aether_shell::{PowerMode, ShellController, ShellRoot};

#[derive(Default)]
struct QuietServices;

impl Services for QuietServices {
    fn set_wifi(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_bluetooth(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_brightness(&mut self, _brightness: f32) -> Result<(), String> {
        Ok(())
    }
    fn set_power_mode(&mut self, _mode: PowerMode) -> Result<(), String> {
        Ok(())
    }
    fn battery(&mut self) -> Option<Battery> {
        Some(Battery {
            percent: 84,
            charging: false,
        })
    }
    fn lock_session(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn temp_config_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-pointer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Dock surface-local position of the first app tile's centre.
///
/// The dock surface is 720 logical px wide and the panel is centred in it
/// (`Math.max(96px, 16px + 4 × 56px + 3 × 10px)` = 270 px with four pinned
/// apps), so tile 0 spans local x 233..289 and y 8..56.
const DOCK_TILE_0: (f64, f64) = (261.0, 32.0);

#[test]
fn a_socket_style_click_on_the_dock_activates_its_tile() {
    aether_ui::render::init().expect("software renderer platform");
    let root = temp_config_root();
    let ui = ShellRoot::new().expect("ShellRoot");
    let controller = ShellController::new(
        ui,
        Box::new(QuietServices),
        SettingsStore::new(root.clone()),
    );
    // Paints once, which lays the scene out so the items can be hit-tested.
    let _painter = LayerPainter::new(controller.ui(), (1920, 1080), 1).expect("layer-host scene");

    let dock = plan()
        .into_iter()
        .find(|request| request.name == "dock")
        .expect("the dock is in the plan");
    let placement = dock.placement((1920, 1080));
    assert_eq!(
        placement,
        (600, 1016),
        "the dock surface sits on the bottom edge"
    );

    let mut router = PointerRouter::new();
    let before = controller.state().active_app.clone();
    assert_eq!(before, "AetherOS", "nothing is active before the click");

    // enter → motion → press → release, exactly what `wl_pointer` delivers.
    let enter = PointerEvent {
        surface: "aether-shell-dock",
        local: DOCK_TILE_0,
        placement,
        kind: PointerKind::Enter,
    };
    let line = router.route(controller.ui(), enter);
    assert_eq!(
        line,
        "AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=none \
         state=enter window=861,1048"
    );
    for kind in [PointerKind::Motion, PointerKind::Press(BTN_LEFT)] {
        router.route(
            controller.ui(),
            PointerEvent {
                surface: "aether-shell-dock",
                local: DOCK_TILE_0,
                placement,
                kind,
            },
        );
    }
    let line = router.route(
        controller.ui(),
        PointerEvent {
            surface: "aether-shell-dock",
            local: DOCK_TILE_0,
            placement,
            kind: PointerKind::Release(BTN_LEFT),
        },
    );
    assert!(
        line.starts_with(
            "AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=272 state=release"
        ),
        "{line}"
    );

    // Tile 0 is the first pinned dock app; the click goes through the same
    // `dock-chosen` handler the keyboard's Enter uses.
    let first = controller.state().dock_apps()[0].clone();
    assert_eq!(first.id, "aether-files");
    assert_eq!(
        controller.state().active_app,
        first.name,
        "the dock click activated its tile"
    );
    assert_eq!(
        router.current().map(|(surface, _)| surface),
        Some("aether-shell-dock")
    );

    // The transparent part of the dock surface is not a tile: a click there
    // changes nothing (the surface is wider than the panel).
    let outside = PointerEvent {
        surface: "aether-shell-dock",
        local: (20.0, 32.0),
        placement,
        kind: PointerKind::Press(BTN_LEFT),
    };
    router.route(controller.ui(), outside);
    router.route(
        controller.ui(),
        PointerEvent {
            kind: PointerKind::Release(BTN_LEFT),
            ..outside
        },
    );
    assert_eq!(
        controller.state().active_app,
        first.name,
        "a click outside the dock panel does not activate anything"
    );

    // Leaving the surface clears the router's hover state.
    router.route(
        controller.ui(),
        PointerEvent {
            surface: "aether-shell-dock",
            local: (20.0, 32.0),
            placement,
            kind: PointerKind::Leave,
        },
    );
    assert!(router.current().is_none());

    let _ = std::fs::remove_dir_all(root);
}
