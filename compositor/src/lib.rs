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
use std::thread;
use std::time::{Duration, Instant};

use smithay::delegate_compositor;
use smithay::delegate_fractional_scale;
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
    Client, Display, DisplayHandle, ListeningSocket, Resource,
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
use smithay::wayland::output::{OutputHandler, OutputManagerState};
use smithay::wayland::shell::wlr_layer::{
    Anchor, Layer, LayerSurface, LayerSurfaceCachedState, WlrLayerShellHandler, WlrLayerShellState,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};

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
    pub fractional_scale: FractionalScaleManagerState,
    pub seat_state: SeatState<Self>,
    /// Headless monitors, in layout order (index 0 is primary).
    pub outputs: Vec<Output>,
    pub layer_surfaces: Vec<LayerSurfaceEntry>,
    pub toplevels: Vec<ToplevelSurface>,
    /// Preferred fractional scale per surface, decided when the surface is
    /// assigned to an output.
    surface_scales: HashMap<ObjectId, f64>,
    /// Surfaces whose buffer scale has already been reported.
    scaled_surfaces: HashSet<ObjectId>,
    pub mapped: bool,
    pub shutdown_at: Option<Instant>,
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
            fractional_scale: FractionalScaleManagerState::new::<Self>(dh),
            seat_state: SeatState::new(),
            outputs: Vec::new(),
            layer_surfaces: Vec::new(),
            toplevels: Vec::new(),
            surface_scales: HashMap::new(),
            scaled_surfaces: HashSet::new(),
            mapped: false,
            shutdown_at: None,
        };

        for spec in HEADLESS_OUTPUTS {
            state.add_headless_output(dh, spec);
        }
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

impl CompositorHandler for AetherCompositor {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(data) = client.get_data::<ClientState>() {
            return &data.compositor;
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
        let _ = surface.send_configure();
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

/// Run the compositor. Returns Ok(()) on clean shutdown.
pub fn run() -> Result<(), String> {
    const SOCKET_NAME: &str = "wayland-0";
    const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);

    let grace = Duration::from_millis(
        std::env::var("AETHER_COMPOSITOR_GRACE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(3000),
    );

    let mut display: Display<AetherCompositor> =
        Display::new().map_err(|e| format!("Display::new: {e:?}"))?;
    let mut dh = display.handle();

    let mut state = AetherCompositor::new(&dh);

    let socket =
        ListeningSocket::bind(SOCKET_NAME).map_err(|e| format!("bind {SOCKET_NAME}: {e:?}"))?;
    println!("AETHER_COMPOSITOR_SOCKET_READY: {SOCKET_NAME}");

    let deadline = Instant::now() + OVERALL_TIMEOUT;
    loop {
        display
            .dispatch_clients(&mut state)
            .map_err(|e| format!("dispatch_clients: {e}"))?;
        match socket.accept() {
            Ok(Some(stream)) => {
                dh.insert_client(stream, Arc::new(ClientState::default()))
                    .map_err(|e| format!("insert_client: {e:?}"))?;
            }
            Ok(None) => {}
            Err(err) => return Err(format!("accept: {err}")),
        }
        display
            .flush_clients()
            .map_err(|e| format!("flush_clients: {e}"))?;

        if state.mapped && state.shutdown_at.is_none() {
            state.shutdown_at = Some(Instant::now() + grace);
        }
        if let Some(shutdown_at) = state.shutdown_at {
            if Instant::now() >= shutdown_at {
                break;
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "compositor timed out waiting for a client surface to commit a buffer".to_string(),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    println!("AETHER_COMPOSITOR_DONE");
    Ok(())
}
