//! P3-T07 headless acceptance test — direct scanout.
//!
//! Boots the real `aether-compositor` binary on a private `XDG_RUNTIME_DIR` and
//! checks the scanout decision:
//!
//! 1. one floating toplevel → `mode=composite reason=not_fullscreen`;
//! 2. the client asks for fullscreen and commits a full-output **opaque**
//!    (XRGB) buffer → `mode=direct window=0` and
//!    `AETHER_COMPOSITOR_SCANOUT_FRAME` (composition is bypassed);
//! 3. a Top-layer shell surface appears over it → back to
//!    `mode=composite reason=overlay_present`;
//! 4. a second scenario with two visible windows keeps composition
//!    (`reason=multiple_windows`).
//!
//! Needs no DRM, input, or GPU, so it runs in CI and on the macOS dev host.
//! The hardware part (real KMS plane flipping the client buffer) stays gated on
//! the Dell.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

#[derive(Default)]
struct ClientState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    /// Serials of the xdg_surface configures, in arrival order.
    configures: Vec<u32>,
    layer_configure: Option<(u32, u32, u32)>,
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
        let wl_registry::Event::Global {
            name,
            interface,
            version: _,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_compositor" => {
                state.compositor = Some(registry.bind(name, 4, qh, ()));
            }
            "wl_shm" => {
                state.shm = Some(registry.bind(name, 1, qh, ()));
            }
            "xdg_wm_base" => {
                state.wm_base = Some(registry.bind(name, 1, qh, ()));
            }
            "zwlr_layer_shell_v1" => {
                state.layer_shell = Some(registry.bind(name, 3, qh, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            state.configures.push(serial);
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            state.layer_configure = Some((serial, width, height));
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for ClientState {
    fn event(
        _state: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: <xdg_wm_base::XdgWmBase as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

macro_rules! ignore_events {
    ($iface:ty, $udata:ty) => {
        impl Dispatch<$iface, $udata> for ClientState {
            fn event(
                _state: &mut Self,
                _proxy: &$iface,
                _event: <$iface as wayland_client::Proxy>::Event,
                _data: &$udata,
                _conn: &Connection,
                _qh: &QueueHandle<Self>,
            ) {
            }
        }
    };
}

ignore_events!(wl_compositor::WlCompositor, ());
ignore_events!(wl_surface::WlSurface, ());
ignore_events!(wl_shm::WlShm, ());
ignore_events!(wl_shm_pool::WlShmPool, ());
ignore_events!(wl_buffer::WlBuffer, ());
ignore_events!(xdg_toplevel::XdgToplevel, ());
ignore_events!(zwlr_layer_shell_v1::ZwlrLayerShellV1, ());

#[test]
fn scanout_decisions() {
    // Sequential on purpose: both scenarios set the process-global
    // XDG_RUNTIME_DIR before connecting, so they must not run in parallel.
    fullscreen_bypasses_composition();
    two_windows_keep_composition();
}

fn fullscreen_bypasses_composition() {
    let compositor = CompositorProcess::start("scanout", &[]);
    let runtime_dir = compositor.runtime_dir.clone();
    std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
    std::env::set_var("WAYLAND_DISPLAY", "wayland-0");

    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let compositor_proxy = state.compositor.clone().expect("wl_compositor");
    let shm = state.shm.clone().expect("wl_shm");
    let wm_base = state.wm_base.clone().expect("xdg_wm_base");
    let layer_shell = state.layer_shell.clone().expect("zwlr_layer_shell_v1");

    // --- One floating window: composition -----------------------------------
    let surface = compositor_proxy.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("p3-t07-scanout".to_string());
    surface.commit();
    queue.roundtrip(&mut state).expect("initial configure");
    let mut serial = state.configures.pop().expect("initial configure serial");
    xdg.ack_configure(serial);

    let small = create_buffer(&shm, &qh, &runtime_dir, 320, 200, "small");
    surface.attach(Some(&small.buffer), 0, 0);
    surface.damage(0, 0, 320, 200);
    surface.commit();
    queue.roundtrip(&mut state).expect("toplevel map");

    wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SCANOUT: mode=composite reason=not_fullscreen",
        Duration::from_secs(5),
    );

    // --- Fullscreen + full-output opaque buffer: direct scanout -------------
    toplevel.set_fullscreen(None);
    queue.roundtrip(&mut state).expect("fullscreen configure");
    if let Some(new_serial) = state.configures.pop() {
        serial = new_serial;
    }
    xdg.ack_configure(serial);

    let full = create_buffer(&shm, &qh, &runtime_dir, 1920, 1080, "full");
    surface.attach(Some(&full.buffer), 0, 0);
    surface.damage(0, 0, 1920, 1080);
    surface.commit();
    queue.roundtrip(&mut state).expect("fullscreen map");

    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SCANOUT: mode=direct window=0",
        Duration::from_secs(5),
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_WM_FULLSCREEN: id=0 rect=0,0,1920x1080"),
        "fullscreen geometry was not applied:\n{log}"
    );

    // The bypass path presents frames without planning any glass passes.
    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SCANOUT_FRAME:",
        Duration::from_secs(5),
    );
    let frame = marker_line(&log, &["AETHER_COMPOSITOR_SCANOUT_FRAME:"]).unwrap();
    assert!(frame.contains("window=0"), "scanout frame line: {frame}");
    assert!(frame.contains("passes=0"), "scanout frame line: {frame}");

    // --- A shell panel over the fullscreen window: compose again ------------
    let layer_wl_surface = compositor_proxy.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &layer_wl_surface,
        None,
        zwlr_layer_shell_v1::Layer::Top,
        "aether-scanout-panel".to_string(),
        &qh,
        (),
    );
    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    layer_surface.set_size(0, 32);
    layer_wl_surface.commit();
    queue.roundtrip(&mut state).expect("layer configure");
    if let Some((configure_serial, _w, _h)) = state.layer_configure {
        layer_surface.ack_configure(configure_serial);
    }
    let panel = create_buffer(&shm, &qh, &runtime_dir, 1920, 32, "panel");
    layer_wl_surface.attach(Some(&panel.buffer), 0, 0);
    layer_wl_surface.commit();
    queue.roundtrip(&mut state).expect("layer map");

    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SCANOUT: mode=composite reason=overlay_present",
        Duration::from_secs(5),
    );
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- compositor log ---\n{log}");
    }
}

fn two_windows_keep_composition() {
    let compositor = CompositorProcess::start("multi", &[]);
    let runtime_dir = compositor.runtime_dir.clone();
    std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
    std::env::set_var("WAYLAND_DISPLAY", "wayland-0");

    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let compositor_proxy = state.compositor.clone().expect("wl_compositor");
    let shm = state.shm.clone().expect("wl_shm");
    let wm_base = state.wm_base.clone().expect("xdg_wm_base");

    let mut buffers = Vec::new();
    for index in 0..2 {
        let surface = compositor_proxy.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        toplevel.set_title(format!("p3-t07-window-{index}"));
        surface.commit();
        queue.roundtrip(&mut state).expect("configure");
        let serial = state.configures.pop().expect("configure serial");
        xdg.ack_configure(serial);
        let buffer = create_buffer(
            &shm,
            &qh,
            &runtime_dir,
            320,
            200,
            &format!("window-{index}"),
        );
        surface.attach(Some(&buffer.buffer), 0, 0);
        surface.commit();
        queue.roundtrip(&mut state).expect("map");
        buffers.push(buffer);
    }

    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SCANOUT: mode=composite reason=multiple_windows",
        Duration::from_secs(5),
    );
    assert!(
        !log.contains("AETHER_COMPOSITOR_SCANOUT: mode=direct"),
        "two windows must never take the scanout path:\n{log}"
    );
    let _ = &buffers;
}

/// First log line containing every needle.
fn marker_line<'a>(log: &'a str, needles: &[&str]) -> Option<&'a str> {
    log.lines()
        .find(|line| needles.iter().all(|needle| line.contains(needle)))
}

/// Poll the compositor log until every needle is present, then return it.
fn wait_for_log(runtime_dir: &Path, needle: &str, timeout: Duration) -> String {
    let path = runtime_dir.join("compositor.log");
    let deadline = Instant::now() + timeout;
    loop {
        let log = fs::read_to_string(&path).unwrap_or_default();
        if log.contains(needle) {
            return log;
        }
        if Instant::now() >= deadline {
            panic!("compositor log never contained {needle:?}:\n{log}");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The compositor under test, killed and cleaned up on drop.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    fn start(name: &str, env: &[(&str, &str)]) -> Self {
        let runtime_dir = std::env::temp_dir().join(format!(
            "aether-p3-t07-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&runtime_dir);
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
            .expect("chmod runtime dir");

        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("create log");
        let stderr =
            fs::File::create(runtime_dir.join("compositor.err")).expect("create stderr log");
        let mut command = Command::new(env!("CARGO_BIN_EXE_aether-compositor"));
        command
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("AETHER_COMPOSITOR_GRACE_MS", "15000")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr));
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn aether-compositor");

        Self::wait_for_socket(&runtime_dir, &mut child);
        Self { runtime_dir, child }
    }

    fn wait_for_socket(runtime_dir: &Path, child: &mut Child) {
        let socket = runtime_dir.join("wayland-0");
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if socket.exists() {
                return;
            }
            if let Ok(Some(status)) = child.try_wait() {
                panic!("compositor exited before creating its socket: {status}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("compositor did not create {} in time", socket.display());
    }
}

impl Drop for CompositorProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

/// A committed SHM buffer plus the file and pool that keep it valid.
struct ShmBuffer {
    _file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
}

/// Create and fill an opaque (XRGB8888) SHM buffer.
fn create_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<ClientState>,
    runtime_dir: &Path,
    width: i32,
    height: i32,
    name: &str,
) -> ShmBuffer {
    use std::os::fd::AsFd;

    let len = (width * height * 4) as usize;
    let path = runtime_dir.join(format!("shm-{name}-{width}x{height}.raw"));
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .expect("create shm file");
    let mut pixels = vec![0u8; len];
    for (index, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        let shade = ((index / 11) % 220) as u8;
        chunk[0] = shade;
        chunk[1] = 0x40;
        chunk[2] = 0x80;
        chunk[3] = 0xff;
    }
    file.write_all(&pixels).expect("fill shm file");
    file.flush().expect("flush shm file");

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
    ShmBuffer {
        _file: file,
        _pool: pool,
        buffer,
    }
}
