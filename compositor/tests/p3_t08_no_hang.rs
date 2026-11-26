//! P3-T08 headless acceptance test — no-hang design.
//!
//! Boots the real `aether-compositor` binary three times (sequentially, because
//! the scenarios share the process-global Wayland environment) with a short
//! watchdog configuration and checks the state machine end to end:
//!
//! 1. **responsive app** — the client dispatches its events and pongs, so the
//!    log shows `AETHER_COMPOSITOR_PING` and `AETHER_COMPOSITOR_PONG`; a frame
//!    that waits in the damage queue produces `AETHER_COMPOSITOR_FRAME_GAP`;
//! 2. **hung app** — the client stops dispatching, the ping is not answered and
//!    the compositor prints `AETHER_COMPOSITOR_APP_STATE: … state=not_responding`
//!    (the G-NH2 signal the shell turns into "Not responding"); a late pong
//!    then recovers it (`state=responsive recovered=true`) without a restart;
//! 3. **shell watchdog** — a layer-shell surface stops committing and the
//!    compositor logs `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout`, then
//!    `state=ok recovered=true` when the shell commits again.
//!
//! Real hardware adds nothing here: the ping/pong exchange is pure protocol.
//! G-NH2's kill/restart part is the shell's action on the marker and stays on
//! the integration checklist.

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

const TEST_ENV: &[(&str, &str)] = &[
    ("AETHER_COMPOSITOR_PING_INTERVAL_MS", "100"),
    ("AETHER_COMPOSITOR_PING_TIMEOUT_MS", "400"),
    ("AETHER_COMPOSITOR_FRAME_GAP_MS", "0"),
    ("AETHER_COMPOSITOR_SHELL_WATCHDOG_MS", "300"),
];

#[derive(Default)]
struct ClientState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
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
            "wl_compositor" => state.compositor = Some(registry.bind(name, 4, qh, ())),
            "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
            "xdg_wm_base" => state.wm_base = Some(registry.bind(name, 1, qh, ())),
            "zwlr_layer_shell_v1" => {
                state.layer_shell = Some(registry.bind(name, 3, qh, ()))
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
        // A real app replies to the ping from its event loop.
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
fn no_hang_state_machine() {
    // Sequential: the scenarios change the process-global XDG_RUNTIME_DIR.
    responsive_app_is_pinged();
    hung_app_is_marked_not_responding_and_recovers();
    shell_watchdog_reports_a_stalled_shell();
}

fn responsive_app_is_pinged() {
    let compositor = CompositorProcess::start("responsive", TEST_ENV);
    let runtime_dir = compositor.runtime_dir.clone();
    connect(&runtime_dir);
    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let compositor_proxy = state.compositor.clone().expect("wl_compositor");
    let shm = state.shm.clone().expect("wl_shm");
    let wm_base = state.wm_base.clone().expect("xdg_wm_base");

    let surface = compositor_proxy.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("p3-t08-responsive".to_string());
    surface.commit();
    queue.roundtrip(&mut state).expect("configure");
    let serial = state.configures.pop().expect("configure serial");
    xdg.ack_configure(serial);
    let buffer = create_buffer(&shm, &qh, &runtime_dir, 320, 200, "responsive");
    surface.attach(Some(&buffer.buffer), 0, 0);
    surface.damage(0, 0, 320, 200);
    surface.commit();
    queue.roundtrip(&mut state).expect("map");

    // Keep dispatching so the ping is answered from the event loop.
    let deadline = Instant::now() + Duration::from_millis(900);
    while Instant::now() < deadline {
        queue.roundtrip(&mut state).expect("ping/pong roundtrip");
        std::thread::sleep(Duration::from_millis(50));
    }

    let log = fs::read_to_string(runtime_dir.join("compositor.log")).expect("compositor log");
    assert!(
        log.contains("AETHER_COMPOSITOR_PING: client=0"),
        "no ping sent:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_PONG: client=0") && log.contains("recovered=false"),
        "responsive client did not pong:\n{log}"
    );
    assert!(
        !log.contains("state=not_responding"),
        "a responsive client must not be marked hung:\n{log}"
    );
    // Frame-gap logger: with a 0 ms threshold any damaged frame is a gap.
    assert!(
        log.contains("AETHER_COMPOSITOR_FRAME_GAP: ms="),
        "frame-gap logger never ran:\n{log}"
    );
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- responsive log ---\n{log}");
    }
    drop(buffer);
}

fn hung_app_is_marked_not_responding_and_recovers() {
    let compositor = CompositorProcess::start("hung", TEST_ENV);
    let runtime_dir = compositor.runtime_dir.clone();
    connect(&runtime_dir);
    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let compositor_proxy = state.compositor.clone().expect("wl_compositor");
    let shm = state.shm.clone().expect("wl_shm");
    let wm_base = state.wm_base.clone().expect("xdg_wm_base");

    let surface = compositor_proxy.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("p3-t08-hung".to_string());
    surface.commit();
    queue.roundtrip(&mut state).expect("configure");
    let serial = state.configures.pop().expect("configure serial");
    xdg.ack_configure(serial);
    let buffer = create_buffer(&shm, &qh, &runtime_dir, 320, 200, "hung");
    surface.attach(Some(&buffer.buffer), 0, 0);
    surface.commit();
    queue.roundtrip(&mut state).expect("map");

    // The app "freezes": it stops dispatching its events, so the ping is left
    // unanswered in the socket.
    std::thread::sleep(Duration::from_millis(1200));

    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_APP_STATE: client=0 state=not_responding",
        Duration::from_secs(5),
    );
    assert!(
        log.contains("after_ms="),
        "the timeout duration is part of the evidence:\n{log}"
    );

    // The app comes back and answers the queued ping: it recovers without a
    // compositor restart.
    let deadline = Instant::now() + Duration::from_millis(900);
    while Instant::now() < deadline {
        queue.roundtrip(&mut state).expect("recovery roundtrip");
        std::thread::sleep(Duration::from_millis(50));
    }
    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_APP_STATE: client=0 state=responsive recovered=true",
        Duration::from_secs(5),
    );
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- hung log ---\n{log}");
    }
    drop(buffer);
}

fn shell_watchdog_reports_a_stalled_shell() {
    let compositor = CompositorProcess::start("shell", TEST_ENV);
    let runtime_dir = compositor.runtime_dir.clone();
    connect(&runtime_dir);
    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let compositor_proxy = state.compositor.clone().expect("wl_compositor");
    let shm = state.shm.clone().expect("wl_shm");
    let layer_shell = state.layer_shell.clone().expect("zwlr_layer_shell_v1");

    let wl_surface = compositor_proxy.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &wl_surface,
        None,
        zwlr_layer_shell_v1::Layer::Top,
        "aether-shell-watchdog".to_string(),
        &qh,
        (),
    );
    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    layer_surface.set_size(0, 32);
    wl_surface.commit();
    queue.roundtrip(&mut state).expect("layer configure");
    if let Some((serial, _w, _h)) = state.layer_configure {
        layer_surface.ack_configure(serial);
    }
    let buffer = create_buffer(&shm, &qh, &runtime_dir, 320, 32, "shell");
    wl_surface.attach(Some(&buffer.buffer), 0, 0);
    wl_surface.damage(0, 0, 320, 32);
    wl_surface.commit();
    queue.roundtrip(&mut state).expect("layer map");

    // The shell stops committing for longer than the watchdog timeout.
    std::thread::sleep(Duration::from_millis(800));
    wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout",
        Duration::from_secs(5),
    );

    // The shell comes back: one commit recovers the watchdog.
    wl_surface.damage(0, 0, 320, 32);
    wl_surface.commit();
    queue.roundtrip(&mut state).expect("shell recovery commit");
    let log = wait_for_log(
        &runtime_dir,
        "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true",
        Duration::from_secs(5),
    );
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- shell log ---\n{log}");
    }
    drop(buffer);
}

fn connect(runtime_dir: &Path) {
    std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
    std::env::set_var("WAYLAND_DISPLAY", "wayland-0");
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
            "aether-p3-t08-{name}-{}",
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
        let shade = ((index / 13) % 200) as u8;
        chunk[0] = 0x30;
        chunk[1] = shade;
        chunk[2] = 0x70;
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
