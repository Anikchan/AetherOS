//! P3-T13: frame callbacks, buffer release (double buffering) and pointer
//! forwarding, driven over the real protocol.
//!
//! Boots the shipped `aether-compositor` (headless backend) with
//! `AETHER_COMPOSITOR_INJECT_KEYS` and `AETHER_COMPOSITOR_INJECT_POINTER`, maps
//! a bottom-anchored dock layer surface, and asserts:
//!
//! 1. the client's `wl_surface.frame` callback is answered even when nothing is
//!    damaged (otherwise a client that paces itself on it stalls forever);
//! 2. attaching buffer B after that callback releases buffer A through
//!    `wl_buffer.release`, which is what makes double buffering possible;
//! 3. the injected pointer click reaches the *dock* with surface-local
//!    coordinates and arrives as `wl_pointer.enter` + motion + button, and the
//!    compositor logs `AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock …`;
//! 4. clicking a dock whose `keyboard_interactivity` is `none` does not take
//!    the keyboard (the injected key still reports `focus=none`).
//!
//! Needs no DRM, input device or GPU: CI and the macOS dev host.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

#[derive(Default)]
struct ClientState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    /// `wl_pointer.enter` positions (surface-local).
    enters: Vec<(f64, f64)>,
    /// `wl_pointer.motion` positions (surface-local).
    motions: Vec<(f64, f64)>,
    /// `(button, pressed)` from `wl_pointer.button`.
    buttons: Vec<(u32, bool)>,
    leaves: u32,
    /// `wl_callback.done` events (`wl_surface.frame`).
    frame_done: u32,
    /// Labels of the buffers the compositor released.
    released: Vec<&'static str>,
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
            if capabilities.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for ClientState {
    fn event(
        state: &mut Self,
        _pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface_x, surface_y, .. } => {
                state.enters.push((surface_x, surface_y));
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                state.motions.push((surface_x, surface_y));
            }
            wl_pointer::Event::Button { button, state: button_state, .. } => {
                let pressed = matches!(
                    button_state,
                    wayland_client::WEnum::Value(wl_pointer::ButtonState::Pressed)
                );
                state.buttons.push((button, pressed));
            }
            wl_pointer::Event::Leave { .. } => state.leaves += 1,
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for ClientState {
    fn event(
        state: &mut Self,
        _callback: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.frame_done += 1;
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, &'static str> for ClientState {
    fn event(
        state: &mut Self,
        _buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        label: &&'static str,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            state.released.push(label);
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
ignore_events!(zwlr_layer_shell_v1::ZwlrLayerShellV1, ());

/// A committed SHM buffer plus the file and pool that keep it valid.
struct ShmBuffer {
    _file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
}

/// Drive the event queue until `predicate` is true or `timeout` elapses.
fn wait_for(
    state: &mut ClientState,
    queue: &mut wayland_client::EventQueue<ClientState>,
    timeout: Duration,
    what: &str,
    predicate: impl Fn(&ClientState) -> bool,
) {
    let deadline = Instant::now() + timeout;
    while !predicate(state) && Instant::now() < deadline {
        queue.roundtrip(state).expect("roundtrip");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(predicate(state), "timed out waiting for {what}");
}

#[test]
fn frame_callbacks_buffers_and_pointer_reach_the_dock() {
    let mut compositor = CompositorProcess::start("15", "x=600,y=1050,button=1");
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
    let layer_shell = state
        .layer_shell
        .clone()
        .expect("compositor advertises zwlr_layer_shell_v1");
    assert!(
        state.seat.is_some(),
        "the injection hooks must advertise a wl_seat"
    );
    queue.roundtrip(&mut state).expect("seat capabilities");
    assert!(
        state.pointer.is_some(),
        "the injected seat must advertise a wl_pointer"
    );

    // --- The shell's dock: 720x64 anchored to the bottom edge ----------------
    let surface = wl_compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None,
        zwlr_layer_shell_v1::Layer::Top,
        "aether-shell-dock".to_string(),
        &qh,
        (),
    );
    layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::Bottom);
    layer_surface.set_size(720, 64);
    // A dock never takes the keyboard, but it must still receive pointer input.
    layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    surface.commit();
    queue.roundtrip(&mut state).expect("dock configure");
    queue.roundtrip(&mut state).expect("dock configure");
    let (serial, width, height) = state.layer_configure.expect("dock configure");
    assert_eq!((width, height), (720, 64), "dock keeps its requested size");
    layer_surface.ack_configure(serial);

    // --- Buffer A: first frame ----------------------------------------------
    let buffer_a = create_buffer(&shm, &qh, &runtime_dir, 720, 64, "A");
    surface.attach(Some(&buffer_a.buffer), 0, 0);
    surface.damage(0, 0, 720, 64);
    surface.commit();
    queue.roundtrip(&mut state).expect("dock map");

    // --- Frame callback without damage --------------------------------------
    // The client paces itself on `wl_surface.frame`; nothing is damaged here, so
    // before P3-T13 the compositor would never present a frame and the callback
    // would never arrive.
    let _frame = surface.frame(&qh, ());
    surface.commit();
    wait_for(&mut state, &mut queue, Duration::from_secs(5), "frame callback", |state| {
        state.frame_done >= 1
    });

    // --- Buffer B replaces A → A must be released ----------------------------
    let buffer_b = create_buffer(&shm, &qh, &runtime_dir, 720, 64, "B");
    surface.attach(Some(&buffer_b.buffer), 0, 0);
    surface.damage(0, 0, 720, 64);
    surface.commit();
    wait_for(
        &mut state,
        &mut queue,
        Duration::from_secs(5),
        "wl_buffer.release for buffer A",
        |state| state.released.contains(&"A"),
    );
    assert_eq!(
        state.released,
        vec!["A"],
        "only the replaced buffer is released; B is still on screen"
    );

    // --- Injected pointer click on the dock ---------------------------------
    // The dock's output rect is (600,1016,720x64), so (600,1050) is local (0,34).
    wait_for(
        &mut state,
        &mut queue,
        Duration::from_secs(5),
        "wl_pointer enter",
        |state| !state.enters.is_empty(),
    );
    let (enter_x, enter_y) = state.enters[0];
    assert_eq!(
        (enter_x.round() as i32, enter_y.round() as i32),
        (0, 34),
        "pointer coordinates are surface-local: {:?}",
        state.enters
    );
    wait_for(
        &mut state,
        &mut queue,
        Duration::from_secs(5),
        "wl_pointer button",
        |state| state.buttons.len() >= 2,
    );
    assert_eq!(
        state.buttons,
        vec![(1, true), (1, false)],
        "the injected click must arrive as press+release"
    );
    // Clicking a keyboard_interactivity=none surface must not take the keyboard,
    // so the injected key (evdev 15) reports no focus.
    queue.roundtrip(&mut state).expect("settle");

    // Let the compositor stop on its own so the exit summaries are printed, then
    // grade the whole log (the FRAME_STATS counters only exist at shutdown).
    let log = compositor.finish();
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- compositor log ---\n{log}");
    }
    assert!(
        log.contains("AETHER_COMPOSITOR_POINTER_INJECTION: backend=headless x=600 y=1050 button=1"),
        "the pointer hook must announce its action:\n{log}"
    );
    assert!(
        log.contains(
            "AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=enter"
        ),
        "the dock must be hit with surface-local coordinates:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=button"),
        "the dock click must be logged:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_BUFFER_RELEASED: surface="),
        "buffer release must be logged:\n{log}"
    );
    assert!(
        log.contains("buffers_released=1"),
        "the frame summary must count the released buffer:\n{log}"
    );
    assert!(
        log.contains("AETHER_COMPOSITOR_KEY_INJECTED: evdev=15 focus=none"),
        "a keyboard-none dock must not take the keyboard:\n{log}"
    );
    assert!(
        !log.contains("event=leave"),
        "no leave marker is invented (leave is a wl_pointer event, not a marker):\n{log}"
    );

    let _ = (buffer_a, buffer_b, layer_surface, surface);
}

/// The compositor under test, killed and cleaned up on drop.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    fn start(keys: &str, pointer: &str) -> Self {
        let runtime_dir = std::env::temp_dir().join(format!(
            "aether-p3-t13-{}-{}",
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
            .env("AETHER_COMPOSITOR_INJECT_POINTER", pointer)
            .env("AETHER_COMPOSITOR_GRACE_MS", "20000")
            // Stop by itself a few seconds in, so the test can read the exit
            // markers (`FRAME_STATS … buffers_released=`).
            .env("AETHER_COMPOSITOR_RUN_SECS", "3")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn aether-compositor");

        Self::wait_for_socket(&runtime_dir, &mut child);
        Self { runtime_dir, child }
    }

    /// Wait for the compositor to stop on its own and return its log.
    fn finish(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        fs::read_to_string(self.runtime_dir.join("compositor.log")).expect("compositor log")
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
    label: &'static str,
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
    let shade = if label == "A" { 0x40 } else { 0x80 };
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = shade;
        chunk[1] = shade / 2;
        chunk[2] = 0x20;
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
        label,
    );
    ShmBuffer {
        _file: file,
        _pool: pool,
        buffer,
    }
}
