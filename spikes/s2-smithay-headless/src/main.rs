//! Spike S2 (P0-T08): a minimal Smithay compositor that shows one client
//! window.
//!
//! Scope of what is proven (headless, no DRM/KMS, no input, no output):
//! - a Wayland server display with a listening socket is created
//! - a client can connect and create surfaces
//! - xdg-shell toplevels and zwlr-layer-shell surfaces get configured
//! - a client surface commit that attaches an SHM buffer is detected
//!   (surface "mapped"), which is the precondition for any rendering
//!
//! Not proven by this spike: DRM backend init, hardware rendering, output
//! presentation — those need a GPU/DRM device and are out of scope here.
//!
//! Markers printed on stdout:
//! - `S2_SOCKET_READY: wayland-0` — the compositor listens on
//!   `$XDG_RUNTIME_DIR/wayland-0`.
//! - `S2_CLIENT_MAPPED` — a client surface committed with a buffer attached.
//! - `S2_SPIKE_DONE` — clean exit 0 after the first mapped surface plus a
//!   short grace period so the client can finish its own protocol exchange.

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
struct ClientState {
    compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

struct SpikeState {
    compositor: CompositorState,
    shm: ShmState,
    xdg_shell: XdgShellState,
    layer_shell: WlrLayerShellState,
    seat_state: SeatState<Self>,
    /// Set once a client surface committed with a buffer attached.
    mapped: bool,
    /// Set when the first surface is mapped; the compositor exits after this
    /// grace period so the client can finish its protocol exchange.
    shutdown_at: Option<Instant>,
}

impl CompositorHandler for SpikeState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(data) = client.get_data::<ClientState>() {
            return &data.compositor;
        }
        // Every client is inserted with ClientState data, so this is a spike
        // bug, not a protocol condition. Exit cleanly instead of panicking.
        eprintln!("spike-s2-smithay-headless: client without ClientState data");
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
            println!("S2_CLIENT_MAPPED");
        }
    }
}

impl BufferHandler for SpikeState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for SpikeState {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl XdgShellHandler for SpikeState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // The spike accepts whatever size the client picks.
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

impl WlrLayerShellHandler for SpikeState {
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
        println!("S2_LAYER_SURFACE_REQUEST: namespace={namespace}");
        // A 0x0 configure means "the client decides its size".
        let _ = surface.send_configure();
    }
}

// Delegate protocol dispatching to SpikeState.
delegate_compositor!(SpikeState);
delegate_shm!(SpikeState);
delegate_xdg_shell!(SpikeState);
delegate_layer_shell!(SpikeState);

// Required by the xdg-shell dispatch bounds (see smithay's xdg_shell module
// docs). No wl_seat global is created by this spike; the focus types are
// only trait plumbing.
impl SeatHandler for SpikeState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("spike-s2-smithay-headless: FATAL: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    const SOCKET_NAME: &str = "wayland-0";
    const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);
    const GRACE_PERIOD: Duration = Duration::from_secs(3);

    let mut display: Display<SpikeState> =
        Display::new().map_err(|e| format!("Display::new: {e:?}"))?;
    let mut dh = display.handle();

    let mut state = SpikeState {
        compositor: CompositorState::new::<SpikeState>(&dh),
        shm: ShmState::new::<SpikeState>(&dh, vec![]),
        xdg_shell: XdgShellState::new::<SpikeState>(&dh),
        layer_shell: WlrLayerShellState::new::<SpikeState>(&dh),
        seat_state: SeatState::new(),
        mapped: false,
        shutdown_at: None,
    };

    let socket =
        ListeningSocket::bind(SOCKET_NAME).map_err(|e| format!("bind {SOCKET_NAME}: {e:?}"))?;
    println!("S2_SOCKET_READY: {SOCKET_NAME}");

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
            return Err("S2 timed out waiting for a client surface to commit a buffer".to_string());
        }
        thread::sleep(Duration::from_millis(10));
    }

    println!("S2_SPIKE_DONE");
    Ok(())
}
