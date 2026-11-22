//! AetherOS compositor (Smithay-based, P3).
//!
//! Phase P3-T01: compositor skeleton. Starts as a headless Wayland compositor
//! (no DRM/KMS, no input, no output) — the same scope proven by spike S2
//! (P0-T08). DRM/KMS, input and output presentation are added in P2/P3
//! once hardware bring-up lands; the headless backend keeps the compositor
//! developable and testable on the macOS dev host and in CI.
//!
//! Run: `cargo run -p aether-compositor` (or `aether-compositor` binary).
//! Markers: `AETHER_COMPOSITOR_SOCKET_READY: wayland-0`,
//!          `AETHER_COMPOSITOR_CLIENT_MAPPED`,
//!          `AETHER_COMPOSITOR_DONE`.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use smithay::delegate_compositor;
use smithay::delegate_layer_shell;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::input::{SeatHandler, SeatState};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Display, ListeningSocket};
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState,
    SurfaceAttributes,
};
use smithay::wayland::shell::wlr_layer::{
    Layer, LayerSurface, WlrLayerShellHandler, WlrLayerShellState,
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

/// Compositor state shared between the Wayland dispatch handlers and the
/// main loop.
pub struct AetherCompositor {
    pub compositor: CompositorState,
    pub shm: ShmState,
    pub xdg_shell: XdgShellState,
    pub layer_shell: WlrLayerShellState,
    pub seat_state: SeatState<Self>,
    pub mapped: bool,
    pub shutdown_at: Option<Instant>,
}

impl AetherCompositor {
    pub fn new(dh: &smithay::reexports::wayland_server::DisplayHandle) -> Self {
        Self {
            compositor: CompositorState::new::<Self>(dh),
            shm: ShmState::new::<Self>(dh, vec![]),
            xdg_shell: XdgShellState::new::<Self>(dh),
            layer_shell: WlrLayerShellState::new::<Self>(dh),
            seat_state: SeatState::new(),
            mapped: false,
            shutdown_at: None,
        }
    }
}

// Delegate protocol dispatching.
delegate_compositor!(AetherCompositor);
delegate_shm!(AetherCompositor);
delegate_xdg_shell!(AetherCompositor);
delegate_layer_shell!(AetherCompositor);

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
        if self.mapped {
            return;
        }
        let has_buffer = with_states(surface, |states| {
            let mut attrs = states.cached_state.get::<SurfaceAttributes>();
            matches!(
                &attrs.current().buffer,
                Some(BufferAssignment::NewBuffer(_))
            )
        });
        if has_buffer {
            self.mapped = true;
            println!("AETHER_COMPOSITOR_CLIENT_MAPPED");
        }
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

impl XdgShellHandler for AetherCompositor {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
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
        _output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        println!("AETHER_COMPOSITOR_LAYER_SURFACE: namespace={namespace}");
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
    const GRACE_PERIOD: Duration = Duration::from_secs(3);

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
            state.shutdown_at = Some(Instant::now() + GRACE_PERIOD);
        }
        if let Some(shutdown_at) = state.shutdown_at {
            if Instant::now() >= shutdown_at {
                break;
            }
        }
        if Instant::now() >= deadline {
            return Err("compositor timed out waiting for a client surface to commit a buffer".to_string());
        }
        thread::sleep(Duration::from_millis(10));
    }

    println!("AETHER_COMPOSITOR_DONE");
    Ok(())
}