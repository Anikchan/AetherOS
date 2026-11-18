//! Minimal Wayland client used by Spike S2 (P0-T08).
//!
//! Connects to the compositor in `WAYLAND_DISPLAY`, creates an xdg-shell
//! toplevel, attaches a 100x100 SHM buffer (XRGB8888) and commits it. After
//! the surface has been configured by the compositor and the buffer attached,
//! it prints `CLIENT_WINDOW_MAPPED` and exits 0.

use std::cell::Cell;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

const WIDTH: i32 = 100;
const HEIGHT: i32 = 100;
const STRIDE: i32 = WIDTH * 4;

#[derive(Default)]
struct ClientState {
    // Globals discovered via the registry.
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    // Protocol state.
    configure_serial: Cell<Option<u32>>,
    toplevel_closed: Cell<bool>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for ClientState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_compositor" => state.compositor = Some(registry.bind(name, 1, qh, ())),
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "xdg_wm_base" => state.wm_base = Some(registry.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for ClientState {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: <wl_compositor::WlCompositor as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for ClientState {
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

impl Dispatch<wl_shm::WlShm, ()> for ClientState {
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

impl Dispatch<wl_shm_pool::WlShmPool, ()> for ClientState {
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

impl Dispatch<wl_buffer::WlBuffer, ()> for ClientState {
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

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for ClientState {
    fn event(
        _: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: <xdg_wm_base::XdgWmBase as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_surface::XdgSurface,
        event: <xdg_surface::XdgSurface as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            state.configure_serial.set(Some(serial));
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: <xdg_toplevel::XdgToplevel as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            state.toplevel_closed.set(true);
        }
    }
}

/// A 100x100 XRGB8888 frame in shared memory.
struct ShmFrame {
    fd: OwnedFd,
    ptr: *mut u8,
    len: usize,
}

impl ShmFrame {
    fn new(len: usize) -> Result<Self, String> {
        let raw_fd = unsafe {
            libc::memfd_create(b"aether-spike-client\0".as_ptr().cast::<libc::c_char>(), 0)
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

/// Paint a simple test pattern: white border, blue gradient body (XRGB8888).
fn fill_pattern(frame: &ShmFrame) {
    let map = frame.as_slice_mut();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let offset = ((y * WIDTH + x) * 4) as usize;
            let (b, g, r) = if x < 4 || y < 4 || x >= WIDTH - 4 || y >= HEIGHT - 4 {
                (0xffu8, 0xffu8, 0xffu8)
            } else {
                (
                    ((x * 255) / WIDTH) as u8,
                    0,
                    ((y * 255) / HEIGHT) as u8,
                )
            };
            map[offset] = b;
            map[offset + 1] = g;
            map[offset + 2] = r;
            map[offset + 3] = 0xff;
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("spike-client: FATAL: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let conn =
        Connection::connect_to_env().map_err(|e| format!("connect to WAYLAND_DISPLAY: {e:?}"))?;
    let mut state = ClientState::default();
    let mut queue = conn.new_event_queue::<ClientState>();
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
    let wm_base = state
        .wm_base
        .clone()
        .ok_or("compositor does not advertise xdg_wm_base")?;

    let surface = compositor.create_surface(&qh, ());
    let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    toplevel.set_title("spike-client".to_string());

    // Initial (empty) commit: asks the compositor for the first configure.
    surface.commit();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if state.toplevel_closed.get() {
            return Err("toplevel was closed by the compositor before being mapped".to_string());
        }
        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip: {e:?}"))?;
        if state.configure_serial.get().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for the xdg_surface configure event".to_string());
        }
    }
    let serial = state
        .configure_serial
        .get()
        .ok_or("configure serial missing after configure")?;
    xdg_surface.ack_configure(serial);

    let frame = ShmFrame::new((WIDTH * HEIGHT * 4) as usize)?;
    fill_pattern(&frame);
    let pool_fd = frame
        .fd
        .try_clone()
        .map_err(|e| format!("dup shm fd: {e:?}"))?;
    let pool = shm.create_pool(pool_fd, WIDTH * HEIGHT * 4, &qh, ());
    let buffer = pool.create_buffer(0, WIDTH, HEIGHT, STRIDE, wl_shm::Format::Xrgb8888, &qh, ());

    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    // Two roundtrips so the compositor definitely processed the mapped commit.
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("map roundtrip: {e:?}"))?;
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("map roundtrip: {e:?}"))?;

    println!("CLIENT_WINDOW_MAPPED");
    Ok(())
}
