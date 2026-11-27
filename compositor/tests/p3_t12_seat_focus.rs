//! P3-T12: layer-surface keyboard focus + headless key injection.
//!
//! Boots the real `aether-compositor` binary (headless backend) with
//! `AETHER_COMPOSITOR_INJECT_KEYS` and drives it with one Wayland client that
//! binds `wl_seat`/`wl_keyboard` and creates:
//!
//! 1. an xdg toplevel (maps first, takes the keyboard, receives injected keys);
//! 2. an `on_demand` layer panel (must **not** take focus from the toplevel);
//! 3. an `exclusive` layer surface (must take focus, and the toplevel must get
//!    `wl_keyboard.leave`);
//! 4. a `none` layer surface (must never take focus).
//!
//! It asserts both sides: the compositor's own markers
//! (`AETHER_COMPOSITOR_SEAT_FOCUS`, `AETHER_COMPOSITOR_KEY_INJECTED`) and what
//! the client actually saw on its `wl_keyboard` (evdev + 8 keycodes).
//!
//! Needs no DRM, input device or GPU, so it runs in CI and on the macOS dev
//! host. The injection hook itself is test-only plumbing — see
//! `compositor/README.md` and `docs/2026/11/27/results/compositor-seat-focus-injection.md`.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
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
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// Keycodes (`evdev + 8`, as `wl_keyboard.key` delivers them) with the
    /// pressed flag of the `wl_keyboard.key` event.
    keys: Vec<(u32, bool)>,
    /// `wl_keyboard.enter` / `leave` counters for the toplevel surface.
    enters: u32,
    leaves: u32,
    /// Modifier mask of the last `wl_keyboard.modifiers` event.
    mods_depressed: u32,
    xdg_configure: Option<u32>,
    layer_configure: Option<(u32, u32, u32)>,
}

impl ClientState {
    fn pressed(&self, keycode: u32) -> bool {
        self.keys.iter().any(|(key, pressed)| *key == keycode && *pressed)
    }
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
                state.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(
                    name,
                    version.min(4),
                    qh,
                    (),
                ));
            }
            "wl_shm" => {
                state.shm =
                    Some(registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ()));
            }
            "xdg_wm_base" => {
                state.wm_base = Some(registry.bind::<xdg_wm_base::XdgWmBase, _, _>(
                    name,
                    version.min(1),
                    qh,
                    (),
                ));
            }
            "zwlr_layer_shell_v1" => {
                state.layer_shell = Some(
                    registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(
                        name,
                        version.min(3),
                        qh,
                        (),
                    ),
                );
            }
            "wl_seat" => {
                state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(
                    name,
                    version.min(7),
                    qh,
                    (),
                ));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for ClientState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            let capabilities = match capabilities {
                wayland_client::WEnum::Value(value) => value,
                wayland_client::WEnum::Unknown(_) => return,
            };
            if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for ClientState {
    fn event(
        state: &mut Self,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            // The keymap fd is dropped with the event; the test only needs the
            // raw keycodes the compositor forwards.
            wl_keyboard::Event::Enter { .. } => state.enters += 1,
            wl_keyboard::Event::Leave { .. } => state.leaves += 1,
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = matches!(
                    key_state,
                    wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed)
                );
                state.keys.push((key, pressed));
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed, ..
            } => state.mods_depressed = mods_depressed,
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
            state.xdg_configure = Some(serial);
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
ignore_events!(xdg_wm_base::XdgWmBase, ());
ignore_events!(zwlr_layer_shell_v1::ZwlrLayerShellV1, ());

/// A committed SHM buffer plus the file and pool that keep it valid.
struct ShmBuffer {
    _file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
}

#[test]
fn layer_keyboard_focus_and_headless_key_injection() {
    let compositor = CompositorProcess::start("30,48");
    let runtime_dir = compositor.runtime_dir.clone();

    std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
    std::env::set_var("WAYLAND_DISPLAY", "wayland-0");

    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let wl_compositor = state
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

    // --- The injection hook gives the headless backend a seat ----------------
    // Without `AETHER_COMPOSITOR_INJECT_KEYS` the headless backend advertises no
    // `wl_seat` at all, so this also proves the seat is created only on demand.
    assert!(
        state.seat.is_some(),
        "key injection must advertise a wl_seat; got none"
    );
    queue.roundtrip(&mut state).expect("seat capabilities");
    assert!(
        state.keyboard.is_some(),
        "the injected seat must advertise a wl_keyboard"
    );

    // --- 1. xdg toplevel takes the keyboard and receives injected keys -------
    let top_surface = wl_compositor.create_surface(&qh, ());
    let top_xdg = wm_base.get_xdg_surface(&top_surface, &qh, ());
    let top_toplevel = top_xdg.get_toplevel(&qh, ());
    top_toplevel.set_title("p3-t12-toplevel".to_string());
    top_surface.commit();
    queue.roundtrip(&mut state).expect("toplevel configure");
    queue.roundtrip(&mut state).expect("toplevel configure");
    let serial = state.xdg_configure.expect("xdg_surface configure");
    top_xdg.ack_configure(serial);
    let top_buffer = create_buffer(&shm, &qh, &runtime_dir, 320, 200, "toplevel");
    top_surface.attach(Some(&top_buffer.buffer), 0, 0);
    top_surface.commit();
    queue.roundtrip(&mut state).expect("toplevel map");

    // The compositor replays evdev 30 (KEY_A) once a client is mapped and
    // focused. `wl_keyboard.key` carries the evdev code (xkb keycode − 8), so the
    // client sees 30 — the same value `shell/src/keys.rs` adds 8 to. Wait for it.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !state.pressed(30) && Instant::now() < deadline {
        queue.roundtrip(&mut state).expect("key delivery roundtrip");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        state.pressed(30),
        "the focused toplevel must receive the injected key through wl_keyboard; saw {:?}",
        state.keys
    );
    assert!(state.enters >= 1, "the toplevel must get wl_keyboard.enter");
    let enters_after_toplevel = state.enters;

    // --- 2. An `on_demand` panel must not steal focus ------------------------
    let panel = map_layer_surface(
        &layer_shell,
        &wl_compositor,
        &shm,
        &qh,
        &mut queue,
        &mut state,
        &runtime_dir,
        "aether-shell-panel",
        zwlr_layer_shell_v1::Layer::Top,
        zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
        360,
        200,
        "panel",
    );
    queue.roundtrip(&mut state).expect("panel map");
    assert_eq!(
        state.enters, enters_after_toplevel,
        "an on_demand panel must not take focus from a mapped toplevel"
    );
    assert_eq!(state.leaves, 0, "the toplevel must keep the keyboard");

    // --- 3. An `exclusive` overlay takes the keyboard ------------------------
    let launcher = map_layer_surface(
        &layer_shell,
        &wl_compositor,
        &shm,
        &qh,
        &mut queue,
        &mut state,
        &runtime_dir,
        "aether-shell-launcher",
        zwlr_layer_shell_v1::Layer::Overlay,
        zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
        320,
        240,
        "launcher",
    );
    queue.roundtrip(&mut state).expect("launcher map");
    queue.roundtrip(&mut state).expect("launcher focus");
    assert_eq!(
        state.leaves, 1,
        "the toplevel must lose the keyboard to the exclusive overlay; saw {:?}",
        state.keys
    );
    assert!(
        state.enters > enters_after_toplevel,
        "the exclusive overlay must receive wl_keyboard.enter"
    );

    // --- 4. A `none` surface never takes focus -------------------------------
    let silent = map_layer_surface(
        &layer_shell,
        &wl_compositor,
        &shm,
        &qh,
        &mut queue,
        &mut state,
        &runtime_dir,
        "aether-shell-silent",
        zwlr_layer_shell_v1::Layer::Top,
        zwlr_layer_surface_v1::KeyboardInteractivity::None,
        200,
        100,
        "silent",
    );
    queue.roundtrip(&mut state).expect("silent map");
    let leaves_before = state.leaves;
    queue.roundtrip(&mut state).expect("silent settle");
    assert_eq!(
        state.leaves, leaves_before,
        "a keyboard_interactivity=none surface must never take focus"
    );

    // --- Compositor-side markers --------------------------------------------
    let log = fs::read_to_string(runtime_dir.join("compositor.log")).expect("compositor log");
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- compositor log ---\n{log}");
    }
    assert!(
        log.contains("AETHER_COMPOSITOR_KEY_INJECTION: backend=headless keys=2 evdev=30,48"),
        "the injection hook must announce its queue:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_SEAT_FOCUS: surface=yes"),
        "the mapped toplevel must be focused:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_SEAT_FOCUS: surface=exclusive layer=aether-shell-launcher"),
        "the exclusive overlay must be focused and named:\n{log}"
    );
    assert!(
        !log.contains("surface=on_demand"),
        "an on_demand panel must never take focus while a toplevel is focused:\n{log}"
    );
    assert!(
        !log.contains("layer=aether-shell-silent"),
        "a keyboard_interactivity=none surface must never be focused:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_KEY_INJECTED: evdev=30 focus=surface"),
        "the injected key must be delivered to a focused surface:\n{log}"
    );

    // Keep the proxies alive until the assertions above have run.
    let _ = (&panel, &launcher, &silent, &top_surface);
}

/// Create, configure, map and return one layer surface.
#[allow(clippy::too_many_arguments)]
fn map_layer_surface(
    layer_shell: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
    compositor: &wl_compositor::WlCompositor,
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<ClientState>,
    queue: &mut wayland_client::EventQueue<ClientState>,
    state: &mut ClientState,
    runtime_dir: &Path,
    namespace: &str,
    layer: zwlr_layer_shell_v1::Layer,
    keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    width: i32,
    height: i32,
    label: &str,
) -> zwlr_layer_surface_v1::ZwlrLayerSurfaceV1 {
    let surface = compositor.create_surface(qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None,
        layer,
        namespace.to_string(),
        qh,
        (),
    );
    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left,
    );
    layer_surface.set_size(width as u32, height as u32);
    layer_surface.set_keyboard_interactivity(keyboard);
    surface.commit();
    queue.roundtrip(state).expect("layer configure");
    queue.roundtrip(state).expect("layer configure");
    let (serial, _, _) = state.layer_configure.expect("layer surface configure");
    layer_surface.ack_configure(serial);

    let buffer = create_buffer(shm, qh, runtime_dir, width, height, label);
    surface.attach(Some(&buffer.buffer), 0, 0);
    surface.commit();
    queue.roundtrip(state).expect("layer map");

    // The buffer must outlive the map; leak it into the compositor connection's
    // lifetime by forgetting the pool/file pair (the process exits at test end).
    std::mem::forget(buffer);
    layer_surface
}

/// The compositor under test, killed and cleaned up on drop.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    /// Run the compositor on a private runtime dir with headless key injection
    /// for `keys` (evdev codes).
    fn start(keys: &str) -> Self {
        let runtime_dir = std::env::temp_dir().join(format!(
            "aether-p3-t12-{}-{}",
            std::process::id(),
            keys.replace(',', "-")
        ));
        let _ = fs::remove_dir_all(&runtime_dir);
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
            .expect("chmod runtime dir");

        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("create log");
        let stderr =
            fs::File::create(runtime_dir.join("compositor.err")).expect("create stderr log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_aether-compositor"))
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("AETHER_COMPOSITOR_BACKEND", "headless")
            .env("AETHER_COMPOSITOR_INJECT_KEYS", keys)
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

/// Create and fill an SHM buffer (a real file, so this also runs on macOS).
fn create_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<ClientState>,
    runtime_dir: &Path,
    width: i32,
    height: i32,
    label: &str,
) -> ShmBuffer {
    use std::os::fd::AsFd;

    let len = (width * height * 4) as usize;
    let path = runtime_dir.join(format!("shm-{label}-{width}x{height}.raw"));
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
