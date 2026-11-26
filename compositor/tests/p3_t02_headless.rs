//! P3-T02 headless acceptance test.
//!
//! Boots the real `aether-compositor` binary on a private `XDG_RUNTIME_DIR`
//! and drives it with one Wayland client that:
//!
//! 1. enumerates the outputs and asserts the two-monitor layout at different
//!    scales (1920×1080 @ 1× and 2880×1800 @ 1.5× → logical 1920×1200 at x=1920);
//! 2. creates an xdg-shell toplevel and maps an SHM buffer;
//! 3. creates a `zwlr_layer_shell_v1` surface for the shell panel on the HiDPI
//!    monitor and asserts the compositor configures it at the *logical* size
//!    (1920×32) and advertises the fractional scale (1.5 → 180/120);
//! 4. draws the layer surface at buffer scale 2.
//!
//! The test then checks the compositor's stdout markers. It needs no DRM,
//! input, or GPU, so it runs in CI (`cargo test --workspace`) and on the
//! macOS dev host.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

/// Everything the client learns about one `wl_output`.
#[derive(Default)]
struct OutputInfo {
    proxy: Option<wl_output::WlOutput>,
    name: String,
    scale: i32,
    mode: (i32, i32),
    logical_size: (i32, i32),
    logical_position: (i32, i32),
}

#[derive(Default)]
struct ClientState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    xdg_output_manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    fractional_scale_manager: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    /// (registry name, advertised version) of every `wl_output` global.
    output_globals: Vec<(u32, u32)>,
    /// Keyed by the registry name of the output's `wl_output` global.
    outputs: HashMap<u32, OutputInfo>,
    xdg_configure: Option<u32>,
    /// (serial, width, height) of the last layer-surface configure.
    layer_configure: Option<(u32, u32, u32)>,
    /// `wp_fractional_scale_v1.preferred_scale` (1/120 units).
    preferred_scale_120: Option<u32>,
    toplevel_closed: bool,
}

/// A committed SHM buffer plus the file and pool that keep it valid.
struct ShmBuffer {
    _file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
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
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_compositor" => {
                let proxy: wl_compositor::WlCompositor = registry.bind(name, 4, qh, ());
                state.compositor = Some(proxy);
            }
            "wl_shm" => {
                let proxy: wl_shm::WlShm = registry.bind(name, 1, qh, ());
                state.shm = Some(proxy);
            }
            "xdg_wm_base" => {
                let proxy: xdg_wm_base::XdgWmBase = registry.bind(name, 1, qh, ());
                state.wm_base = Some(proxy);
            }
            "zwlr_layer_shell_v1" => {
                let proxy: zwlr_layer_shell_v1::ZwlrLayerShellV1 = registry.bind(name, 3, qh, ());
                state.layer_shell = Some(proxy);
            }
            "zxdg_output_manager_v1" => {
                let proxy: zxdg_output_manager_v1::ZxdgOutputManagerV1 =
                    registry.bind(name, 2, qh, ());
                state.xdg_output_manager = Some(proxy);
            }
            "wp_fractional_scale_manager_v1" => {
                let proxy: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1 =
                    registry.bind(name, 1, qh, ());
                state.fractional_scale_manager = Some(proxy);
            }
            "wl_output" => {
                state.output_globals.push((name, version));
                let proxy: wl_output::WlOutput = registry.bind(name, version.min(4), qh, name);
                state.outputs.entry(name).or_default().proxy = Some(proxy);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        global: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let output = state.outputs.entry(*global).or_default();
        match event {
            wl_output::Event::Name { name } => output.name = name,
            wl_output::Event::Scale { factor } => output.scale = factor,
            wl_output::Event::Mode { width, height, .. } => output.mode = (width, height),
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        global: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let output = state.outputs.entry(*global).or_default();
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => output.logical_position = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical_size = (width, height)
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_surface::XdgSurface,
        event: <xdg_surface::XdgSurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            state.xdg_configure = Some(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: <xdg_toplevel::XdgToplevel as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Close = event {
            state.toplevel_closed = true;
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

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.preferred_scale_120 = Some(scale);
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
ignore_events!(zxdg_output_manager_v1::ZxdgOutputManagerV1, ());
ignore_events!(
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    ()
);
ignore_events!(zwlr_layer_shell_v1::ZwlrLayerShellV1, ());

#[test]
fn two_outputs_layer_shell_and_xdg_toplevel() {
    let compositor = CompositorProcess::start();
    let runtime_dir = compositor.runtime_dir.clone();

    // The client resolves the display through the same environment.
    std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
    std::env::set_var("WAYLAND_DISPLAY", "wayland-0");

    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let display = conn.display();
    let _registry = display.get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    // Bind xdg-output objects; they need the manager global from the registry.
    {
        let manager = state
            .xdg_output_manager
            .clone()
            .expect("compositor advertises zxdg_output_manager_v1");
        for (name, _) in state.output_globals.clone() {
            if let Some(proxy) = state.outputs.get(&name).and_then(|o| o.proxy.clone()) {
                manager.get_xdg_output(&proxy, &qh, name);
            }
        }
        queue.roundtrip(&mut state).expect("xdg-output roundtrip");
    }

    let compositor = state
        .compositor
        .clone()
        .expect("compositor advertises wl_compositor");
    let shm = state.shm.clone().expect("compositor advertises wl_shm");
    let wm_base = state
        .wm_base
        .clone()
        .expect("compositor advertises xdg_wm_base");
    let layer_shell = state
        .layer_shell
        .clone()
        .expect("compositor advertises zwlr_layer_shell_v1");
    let fractional_manager = state
        .fractional_scale_manager
        .clone()
        .expect("compositor advertises wp_fractional_scale_manager_v1");

    // --- Multi-monitor layout + HiDPI scales --------------------------------
    assert_eq!(
        state.outputs.len(),
        2,
        "expected two wl_output globals, got {}",
        state.outputs.len()
    );

    let by_name = |name: &str| -> &OutputInfo {
        state
            .outputs
            .values()
            .find(|output| output.name == name)
            .unwrap_or_else(|| panic!("no output named {name}"))
    };

    let primary = by_name("aether-headless-0");
    assert_eq!(primary.scale, 1, "primary output scale");
    assert_eq!(primary.mode, (1920, 1080), "primary output mode");
    assert_eq!(primary.logical_size, (1920, 1080), "primary logical size");
    assert_eq!(primary.logical_position, (0, 0), "primary logical position");

    let hdpi = by_name("aether-headless-1");
    assert_eq!(
        hdpi.scale, 2,
        "second output advertises the integer-rounded scale"
    );
    assert_eq!(hdpi.mode, (2880, 1800), "second output mode");
    assert_eq!(
        hdpi.logical_size,
        (1920, 1200),
        "second output logical size at 1.5x"
    );
    assert_eq!(
        hdpi.logical_position,
        (1920, 0),
        "second output sits to the right of the first"
    );
    let hdpi_output = hdpi.proxy.clone().expect("bound wl_output");

    // --- Links: xdg-shell toplevel maps -------------------------------------
    let top_surface = compositor.create_surface(&qh, ());
    let top_xdg = wm_base.get_xdg_surface(&top_surface, &qh, ());
    let top_toplevel = top_xdg.get_toplevel(&qh, ());
    top_toplevel.set_title("p3-t02-toplevel".to_string());
    top_surface.commit();
    queue.roundtrip(&mut state).expect("toplevel configure");
    queue.roundtrip(&mut state).expect("toplevel configure");
    let serial = state.xdg_configure.expect("xdg_surface configure");
    top_xdg.ack_configure(serial);

    let top_buffer = create_buffer(&shm, &qh, &runtime_dir, 100, 100);
    top_surface.attach(Some(&top_buffer.buffer), 0, 0);
    top_surface.commit();
    queue.roundtrip(&mut state).expect("toplevel map");
    assert!(!state.toplevel_closed, "toplevel must stay open");

    // --- Layer shell on the HiDPI monitor -----------------------------------
    let layer_wl_surface = compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &layer_wl_surface,
        Some(&hdpi_output),
        zwlr_layer_shell_v1::Layer::Top,
        "aether-shell-test".to_string(),
        &qh,
        (),
    );
    fractional_manager.get_fractional_scale(&layer_wl_surface, &qh, ());
    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    layer_surface.set_size(0, 32);
    layer_surface.set_exclusive_zone(32);
    layer_wl_surface.commit();
    queue.roundtrip(&mut state).expect("layer configure");
    queue.roundtrip(&mut state).expect("layer configure");

    let (configure_serial, width, height) = state.layer_configure.expect("layer surface configure");
    assert_eq!(
        (width, height),
        (1920, 32),
        "panel is anchored left+right on the 1920-logical-wide HiDPI output"
    );
    assert_eq!(
        state.preferred_scale_120,
        Some(180),
        "fractional scale is 1.5 (180/120)"
    );
    layer_surface.ack_configure(configure_serial);

    // Draw the panel for the HiDPI monitor: 200×64 buffer at buffer scale 2.
    layer_wl_surface.set_buffer_scale(2);
    let layer_buffer = create_buffer(&shm, &qh, &runtime_dir, 200, 64);
    layer_wl_surface.attach(Some(&layer_buffer.buffer), 0, 0);
    layer_wl_surface.commit();
    queue.roundtrip(&mut state).expect("layer map");

    // --- Compositor-side markers --------------------------------------------
    let log = fs::read_to_string(runtime_dir.join("compositor.log")).expect("compositor log");
    assert_eq!(
        log.matches("AETHER_COMPOSITOR_OUTPUT: ").count(),
        2,
        "compositor created two outputs:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_OUTPUT: name=aether-headless-1 mode=2880x1800 scale=1.5"),
        "HiDPI output advertised:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-test")
            && log.contains("output=aether-headless-1"),
        "layer surface created on the HiDPI output:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-test size=1920x32"),
        "layer surface configured at its logical size:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_XDG_TOPLEVEL: output=aether-headless-0"),
        "xdg toplevel created on the primary output:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_BUFFER_SCALE: surface_buffer_scale=2"),
        "buffer scale 2 recorded:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_CLIENT_MAPPED"),
        "compositor observed a mapped surface:\n{log}"
    );
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- compositor log ---\n{log}");
    }

    // CompositorProcess::drop kills the child and removes the runtime dir.
}

/// The compositor under test, killed and cleaned up on drop — even when an
/// assertion panics.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    /// Run the compositor on a private runtime dir. Its stdout goes to
    /// `<runtime-dir>/compositor.log` for the marker assertions.
    fn start() -> Self {
        let runtime_dir =
            std::env::temp_dir().join(format!("aether-p3-t02-{}", std::process::id()));
        let _ = fs::remove_dir_all(&runtime_dir);
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
            .expect("chmod runtime dir");

        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("create log");
        let stderr =
            fs::File::create(runtime_dir.join("compositor.err")).expect("create stderr log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_aether-compositor"))
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            // Keep the compositor alive across the xdg-shell and layer-shell
            // phases (default is 3 s after the first mapped surface).
            .env("AETHER_COMPOSITOR_GRACE_MS", "20000")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn aether-compositor");

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

/// Create and fill an SHM buffer. A real file is used instead of `memfd_create`
/// so the test also runs on the macOS dev host.
fn create_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<ClientState>,
    runtime_dir: &Path,
    width: i32,
    height: i32,
) -> ShmBuffer {
    use std::os::fd::AsFd;

    let len = (width * height * 4) as usize;
    let path = runtime_dir.join(format!("shm-{width}x{height}.raw"));
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .expect("create shm file");
    let mut pixels = vec![0u8; len];
    for (index, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        let (b, g, r) = match index % width as usize {
            0..=1 => (0xff, 0xff, 0xff),
            _ => (0x80, 0x40, 0x10),
        };
        chunk[0] = b;
        chunk[1] = g;
        chunk[2] = r;
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
