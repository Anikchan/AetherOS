//! AetherOS compositor (Smithay-based, P3).
//!
//! Phase P3-T01: compositor skeleton. Starts as a headless Wayland compositor
//! (no DRM/KMS, no input) — the same scope proven by spike S2 (P0-T08).
//!
//! Phase P3-T02 extends the skeleton with the window-system surface area the
//! desktop shell and apps need, still headless so it builds and runs on the
//! macOS dev host and the Linux CI runner:
//!
//! - **xdg-shell** toplevels and popups (`xdg_wm_base`).
//! - **layer shell** (`zwlr_layer_shell_v1`) for the menu bar and dock.
//!   Layer surfaces get a concrete configure size derived from their anchors,
//!   margins, requested size and the *logical* size of their output.
//! - **multi-monitor**: two `wl_output` globals in one logical layout
//!   (side by side) plus `zxdg_output_v1` for logical geometry.
//! - **HiDPI**: the two monitors run at different scales (1 and 1.5, the
//!   latter advertised as integer 2 to `wl_output`-only clients). Surfaces are
//!   assigned to their output (`wl_surface.enter`) and told their preferred
//!   `wp_fractional_scale_v1` scale; `wl_surface.set_buffer_scale` is accepted
//!   and recorded.
//!
//! Run: `cargo run -p aether-compositor` (or `aether-compositor` binary).
//! Markers: `AETHER_COMPOSITOR_SOCKET_READY: wayland-0`,
//!          `AETHER_COMPOSITOR_OUTPUT: …`,
//!          `AETHER_COMPOSITOR_OUTPUT_COUNT: 2`,
//!          `AETHER_COMPOSITOR_OUTPUT_BOUND: …`,
//!          `AETHER_COMPOSITOR_XDG_TOPLEVEL: …`,
//!          `AETHER_COMPOSITOR_LAYER_SURFACE: …`,
//!          `AETHER_COMPOSITOR_LAYER_CONFIGURE: …`,
//!          `AETHER_COMPOSITOR_BUFFER_SCALE: …`,
//!          `AETHER_COMPOSITOR_CLIENT_MAPPED`,
//!          `AETHER_COMPOSITOR_DONE`.
//!
//! Env: `AETHER_COMPOSITOR_GRACE_MS` (default 3000) — how long the compositor
//! keeps running after the first mapped surface before exiting. The headless
//! multi-output test raises it so two clients' worth of surfaces can be
//! exercised in one run.
//!
//! The Rust module and the wire protocol share the word "compositor"; see
//! [`AetherCompositor`] (state) vs. `wayland::compositor` (protocol).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::delegate_compositor;
use smithay::delegate_fractional_scale;
#[cfg(target_os = "linux")]
use smithay::delegate_xwayland_shell;
use smithay::delegate_layer_shell;
use smithay::delegate_output;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::input::{SeatHandler, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, ObjectId,
};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, Display, DisplayHandle, Resource,
};
use smithay::utils::{Logical, Serial, Size, Transform};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState,
    SurfaceAttributes,
};
use smithay::wayland::fractional_scale::{
    with_fractional_scale, FractionalScaleHandler, FractionalScaleManagerState,
};
use smithay::wayland::socket::ListeningSocketSource;
#[cfg(target_os = "linux")]
use smithay::wayland::xwayland_shell::{
    XWaylandShellHandler, XWaylandShellState,
};
#[cfg(target_os = "linux")]
use smithay::xwayland::{XWayland, XWaylandEvent};
#[cfg(target_os = "linux")]
use smithay::xwayland::xwm::{X11Surface, X11Wm, XwmHandler, XwmId};
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::shell::wlr_layer::{
    Anchor, Layer, LayerSurface, LayerSurfaceCachedState, WlrLayerShellHandler, WlrLayerShellState,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};

mod wm;
pub use wm::*;

/// Per-client state, as required by the smithay compositor module.
#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// One headless monitor: physical mode, advertised scale and logical position
/// in the global layout.
pub struct HeadlessOutputSpec {
    /// `wl_output.name` / `zxdg_output_v1.name`
    pub name: &'static str,
    /// Physical mode in pixels (width, height).
    pub mode: (i32, i32),
    /// Scale advertised to clients (integer to `wl_output`, fractional to
    /// `wp_fractional_scale_v1` / `zxdg_output_v1`).
    pub scale: Scale,
    /// Logical position of the output's top-left corner.
    pub location: (i32, i32),
    /// Monitor size in millimetres (for `wl_output.geometry`).
    pub physical_mm: (i32, i32),
}

/// The two-monitor HiDPI layout used by the headless (CI) compositor.
///
/// - Output 0: 1920×1080 at scale 1 → logical 1920×1080 at (0, 0).
/// - Output 1: 2880×1800 at scale 1.5 → `wl_output.scale` 2 (integer
///   protocols round up), `zxdg_output_v1` logical size 1920×1200 at (1920, 0).
pub const HEADLESS_OUTPUTS: &[HeadlessOutputSpec] = &[
    HeadlessOutputSpec {
        name: "aether-headless-0",
        mode: (1920, 1080),
        scale: Scale::Integer(1),
        location: (0, 0),
        physical_mm: (344, 194),
    },
    HeadlessOutputSpec {
        name: "aether-headless-1",
        mode: (2880, 1800),
        scale: Scale::Fractional(1.5),
        location: (1920, 0),
        physical_mm: (508, 318),
    },
];

/// A `zwlr_layer_surface_v1` surface known to the compositor.
pub struct LayerSurfaceEntry {
    pub surface: LayerSurface,
    pub namespace: String,
    pub layer: Layer,
    /// Name of the output the surface was created for, if any (None = all).
    pub output: Option<String>,
}

/// Compositor state shared between the Wayland dispatch handlers and the
/// main loop.
pub struct AetherCompositor {
    pub compositor: CompositorState,
    pub shm: ShmState,
    pub xdg_shell: XdgShellState,
    pub layer_shell: WlrLayerShellState,
    pub output_manager: OutputManagerState,
    pub display_handle: smithay::reexports::wayland_server::DisplayHandle,
    pub fractional_scale: FractionalScaleManagerState,
    pub seat_state: SeatState<Self>,
    /// Headless monitors, in layout order (index 0 is primary).
    pub outputs: Vec<Output>,
    pub layer_surfaces: Vec<LayerSurfaceEntry>,
    pub toplevels: Vec<ToplevelSurface>,
    /// xwayland shell global (associates X11 windows with wl_surfaces).
    #[cfg(target_os = "linux")]
    pub xwayland_shell: XWaylandShellState,
    /// Running X11 window manager, if XWayland spawned successfully.
    #[cfg(target_os = "linux")]
    pub xwm: Option<X11Wm>,
    /// Wayland client representing the XWayland server.
    #[cfg(target_os = "linux")]
    pub xwayland_client: Option<smithay::reexports::wayland_server::Client>,
    /// Display number chosen by XWayland.
    #[cfg(target_os = "linux")]
    pub xwayland_display: Option<u32>,
    /// Preferred fractional scale per surface, decided when the surface is
    /// assigned to an output.
    surface_scales: HashMap<ObjectId, f64>,
    /// Surfaces whose buffer scale has already been reported.
    scaled_surfaces: HashSet<ObjectId>,
    /// P3-T04 window-management model: floating/snap/tile + workspaces.
    /// Layer surfaces (shell chrome) are deliberately *not* managed here.
    pub wm: WorkspaceManager,
    /// Monotonic id source for managed toplevels.
    next_window_id: u64,
    pub mapped: bool,
    pub shutdown_at: Option<Instant>,
    pub deadline: Option<Instant>,
    pub timed_out: bool,
}

impl AetherCompositor {
    pub fn new(dh: &DisplayHandle) -> Self {
        let mut state = Self {
            compositor: CompositorState::new::<Self>(dh),
            shm: ShmState::new::<Self>(dh, vec![]),
            xdg_shell: XdgShellState::new::<Self>(dh),
            layer_shell: WlrLayerShellState::new::<Self>(dh),
            // xdg-output piggybacks on every wl_output global created below,
            // so it must exist first.
            output_manager: OutputManagerState::new_with_xdg_output::<Self>(dh),
            display_handle: dh.clone(),
            fractional_scale: FractionalScaleManagerState::new::<Self>(dh),
            seat_state: SeatState::new(),
            outputs: Vec::new(),
            layer_surfaces: Vec::new(),
            toplevels: Vec::new(),
            #[cfg(target_os = "linux")]
            xwayland_shell: XWaylandShellState::new::<Self>(dh),
            #[cfg(target_os = "linux")]
            xwm: None,
            #[cfg(target_os = "linux")]
            xwayland_client: None,
            #[cfg(target_os = "linux")]
            xwayland_display: None,
            surface_scales: HashMap::new(),
            scaled_surfaces: HashSet::new(),
            // Replaced with the primary output's logical size once outputs are
            // added below.
            wm: WorkspaceManager::new(0, 0),
            next_window_id: 0,
            mapped: false,
            shutdown_at: None,
            deadline: None,
            timed_out: false,
        };

        for spec in HEADLESS_OUTPUTS {
            state.add_headless_output(dh, spec);
        }
        // Seed the WM with the primary output's logical size so snap/tile
        // geometry matches what clients are configured at.
        let (ow, oh) = state
            .outputs
            .first()
            .map(|output| state.logical_size(output))
            .map(|size| (size.w, size.h))
            .unwrap_or((1920, 1080));
        state.wm = WorkspaceManager::new(ow, oh);
        println!("AETHER_COMPOSITOR_OUTPUT_COUNT: {}", state.outputs.len());
        state
    }

    /// Create one headless monitor: advertise the `wl_output` global and
    /// configure mode, scale and logical position.
    fn add_headless_output(&mut self, dh: &DisplayHandle, spec: &HeadlessOutputSpec) {
        let output = Output::new(
            spec.name.to_string(),
            PhysicalProperties {
                size: spec.physical_mm.into(),
                subpixel: Subpixel::Unknown,
                make: "AetherOS".into(),
                model: "Headless".into(),
            },
        );
        // The global stays alive for the compositor's lifetime.
        let _global = output.create_global::<Self>(dh);

        let mode = Mode {
            size: spec.mode.into(),
            refresh: 60_000,
        };
        output.set_preferred(mode);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(spec.scale),
            Some(spec.location.into()),
        );

        let fractional = spec.scale.fractional_scale();
        let logical_w = (spec.mode.0 as f64 / fractional).round() as i32;
        let logical_h = (spec.mode.1 as f64 / fractional).round() as i32;
        println!(
            "AETHER_COMPOSITOR_OUTPUT: name={} mode={}x{} scale={} logical={}x{} position={},{}",
            spec.name,
            spec.mode.0,
            spec.mode.1,
            fractional,
            logical_w,
            logical_h,
            spec.location.0,
            spec.location.1
        );
        self.outputs.push(output);
    }

    /// Logical (post-scale) size of an output.
    fn logical_size(&self, output: &Output) -> Size<i32, Logical> {
        let (w, h) = output
            .current_mode()
            .map(|mode| (mode.size.w, mode.size.h))
            .unwrap_or((0, 0));
        let scale = output.current_scale().fractional_scale();
        Size::from((
            (w as f64 / scale).round() as i32,
            (h as f64 / scale).round() as i32,
        ))
    }

    fn output_by_name(&self, name: &str) -> Option<&Output> {
        self.outputs.iter().find(|output| output.name() == name)
    }

    /// Assign a surface to an output: send `wl_surface.enter` and remember the
    /// output's fractional scale for `wp_fractional_scale_v1`.
    fn assign_surface(&mut self, surface: &WlSurface, output: &Output) {
        output.enter(surface);
        let scale = output.current_scale().fractional_scale();
        self.surface_scales.insert(surface.id(), scale);
    }

    /// Send a configure with a concrete size to the layer surface behind
    /// `surface` once its anchors and requested size are known.
    fn configure_layer_surface(&mut self, surface: &WlSurface) {
        let id = surface.id();
        let Some(entry) = self
            .layer_surfaces
            .iter()
            .find(|entry| entry.surface.wl_surface().id() == id)
        else {
            return;
        };

        let output = match entry.output.as_deref() {
            Some(name) => self.output_by_name(name).or_else(|| self.outputs.first()),
            None => self.outputs.first(),
        };
        let Some(output) = output else {
            return;
        };

        let output_size = self.logical_size(output);
        let cached = with_states(surface, |states| {
            states
                .cached_state
                .get::<LayerSurfaceCachedState>()
                .current()
                .clone()
        });
        let size = layer_surface_size(&cached, output_size);

        let layer_surface = entry.surface.clone();
        let namespace = entry.namespace.clone();
        layer_surface.with_pending_state(|state| {
            state.size = Some(size);
        });
        if layer_surface.send_pending_configure().is_some() {
            println!(
                "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace={namespace} size={}x{}",
                size.w, size.h
            );
        }
    }

    /// Push a managed geometry to the toplevel at `index` (the WM `WindowId`
    /// equals the toplevel index). No-op if the index is unknown.
    fn configure_toplevel(&mut self, index: usize, rect: Rect) {
        let Some(toplevel) = self.toplevels.get(index) else {
            return;
        };
        let size = Size::from((rect.w.max(1), rect.h.max(1)));
        toplevel.with_pending_state(|state| {
            state.size = Some(size);
        });
        if toplevel.send_pending_configure().is_some() {
            println!(
                "AETHER_COMPOSITOR_WM_CONFIGURE: id={index} size={}x{}",
                rect.w, rect.h
            );
        }
    }

    /// P3-T04: snap the toplevel at `index` to an output edge.
    pub fn snap_toplevel(&mut self, index: usize, edge: SnapEdge) {
        let id = WindowId(index as u64);
        self.wm.set_mode(id, WindowMode::Snapped(edge));
        let Some(rect) = self.wm.windows.get(&id).map(|window| window.rect) else {
            return;
        };
        println!(
            "AETHER_COMPOSITOR_WM_SNAP: id={index} edge={edge:?} rect={},{},{}x{}",
            rect.x, rect.y, rect.w, rect.h
        );
        self.configure_toplevel(index, rect);
    }

    /// P3-T04: assign the toplevel at `index` to a workspace.
    pub fn move_toplevel_to_workspace(&mut self, index: usize, workspace: WorkspaceId) {
        let id = WindowId(index as u64);
        if self.wm.move_to_workspace(id, workspace) {
            println!(
                "AETHER_COMPOSITOR_WM_MOVE: id={index} workspace={}",
                workspace.0
            );
        }
    }

    /// P3-T04: tile every window on the active workspace into a grid.
    pub fn tile_active_workspace(&mut self) {
        let workspace = self.wm.active;
        self.wm.tile_all(workspace);
        let rects: Vec<(usize, Rect)> = self
            .wm
            .visible_on(workspace)
            .into_iter()
            .filter_map(|id| {
                self.wm
                    .windows
                    .get(&id)
                    .map(|window| (id.0 as usize, window.rect))
            })
            .collect();
        for (index, rect) in rects {
            println!(
                "AETHER_COMPOSITOR_WM_TILE: id={index} rect={},{},{}x{}",
                rect.x, rect.y, rect.w, rect.h
            );
            self.configure_toplevel(index, rect);
        }
    }

    /// P3-T04: switch the active workspace (creating it if unknown).
    pub fn activate_workspace(&mut self, workspace: WorkspaceId) {
        if !self.wm.workspaces.contains(&workspace) {
            self.wm.workspaces.push(workspace);
        }
        self.wm.active = workspace;
        println!(
            "AETHER_COMPOSITOR_WM_WORKSPACE: active={}",
            workspace.0
        );
    }
}

/// Layer-shell size negotiation (the subset the shell needs): anchored edges
/// stretch to the output, an explicit non-zero size wins, otherwise the
/// surface fills the output along that axis. Margins are subtracted.
fn layer_surface_size(
    cached: &LayerSurfaceCachedState,
    output: Size<i32, Logical>,
) -> Size<i32, Logical> {
    let anchor = cached.anchor;
    let margins = cached.margin;

    let width = if anchor.contains(Anchor::LEFT) && anchor.contains(Anchor::RIGHT) {
        output.w - margins.left - margins.right
    } else if cached.size.w > 0 {
        cached.size.w
    } else {
        output.w - margins.left - margins.right
    };

    let height = if anchor.contains(Anchor::TOP) && anchor.contains(Anchor::BOTTOM) {
        output.h - margins.top - margins.bottom
    } else if cached.size.h > 0 {
        cached.size.h
    } else {
        output.h - margins.top - margins.bottom
    };

    Size::from((width.max(1), height.max(1)))
}

// Delegate protocol dispatching.
delegate_compositor!(AetherCompositor);
delegate_shm!(AetherCompositor);
delegate_xdg_shell!(AetherCompositor);
delegate_layer_shell!(AetherCompositor);
delegate_output!(AetherCompositor);
delegate_fractional_scale!(AetherCompositor);
#[cfg(target_os = "linux")]
delegate_xwayland_shell!(AetherCompositor);

impl CompositorHandler for AetherCompositor {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(data) = client.get_data::<ClientState>() {
            return &data.compositor;
        }
        // XWayland is inserted with smithay's own `XWaylandClientData` (it is
        // not a `ClientState` client). Its `wl_surface`/`wl_region` requests
        // still need the compositor client state; without this branch the
        // compositor would `exit(1)` the moment XWayland creates its root
        // surface, so XWayland never reaches `XWaylandEvent::Ready`.
        #[cfg(target_os = "linux")]
        if let Some(data) = client.get_data::<smithay::xwayland::XWaylandClientData>() {
            return &data.compositor_state;
        }
        eprintln!("aether-compositor: client without ClientState data");
        std::process::exit(1);
    }

    fn commit(&mut self, surface: &WlSurface) {
        let (buffer_scale, has_buffer) = with_states(surface, |states| {
            let mut attrs = states.cached_state.get::<SurfaceAttributes>();
            let current = attrs.current();
            (
                current.buffer_scale,
                matches!(&current.buffer, Some(BufferAssignment::NewBuffer(_))),
            )
        });

        // HiDPI evidence: a client drawing for a scaled output.
        if buffer_scale > 1 && self.scaled_surfaces.insert(surface.id()) {
            println!("AETHER_COMPOSITOR_BUFFER_SCALE: surface_buffer_scale={buffer_scale}");
        }

        if !self.mapped && has_buffer {
            self.mapped = true;
            println!("AETHER_COMPOSITOR_CLIENT_MAPPED");
        }

        self.configure_layer_surface(surface);
    }
}

impl BufferHandler for AetherCompositor {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for AetherCompositor {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl OutputHandler for AetherCompositor {
    fn output_bound(&mut self, output: Output, _wl_output: WlOutput) {
        println!("AETHER_COMPOSITOR_OUTPUT_BOUND: name={}", output.name());
    }
}

impl FractionalScaleHandler for AetherCompositor {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let Some(scale) = self.surface_scales.get(&surface.id()).copied() else {
            return;
        };
        with_states(&surface, |states| {
            with_fractional_scale(states, |fractional| {
                fractional.set_preferred_scale(scale);
            });
        });
    }
}

impl XdgShellHandler for AetherCompositor {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let output = self.outputs.first().cloned();
        let output_name = output
            .as_ref()
            .map(Output::name)
            .unwrap_or_else(|| "none".to_string());
        if let Some(output) = output {
            self.assign_surface(&wl_surface, &output);
        }
        println!("AETHER_COMPOSITOR_XDG_TOPLEVEL: output={output_name}");
        self.toplevels.push(surface.clone());

        // P3-T04: register the toplevel with the window manager as a floating
        // window on the active workspace and configure it to its managed
        // geometry. The window id equals the toplevel's index so the WM model
        // and `self.toplevels` stay in lock-step.
        let index = self.toplevels.len() - 1;
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        let (ow, oh) = self
            .outputs
            .first()
            .map(|output| self.logical_size(output))
            .map(|size| (size.w.max(1), size.h.max(1)))
            .unwrap_or((1920, 1080));
        self.wm.output_size = (ow, oh);
        self.wm.add_window(id, None);
        // Default floating geometry: a centered window at 60% of the output.
        let win_w = (ow * 3 / 5).max(1);
        let win_h = (oh * 3 / 5).max(1);
        let rect = Rect {
            x: (ow - win_w) / 2,
            y: (oh - win_h) / 2,
            w: win_w,
            h: win_h,
        };
        if let Some(window) = self.wm.windows.get_mut(&id) {
            window.rect = rect;
        }
        println!(
            "AETHER_COMPOSITOR_WM_WINDOW: id={} workspace={} output={} mode=floating rect={},{},{}x{}",
            id.0, self.wm.active.0, output_name, rect.x, rect.y, rect.w, rect.h
        );
        self.configure_toplevel(index, rect);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

impl WlrLayerShellHandler for AetherCompositor {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        output: Option<WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        // Resolve the wl_output the client asked for; fall back to the primary
        // output so every layer surface has a scale and target size.
        let target = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.outputs.first().cloned());
        let (output_name, scale) = match &target {
            Some(target) => {
                self.assign_surface(surface.wl_surface(), target);
                (
                    target.name(),
                    Some(target.current_scale().fractional_scale()),
                )
            }
            None => ("all".to_string(), None),
        };
        println!(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace={namespace} layer={layer:?} output={output_name} scale={}",
            scale.unwrap_or(1.0)
        );
        self.layer_surfaces.push(LayerSurfaceEntry {
            surface: surface.clone(),
            namespace,
            layer,
            output: target.as_ref().map(Output::name),
        });
        // Layer surfaces are shell chrome (menu bar, dock, wallpapers) and are
        // intentionally excluded from the P3-T04 window manager: they are never
        // tiled/snapped and have no workspace. Their geometry is negotiated by
        // `configure_layer_surface` instead.
        // Immediate initial configure; a sized configure follows on the first
        // commit once anchors and the requested size are known.
        let _ = surface.send_configure();
    }
}

impl SeatHandler for AetherCompositor {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

#[cfg(target_os = "linux")]
impl XWaylandShellHandler for AetherCompositor {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell
    }

    fn surface_associated(
        &mut self,
        _xwm: XwmId,
        _wl_surface: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        _surface: X11Surface,
    ) {
    }
}

#[cfg(target_os = "linux")]
impl XwmHandler for AetherCompositor {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("xwm not initialised")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let _ = window.set_mapped(true);
        let _ = window.configure(None);
        println!("AETHER_COMPOSITOR_XWAYLAND_MAPPED");
    }

    fn map_window_notify(&mut self, _xwm: XwmId, _window: X11Surface) {
        println!("AETHER_COMPOSITOR_XWAYLAND_MAPPED");
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {
        println!("AETHER_COMPOSITOR_XWAYLAND_MAPPED");
    }

    fn unmapped_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn destroyed_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<smithay::xwayland::xwm::Reorder>,
    ) {
        use smithay::utils::{Logical, Rectangle, Size};
        let rect = Rectangle::<i32, Logical>::new(
            (x.unwrap_or(0), y.unwrap_or(0)).into(),
            Size::from((w.unwrap_or(0) as i32, h.unwrap_or(0) as i32)),
        );
        if w.is_some() || h.is_some() {
            let _ = window.configure(Some(rect));
        } else {
            let _ = window.configure(None);
        }
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: smithay::utils::Rectangle<i32, smithay::utils::Logical>,
        _above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
    }

    fn resize_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32, _resize_edge: smithay::xwayland::xwm::ResizeEdge) {}

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}
}

/// Run the compositor. Returns Ok(()) on clean shutdown.
pub fn run() -> Result<(), String> {
    const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);

    let grace = Duration::from_millis(
        std::env::var("AETHER_COMPOSITOR_GRACE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(3000),
    );

    let display: Display<AetherCompositor> =
        Display::new().map_err(|e| format!("Display::new: {e:?}"))?;
    let dh = display.handle();

    let mut state = AetherCompositor::new(&dh);
    state.deadline = Some(Instant::now() + OVERALL_TIMEOUT);

    // XWayland (and smithay's prepare_x11_sockets) bind Unix sockets under
    // /tmp/.X11-unix; that directory is not always present on minimal CI images
    // (e.g. ubuntu-latest with only `xwayland`/`x11-apps` installed), which
    // would make XWayland::spawn fail silently. Make sure it exists.
    let _ = std::fs::create_dir_all("/tmp/.X11-unix");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions("/tmp/.X11-unix", std::fs::Permissions::from_mode(0o1777));
    }

    // Spawn XWayland if available (Linux CI). Failure is non-fatal: the
    // compositor keeps running as a pure Wayland compositor, but log the
    // reason so a missing binary / socket dir is diagnosable in CI.
    //
    // stdout/stderr are inherited (not `Stdio::null()`) so XWayland's own
    // `-verbose` diagnostics reach the compositor's log/console and a startup
    // failure is visible in CI instead of silent.
    #[cfg(target_os = "linux")]
    let xwayland_pair = match XWayland::spawn(
        &dh,
        None,
        std::iter::empty::<(String, String)>(),
        false,
        std::process::Stdio::inherit(),
        std::process::Stdio::inherit(),
        |_ud| {},
    ) {
        Ok(pair) => Some(pair),
        Err(err) => {
            eprintln!("aether-compositor: XWayland spawn failed: {err}");
            None
        }
    };
    #[cfg(target_os = "linux")]
    if let Some((_, client)) = &xwayland_pair {
        state.xwayland_client = Some(client.clone());
    }

    let mut event_loop: smithay::reexports::calloop::EventLoop<'_, AetherCompositor> =
        smithay::reexports::calloop::EventLoop::try_new()
            .map_err(|e| format!("EventLoop::try_new: {e:?}"))?;
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    let loop_handle = event_loop.handle();
    let signal = event_loop.get_signal();

    // Listening socket for Wayland clients.
    let socket_source = ListeningSocketSource::with_name("wayland-0")
        .map_err(|e| format!("bind wayland-0: {e:?}"))?;
    println!("AETHER_COMPOSITOR_SOCKET_READY: wayland-0");
    event_loop
        .handle()
        .insert_source(socket_source, |stream, _, state| {
            if let Err(err) = state
                .display_handle
                .insert_client(stream, Arc::new(ClientState::default()))
            {
                eprintln!("aether-compositor: insert_client: {err:?}");
            }
        })
        .map_err(|e| format!("insert socket source: {e:?}"))?;

    // XWayland event source.
    #[cfg(target_os = "linux")]
    if let Some((xwayland, _client)) = xwayland_pair {
        let lh = loop_handle.clone();
        event_loop
            .handle()
            .insert_source(xwayland, move |event, _, state| {
                match event {
                    XWaylandEvent::Ready {
                        x11_socket,
                        display_number,
                    } => {
                        state.xwayland_display = Some(display_number);
                        println!("AETHER_COMPOSITOR_XWAYLAND_DISPLAY: {display_number}");
                        if let Some(client) = state.xwayland_client.clone() {
                            match X11Wm::start_wm(lh.clone(), x11_socket, client) {
                                Ok(wm) => {
                                    state.xwm = Some(wm);
                                }
                                Err(err) => {
                                    eprintln!("aether-compositor: X11Wm start failed: {err:?}");
                                }
                            }
                        }
                    }
                    XWaylandEvent::Error => {
                        eprintln!("aether-compositor: XWayland error");
                    }
                }
            })
            .map_err(|e| format!("insert xwayland source: {e:?}"))?;
    }

    // We need display accessible inside the event loop callback. Store it in a
    // thread-local-ish slot via a wrapper? Instead, keep display in a mutable
    // captured variable and use a raw pointer. Simpler: make AetherCompositor
    // carry the display? Display<AetherCompositor> is not self-referential by
    // value (backend only), so we can store it in the state.
    // -> We instead handle display via a global static pointer set before run.
    // (See below.)

    // To avoid self-referential complexity, store display in an Arc<Mutex>?
    // Display is not Sync. Instead, move display into state.display field.
    // Add field at runtime by... AetherCompositor has no display field.
    // We'll use a separate captured mutable and pass through callback closure.
    // calloop callback only receives &mut Data, so capture display in an
    // UnsafeCell via a box leaked for the duration of the run.
    let display_box = Box::new(display);
    let display_ptr = Box::into_raw(display_box);
    // Save dh already. We'll use display_ptr in closure.

    // Insert display pointer into state? Store as usize in a global.
    DISPLAY_PTR.store(display_ptr as usize, std::sync::atomic::Ordering::SeqCst);

    let grace_for_closure = grace;
    let result = event_loop.run(
        Some(Duration::from_millis(10)),
        &mut state,
        move |state| {
            // dispatch wayland clients
            let display = unsafe { &mut *(DISPLAY_PTR.load(std::sync::atomic::Ordering::SeqCst) as *mut Display<AetherCompositor>) };
            let _ = display.dispatch_clients(state);
            let _ = display.flush_clients();

            if state.mapped && state.shutdown_at.is_none() {
                state.shutdown_at = Some(Instant::now() + grace_for_closure);
            }
            if let Some(shutdown_at) = state.shutdown_at {
                if Instant::now() >= shutdown_at {
                    signal.stop();
                }
            }
            if let Some(deadline) = state.deadline {
                if Instant::now() >= deadline {
                    state.timed_out = true;
                    signal.stop();
                }
            }
        },
    );

    // reclaim display
    let _ = unsafe { Box::from_raw(display_ptr) };

    // Need state.timed_out field. Add to struct? We'll check via a side channel:
    // We stored timed_out? We didn't add field. Add quickly? Instead, track via
    // a global.
    // For now, report timeout if result ok but no mapped? Keep simple.
    let _ = result;
    // We need to propagate timed_out. We added field? Let's add field `timed_out: bool` to AetherCompositor.
    if state.timed_out {
        return Err("compositor timed out waiting for a client surface to commit a buffer".to_string());
    }

    println!("AETHER_COMPOSITOR_DONE");
    Ok(())
}

// globals for display pointer and timed_out
static DISPLAY_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
