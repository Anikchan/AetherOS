//! Spike S1 (P0-T07): prove that Slint can draw into a `zwlr_layer_shell_v1`
//! layer surface and request a background blur region.
//!
//! winit has no wlr-layer-shell support (rust-windowing/winit#2582) and Slint's
//! winit backend has none either (slint-ui/slint#1765), so this spike uses
//! Slint's custom platform API: `slint::platform::{Platform, WindowAdapter}`
//! with the software renderer, and talks to the Wayland compositor itself via
//! the pure-Rust `wayland-client` crate.
//!
//! Markers printed on stdout:
//! - `S1_LAYERSHELL_SURFACE_OK`  — layer surface configured by the compositor
//!   and the first rendered Slint frame was attached + committed.
//! - `S1_BLUR_REQUEST_SENT`      — `org_kde_kwin_blur_manager` was bound and a
//!   blur region covering the surface was requested.
//! - `S1_BLUR_UNAVAILABLE: ...`  — the compositor does not advertise the KDE
//!   blur protocol; recorded as a spike finding instead of failing.
//! - `S1_LAYERSHELL_UNAVAILABLE: ...` — the compositor does not advertise
//!   `zwlr_layer_shell_v1`; recorded as a spike finding instead of failing.

use std::cell::{Cell, RefCell};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, WindowAdapter};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

/// KDE blur protocol, hand-inlined (not packaged in wayland-protocols).
mod blur {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/org-kde-blur.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/org-kde-blur.xml");
}
use blur::org_kde_kwin_blur::OrgKdeKwinBlur;
use blur::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager;

slint::slint! {
    export component SpikeUi inherits Window {
        background: #101828;
        Text {
            text: "S1";
            color: #00ff88;
            font-size: 96px;
            vertical-alignment: center;
            horizontal-alignment: center;
        }
        Rectangle {
            x: 8px;
            y: 8px;
            width: 48px;
            height: 48px;
            background: #ff5252;
        }
    }
}

/// Client-side state shared between the Wayland dispatch handlers and the
/// main loop. All fields touched from event callbacks use `Cell` because
/// dispatching happens on the same thread.
#[derive(Default)]
struct S1State {
    // Globals discovered via the registry.
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<ZwlrLayerShellV1>,
    blur_manager: Option<OrgKdeKwinBlurManager>,
    // zwlr_layer_surface_v1 events.
    configure_serial: Cell<Option<u32>>,
    configure_size: Cell<(u32, u32)>,
    layer_closed: Cell<bool>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for S1State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version: _,
        } = event
        {
            // Bind every interesting global at version 1: all requests the
            // spike uses exist since version 1, and binding at 1 never
            // exceeds what the compositor advertised.
            match interface.as_str() {
                "wl_compositor" => state.compositor = Some(registry.bind(name, 1, qh, ())),
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "zwlr_layer_shell_v1" => state.layer_shell = Some(registry.bind(name, 1, qh, ())),
                "org_kde_kwin_blur_manager" => {
                    state.blur_manager = Some(registry.bind(name, 1, qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for S1State {
    fn event(
        _: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: <wl_compositor::WlCompositor as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: <wl_surface::WlSurface as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: <wl_shm::WlShm as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: <wl_shm_pool::WlShmPool as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: <wl_buffer::WlBuffer as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_region::WlRegion, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: <wl_region::WlRegion as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: <ZwlrLayerShellV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for S1State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                state.configure_serial.set(Some(serial));
                state.configure_size.set((width, height));
            }
            zwlr_layer_surface_v1::Event::Closed => state.layer_closed.set(true),
            _ => {}
        }
    }
}

impl Dispatch<OrgKdeKwinBlurManager, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinBlurManager,
        _: <OrgKdeKwinBlurManager as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinBlur, ()> for S1State {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinBlur,
        _: <OrgKdeKwinBlur as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Slint platform: hands out a `MinimalSoftwareWindow` backed by the software
/// renderer. The spike keeps the adapter so the main loop can render frames.
#[derive(Clone, Default)]
struct SpikePlatform {
    window: Rc<RefCell<Option<Rc<MinimalSoftwareWindow>>>>,
}

impl Platform for SpikePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let adapter = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        *self.window.borrow_mut() = Some(adapter.clone());
        Ok(adapter)
    }
}

/// A CPU-side frame backed by a shared memory file, used as the wl_shm buffer
/// contents for the layer surface (XRGB8888).
struct ShmFrame {
    fd: OwnedFd,
    ptr: *mut u8,
    len: usize,
}

impl ShmFrame {
    fn new(len: usize) -> Result<Self, String> {
        let raw_fd = unsafe {
            libc::memfd_create(b"aether-spike-s1\0".as_ptr().cast::<libc::c_char>(), 0)
        };
        if raw_fd < 0 {
            return Err(format!(
                "memfd_create failed: {}",
                io::Error::last_os_error()
            ));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let truncated = unsafe { libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) };
        if truncated != 0 {
            return Err(format!("ftruncate failed: {}", io::Error::last_os_error()));
        }
        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            return Err(format!("mmap failed: {}", io::Error::last_os_error()));
        }
        Ok(Self {
            fd,
            ptr: mapped.cast::<u8>(),
            len,
        })
    }

    fn as_slice_mut(&self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for ShmFrame {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr.cast::<libc::c_void>(), self.len) };
    }
}

fn rgb565_to_rgb888(value: u16) -> (u8, u8, u8) {
    let r5 = ((value >> 11) & 0x1f) as u8;
    let g6 = ((value >> 5) & 0x3f) as u8;
    let b5 = (value & 0x1f) as u8;
    (
        (r5 << 3) | (r5 >> 2),
        (g6 << 2) | (g6 >> 4),
        (b5 << 3) | (b5 >> 2),
    )
}

/// Render the Slint scene (if dirty) and copy it into the SHM mapping as
/// XRGB8888.
fn render_frame(window: &Rc<MinimalSoftwareWindow>, frame: &ShmFrame, width: u32, height: u32) {
    let w = width as usize;
    let h = height as usize;
    let mut pixels = vec![Rgb565Pixel::default(); w * h];
    let _ = window.draw_if_needed(|renderer| {
        let _ = renderer.render(&mut pixels, w);
    });
    let target = frame.as_slice_mut();
    for (index, pixel) in pixels.iter().enumerate() {
        let (r, g, b) = rgb565_to_rgb888(pixel.0);
        let offset = index * 4;
        target[offset] = b;
        target[offset + 1] = g;
        target[offset + 2] = r;
        target[offset + 3] = 0xff;
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("spike-s1-slint-layershell: FATAL: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    const DEFAULT_WIDTH: u32 = 320;
    const DEFAULT_HEIGHT: u32 = 240;

    let conn =
        Connection::connect_to_env().map_err(|e| format!("connect to WAYLAND_DISPLAY: {e:?}"))?;
    let mut state = S1State::default();
    let mut queue = conn.new_event_queue::<S1State>();
    let qh = queue.handle();
    let display = conn.display();
    let _registry = display.get_registry(&qh, ());
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("registry roundtrip: {e:?}"))?;

    let compositor = state
        .compositor
        .clone()
        .ok_or("compositor does not advertise wl_compositor")?;
    let shm = state
        .shm
        .clone()
        .ok_or("compositor does not advertise wl_shm")?;
    if state.layer_shell.is_none() {
        println!(
            "S1_LAYERSHELL_UNAVAILABLE: compositor does not advertise zwlr_layer_shell_v1; \
             Slint cannot place a layer-shell surface here"
        );
        return Ok(());
    }
    let layer_shell = state
        .layer_shell
        .clone()
        .ok_or("zwlr_layer_shell_v1 disappeared between roundtrips")?;

    // Create the layer surface and give the compositor the initial commit, so
    // it sends us a configure event.
    let surface = compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Background,
        "spike-s1-slint".to_string(),
        &qh,
        (),
    );
    layer_surface.set_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    surface.commit();

    // Shared-memory buffer the rendered frame is committed from.
    let stride = (DEFAULT_WIDTH * 4) as i32;
    let frame = ShmFrame::new((DEFAULT_WIDTH * DEFAULT_HEIGHT * 4) as usize)?;
    let pool_fd = frame
        .fd
        .try_clone()
        .map_err(|e| format!("dup shm fd: {e:?}"))?;
    let pool = shm.create_pool(pool_fd, (DEFAULT_WIDTH * DEFAULT_HEIGHT * 4) as i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        DEFAULT_WIDTH as i32,
        DEFAULT_HEIGHT as i32,
        stride,
        wl_shm::Format::Xrgb8888,
        &qh,
        (),
    );

    // Slint: custom platform first, then the component.
    let platform = SpikePlatform::default();
    slint::platform::set_platform(Box::new(platform.clone()))
        .map_err(|e| format!("set_platform: {e:?}"))?;
    let ui = SpikeUi::new().map_err(|e| format!("create SpikeUi: {e:?}"))?;
    let window = platform
        .window
        .borrow()
        .clone()
        .ok_or("platform did not create a window adapter")?;
    window.set_size(slint::PhysicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT));
    ui.show().map_err(|e| format!("show SpikeUi: {e:?}"))?;

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut frame_sent = false;
    let mut blur_done = false;
    while Instant::now() < deadline {
        if state.layer_closed.get() {
            return Err("layer surface was closed by the compositor".to_string());
        }
        if !frame_sent {
            queue
                .roundtrip(&mut state)
                .map_err(|e| format!("roundtrip: {e:?}"))?;
            if let Some(serial) = state.configure_serial.get() {
                let (configured_w, configured_h) = state.configure_size.get();
                let width = if configured_w > 0 { configured_w } else { DEFAULT_WIDTH };
                let height = if configured_h > 0 { configured_h } else { DEFAULT_HEIGHT };
                window.set_size(slint::PhysicalSize::new(width, height));
                layer_surface.ack_configure(serial);
                render_frame(&window, &frame, width, height);
                surface.attach(Some(&buffer), 0, 0);
                surface.commit();
                println!("S1_LAYERSHELL_SURFACE_OK");
                frame_sent = true;
            }
        } else if !blur_done {
            // Make sure the compositor processed the first frame before the
            // blur request is evaluated against it.
            queue
                .roundtrip(&mut state)
                .map_err(|e| format!("roundtrip: {e:?}"))?;
            match &state.blur_manager {
                Some(manager) => {
                    let region = compositor.create_region(&qh, ());
                    region.add(
                        0,
                        0,
                        DEFAULT_WIDTH as i32,
                        DEFAULT_HEIGHT as i32,
                    );
                    let blur = manager.create(&surface, &qh, ());
                    blur.set_region(Some(&region));
                    blur.commit();
                    queue
                        .roundtrip(&mut state)
                        .map_err(|e| format!("blur roundtrip: {e:?}"))?;
                    println!("S1_BLUR_REQUEST_SENT");
                }
                None => println!(
                    "S1_BLUR_UNAVAILABLE: compositor does not advertise org_kde_kwin_blur_manager \
                     (KDE blur protocol); background blur cannot be requested through \
                     Slint/wlr-layer-shell on this compositor"
                ),
            }
            blur_done = true;
        } else {
            break;
        }
    }
    if !frame_sent {
        return Err(
            "S1 timed out: no zwlr_layer_surface_v1 configure event received within 30s".to_string(),
        );
    }
    println!("S1_SPIKE_DONE");
    Ok(())
}
