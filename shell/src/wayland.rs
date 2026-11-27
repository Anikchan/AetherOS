//! The real layer-shell host: a Wayland client that creates the shell
//! surfaces (P3-T02 → P4).
//!
//! [`crate::host`] is the contract (what each surface asks for, and how the
//! compositor's markers are read back). [`crate::layers`] is the table. This
//! module is the socket: it connects to `$WAYLAND_DISPLAY`, binds
//! `wl_compositor`, `wl_shm` and `zwlr_layer_shell_v1` (plus `wl_seat` when the
//! compositor advertises one), creates every planned surface in plan order with
//! its exact layer, anchors, size, exclusive zone and keyboard interactivity,
//! acks the resulting configure events, attaches an SHM buffer of the
//! configured size (so the compositor maps the surface), and reports what
//! happened.
//!
//! `aether-shell --connect` prints the report; `shell/tests/
//! compositor_handshake.rs` boots the real compositor headless and asserts it,
//! so the shipped host is exercised end to end with no fake log in between.
//!
//! The protocol version is the pure-Rust `wayland-client` backend: no
//! `libwayland` on the development host, and the Linux image links the same
//! crate.

use std::fs;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::host::{self, HostRequest};
use crate::keys::KeyInput;

/// A committed SHM buffer plus the file and pool that keep it valid.
struct ShmBuffer {
    _file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
}

/// One layer surface the host created.
struct Slot {
    request: HostRequest,
    wl_surface: wl_surface::WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    /// `(serial, width, height)` of every configure, in arrival order.
    configures: Vec<(u32, i32, i32)>,
    acked: usize,
    /// Latest configure that carried a real size.
    size: Option<(i32, i32)>,
    /// Size of the buffer attached (and acked) so far.
    buffer_size: Option<(i32, i32)>,
}

/// Client-side protocol state.
#[derive(Default)]
struct HostState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    slots: Vec<Slot>,
    buffers: Vec<ShmBuffer>,
    /// Wire keycodes and the modifier mask in force, as `wl_keyboard` sends them.
    keys: Vec<(u32, u32)>,
    modifiers: u32,
    keyboard: bool,
    /// Indices of surfaces the compositor closed.
    closed: Vec<usize>,
}

/// What the host did for one surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceReport {
    pub name: &'static str,
    pub namespace: &'static str,
    pub layer_name: &'static str,
    pub anchors: u32,
    pub exclusive_zone: i32,
    pub keyboard_interactivity: u32,
    pub size: Option<(i32, i32)>,
    pub configures: usize,
    pub buffer: Option<(i32, i32)>,
}

impl SurfaceReport {
    fn problem(&self) -> Option<String> {
        match (self.size, self.buffer) {
            (None, _) => Some(format!(
                "{} was created but never configured (no sized zwlr_layer_surface_v1.configure)",
                self.name
            )),
            (Some(_), None) => Some(format!(
                "{} was configured but never drawn (no SHM buffer attached)",
                self.name
            )),
            _ => None,
        }
    }
}

/// The result of one `--connect` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostReport {
    pub display: String,
    pub outputs: usize,
    pub keyboard: bool,
    pub surfaces: Vec<SurfaceReport>,
}

impl HostReport {
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.outputs == 0 {
            problems.push("the compositor advertises no wl_output".to_string());
        }
        problems.extend(self.surfaces.iter().filter_map(SurfaceReport::problem));
        problems
    }

    pub fn is_ok(&self) -> bool {
        !self.surfaces.is_empty() && self.problems().is_empty()
    }

    /// One marker per surface plus the verdict, for `--connect` and the harness.
    pub fn summary(&self) -> String {
        let mut out = format!(
            "AETHER_SHELL_LAYER_HOST: display={} outputs={} seat={}\n",
            self.display,
            self.outputs,
            if self.keyboard { "keyboard" } else { "none" }
        );
        for surface in &self.surfaces {
            out.push_str(&format!(
                "AETHER_SHELL_LAYER_CREATED: name={} namespace={} layer={} anchors={} size={}x{} exclusive_zone={} keyboard={}\n",
                surface.name,
                surface.namespace,
                surface.layer_name,
                surface.anchors,
                surface.size.map(|size| size.0).unwrap_or(0),
                surface.size.map(|size| size.1).unwrap_or(0),
                surface.exclusive_zone,
                surface.keyboard_interactivity
            ));
            out.push_str(&format!(
                "AETHER_SHELL_LAYER_CONFIGURE: name={} namespace={} configured={} serials={}\n",
                surface.name,
                surface.namespace,
                surface
                    .size
                    .map(|size| format!("{}x{}", size.0, size.1))
                    .unwrap_or_else(|| "none".to_string()),
                surface.configures
            ));
        }
        let configured = self
            .surfaces
            .iter()
            .filter(|surface| surface.size.is_some())
            .count();
        let drawn = self
            .surfaces
            .iter()
            .filter(|surface| surface.buffer.is_some())
            .count();
        out.push_str(&format!(
            "AETHER_SHELL_LAYER_HOST_READY: surfaces={} configured={} drawn={}\n",
            self.surfaces.len(),
            configured,
            drawn
        ));
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

/// A live connection to the compositor.
pub struct LayerHost {
    conn: Connection,
    queue: EventQueue<HostState>,
    state: HostState,
    runtime_dir: PathBuf,
    display: String,
    outputs: usize,
}

impl LayerHost {
    /// Connect to `$WAYLAND_DISPLAY` inside `$XDG_RUNTIME_DIR` and bind the
    /// globals the shell needs. Errors are harness-readable strings.
    pub fn connect() -> Result<LayerHost, String> {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .map_err(|_| {
                "XDG_RUNTIME_DIR is not set: no Wayland socket to connect to".to_string()
            })?;
        let conn = Connection::connect_to_env()
            .map_err(|error| format!("cannot connect to {display}: {error}"))?;
        let (globals, queue) = registry_queue_init::<HostState>(&conn)
            .map_err(|error| format!("wl_registry initialisation failed: {error:?}"))?;
        let qh = queue.handle();

        let mut state = HostState::default();
        state.compositor = Some(
            globals
                .bind::<wl_compositor::WlCompositor, _, _>(&qh, 4..=6, ())
                .map_err(|error| format!("the compositor has no wl_compositor: {error:?}"))?,
        );
        state.shm = Some(
            globals
                .bind::<wl_shm::WlShm, _, _>(&qh, 1..=1, ())
                .map_err(|error| format!("the compositor has no wl_shm: {error:?}"))?,
        );
        state.layer_shell = Some(
            globals
                .bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(&qh, 1..=4, ())
                .map_err(|error| {
                    format!("the compositor advertises no zwlr_layer_shell_v1: {error:?}")
                })?,
        );
        // The DRM backend creates a seat; the headless backend has no input
        // devices, so the keyboard path is reported as absent instead of
        // failing the handshake.
        if let Ok(seat) = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=7, ()) {
            seat.get_keyboard(&qh, ());
            state.keyboard = true;
        }
        let outputs = globals.contents().with_list(|list| {
            list.iter()
                .filter(|global| global.interface == "wl_output")
                .count()
        });

        Ok(LayerHost {
            conn,
            queue,
            state,
            runtime_dir,
            display,
            outputs,
        })
    }

    /// Create every planned layer surface, in plan order, and draw it.
    pub fn create_surfaces(&mut self) -> Result<HostReport, String> {
        let compositor = self.state.compositor.clone().expect("bound in connect");
        let shm = self.state.shm.clone().expect("bound in connect");
        let layer_shell = self.state.layer_shell.clone().expect("bound in connect");
        let qh = self.queue.handle();

        for (index, request) in host::plan().into_iter().enumerate() {
            let wl_surface = compositor.create_surface(&qh, ());
            let layer_surface = layer_shell.get_layer_surface(
                &wl_surface,
                None,
                match request.layer {
                    0 => zwlr_layer_shell_v1::Layer::Background,
                    1 => zwlr_layer_shell_v1::Layer::Bottom,
                    2 => zwlr_layer_shell_v1::Layer::Top,
                    _ => zwlr_layer_shell_v1::Layer::Overlay,
                },
                request.namespace.to_string(),
                &qh,
                index,
            );
            layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::from_bits_truncate(
                request.anchors,
            ));
            layer_surface.set_size(request.width.max(0) as u32, request.height.max(0) as u32);
            layer_surface.set_exclusive_zone(request.exclusive_zone);
            layer_surface.set_keyboard_interactivity(match request.keyboard_interactivity {
                host::KEYBOARD_EXCLUSIVE => zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
                host::KEYBOARD_ON_DEMAND => zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
                _ => zwlr_layer_surface_v1::KeyboardInteractivity::None,
            });
            wl_surface.set_buffer_scale(1);
            wl_surface.commit();
            self.state.slots.push(Slot {
                request,
                wl_surface,
                layer_surface,
                configures: Vec::new(),
                acked: 0,
                size: None,
                buffer_size: None,
            });
        }

        // The first commit makes the compositor send a sized configure after
        // the initial unsized one from `new_layer_surface`.
        self.roundtrip()?;
        self.roundtrip()?;
        self.ack_configures();

        // Draw every surface at the size it was configured to.
        let palette: [u32; 5] = [0x1c1f26, 0x14171d, 0x24272f, 0x1a1d24, 0x1e2128];
        for index in 0..self.state.slots.len() {
            let Some(size) = self.state.slots[index].size else {
                continue;
            };
            let (width, height) = size;
            let path = self.runtime_dir.join(format!(
                "aether-shell-shm-{}-{width}x{height}.raw",
                self.state.slots[index].request.name
            ));
            let buffer = create_buffer(
                &shm,
                &qh,
                &path,
                width,
                height,
                palette[index % palette.len()],
            )?;
            self.state.slots[index]
                .wl_surface
                .attach(Some(&buffer.buffer), 0, 0);
            self.state.slots[index].wl_surface.commit();
            self.state.slots[index].buffer_size = Some((width, height));
            self.state.buffers.push(buffer);
        }
        self.roundtrip()?;
        // The attach commit produces one more sized configure; ack it so the
        // compositor's pending state never stays unacked.
        self.ack_configures();

        let surfaces = self
            .state
            .slots
            .iter()
            .map(|slot| SurfaceReport {
                name: slot.request.name,
                namespace: slot.request.namespace,
                layer_name: slot.request.layer_name,
                anchors: slot.request.anchors,
                exclusive_zone: slot.request.exclusive_zone,
                keyboard_interactivity: slot.request.keyboard_interactivity,
                size: slot.size,
                configures: slot.configures.len(),
                buffer: slot.buffer_size,
            })
            .collect::<Vec<_>>();
        Ok(HostReport {
            display: self.display.clone(),
            outputs: self.outputs,
            keyboard: self.state.keyboard,
            surfaces,
        })
    }

    /// Ack every configure received so far (in arrival order).
    fn ack_configures(&mut self) {
        for index in 0..self.state.slots.len() {
            let pending =
                self.state.slots[index].configures[self.state.slots[index].acked..].to_vec();
            for (serial, width, height) in pending {
                let slot = &mut self.state.slots[index];
                slot.layer_surface.ack_configure(serial);
                slot.acked += 1;
                if width > 0 && height > 0 {
                    slot.size = Some((width, height));
                }
            }
        }
    }

    fn roundtrip(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut self.state)
            .map(|_| ())
            .map_err(|error| format!("Wayland roundtrip failed: {error:?}"))
    }

    /// Key presses the compositor delivered, as evdev + modifier state.
    pub fn take_keys(&mut self) -> Vec<KeyInput> {
        std::mem::take(&mut self.state.keys)
            .into_iter()
            .map(|(key, modifiers)| KeyInput::from_wire(key, modifiers))
            .collect()
    }

    /// Dispatch events for at most `duration` (surfaces stay mapped), handing
    /// every key press the compositor delivered to `on_key`.
    pub fn hold(&mut self, duration: Duration, mut on_key: impl FnMut(KeyInput)) {
        let deadline = Instant::now() + duration;
        loop {
            let _ = self.conn.flush();
            let _ = self.roundtrip();
            for input in self.take_keys() {
                on_key(input);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20).min(left));
        }
    }

    /// True when the host is connected to `WAYLAND_DISPLAY` in
    /// `XDG_RUNTIME_DIR`.
    pub fn has_socket() -> bool {
        let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else {
            return false;
        };
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
        PathBuf::from(dir).join(display).exists()
    }
}

/// Create and fill a buffer with one colour. A real file (not `memfd_create`)
/// keeps the host portable to the macOS dev host.
fn create_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<HostState>,
    path: &std::path::Path,
    width: i32,
    height: i32,
    colour: u32,
) -> Result<ShmBuffer, String> {
    use std::io::Write;

    let len = (width as usize) * (height as usize) * 4;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    let mut pixels = Vec::with_capacity(len);
    let b = (colour & 0xff) as u8;
    let g = ((colour >> 8) & 0xff) as u8;
    let r = ((colour >> 16) & 0xff) as u8;
    for _ in 0..(width as usize * height as usize) {
        pixels.extend_from_slice(&[b, g, r, 0xff]);
    }
    file.write_all(&pixels)
        .map_err(|error| format!("cannot fill {}: {error}", path.display()))?;
    file.flush()
        .map_err(|error| format!("cannot flush {}: {error}", path.display()))?;

    let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width,
        height,
        width * 4,
        wl_shm::Format::Xrgb8888,
        qh,
        (),
    );
    Ok(ShmBuffer {
        _file: file,
        _pool: pool,
        buffer,
    })
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The global list is stable for the life of a session; runtime
        // hotplug is handled by the compositor, which re-configures us.
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, usize> for HostState {
    fn event(
        state: &mut Self,
        proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                if let Some(slot) = state.slots.get_mut(*index) {
                    slot.configures.push((serial, width as i32, height as i32));
                }
            }
            zwlr_layer_surface_v1::Event::Closed => state.closed.push(*index),
            _ => {}
        }
        let _ = proxy;
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                // Key repeat and releases are not shell actions.
                if matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed)) {
                    state.keys.push((key, state.modifiers));
                }
            }
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                state.modifiers = mods_depressed;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for HostState {
    fn event(
        state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            if let WEnum::Value(capabilities) = capabilities {
                state.keyboard = capabilities.contains(wl_seat::Capability::Keyboard);
            }
            let _ = qh;
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: <wl_compositor::WlCompositor as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: <wl_surface::WlSurface as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm::WlShm,
        _event: <wl_shm::WlShm as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_shm_pool::WlShmPool,
        _event: <wl_shm_pool::WlShmPool as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        _event: <wl_buffer::WlBuffer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for HostState {
    fn event(
        _state: &mut Self,
        _proxy: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _event: <zwlr_layer_shell_v1::ZwlrLayerShellV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}
