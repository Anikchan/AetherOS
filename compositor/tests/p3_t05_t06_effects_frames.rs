//! P3-T05 / P3-T06 headless acceptance test.
//!
//! Boots the real `aether-compositor` binary on a private `XDG_RUNTIME_DIR`,
//! maps **three** xdg-toplevels with damage, requests a `wl_surface.frame`
//! callback on each, and checks the compositor's markers:
//!
//! - `AETHER_COMPOSITOR_EFFECT_TIER` / `AETHER_COMPOSITOR_EFFECT_PIPELINE`
//!   reflect the tier requested through `AETHER_COMPOSITOR_EFFECT_TIER`;
//! - `AETHER_COMPOSITOR_GLASS_PLAN` reports three windows, the resolved pass
//!   count and the modelled frame cost against the 60 Hz budget;
//! - `AETHER_COMPOSITOR_FRAME` shows damage-based repaint (fewer pixels than a
//!   full repaint) and its `frost_px` field shows the glass chain is scoped to
//!   the damaged strip plus the blur halo, not the whole surface;
//! - `AETHER_COMPOSITOR_FRAME_CALLBACKS` answers the client's frame callbacks
//!   (the client asserts it received them);
//! - a run at the Minimal tier produces a shorter pipeline than Full, i.e. the
//!   tier switch really changes the effect plan.
//!
//! Needs no DRM, input, or GPU, so it runs in CI and on the macOS dev host.
//! The 60 fps number itself is modelled, not measured — see the hardware gap
//! noted in `src/glass.rs`.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

#[derive(Default)]
struct ClientState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    /// Serials of the xdg_surface configures, one per toplevel.
    configures: Vec<u32>,
    /// `wl_callback.done` events received.
    frame_callbacks: u32,
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
            _ => {
                let _ = version;
            }
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

impl Dispatch<wl_callback::WlCallback, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.frame_callbacks += 1;
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

/// A committed SHM buffer plus the file and pool that keep it valid.
struct ShmBuffer {
    _file: fs::File,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
}

#[test]
fn effect_tiers_and_damage_based_frames() {
    let full = run_scenario("full");
    let minimal = run_scenario("minimal");

    // --- P3-T05: tier switch is real ---------------------------------------
    assert!(
        full.contains("AETHER_COMPOSITOR_EFFECT_TIER: full"),
        "full tier marker missing:\n{full}"
    );
    assert!(
        full.contains("AETHER_COMPOSITOR_EFFECT_PIPELINE: tier=full")
            && full.contains("refraction=on"),
        "full tier pipeline missing:\n{full}"
    );
    assert!(
        minimal.contains("AETHER_COMPOSITOR_EFFECT_TIER: minimal"),
        "minimal tier marker missing:\n{minimal}"
    );
    assert!(
        minimal.contains("AETHER_COMPOSITOR_EFFECT_PIPELINE: tier=minimal")
            && minimal.contains("blur=none")
            && minimal.contains("refraction=off"),
        "minimal tier pipeline missing:\n{minimal}"
    );

    // --- P3-T05: three windows at Full tier fit the modelled budget --------
    let plan = marker_line(
        &full,
        &["AETHER_COMPOSITOR_GLASS_PLAN: tier=full", "windows=3"],
    )
        .unwrap_or_else(|| panic!("no 3-window glass plan:\n{full}"));
    assert!(
        plan.contains("within_budget=true"),
        "Full tier with three windows is over budget: {plan}"
    );
    let full_passes: usize = value_usize(&plan, "passes=");
    let minimal_plan = marker_line(
        &minimal,
        &["AETHER_COMPOSITOR_GLASS_PLAN: tier=minimal", "windows=3"],
    )
    .unwrap_or_else(|| panic!("no 3-window minimal plan:\n{minimal}"));
    let minimal_passes: usize = value_usize(&minimal_plan, "passes=");
    assert!(
        full_passes > minimal_passes,
        "full passes {full_passes} must exceed minimal passes {minimal_passes}"
    );
    assert!(
        minimal_plan.contains("within_budget=true"),
        "minimal tier over budget: {minimal_plan}"
    );

    // --- P3-T06: damage-based repaint + frame callbacks --------------------
    let frame = marker_line(&full, &["AETHER_COMPOSITOR_FRAME: n=", "windows=3"])
        .unwrap_or_else(|| panic!("no frame marker:\n{full}"));
    assert!(
        frame.contains("windows=3"),
        "frame should compose three windows: {frame}"
    );
    let damage_px: u64 = value_usize(&frame, "damage_px=") as u64;
    let covered_px: u64 = value_usize(&plan, "covered_px=") as u64;
    assert!(
        damage_px > 0 && damage_px < covered_px,
        "frame damage {damage_px} should be a subset of the {covered_px} covered pixels"
    );
    assert!(
        full.contains("AETHER_COMPOSITOR_FRAME_CALLBACKS: n="),
        "no frame callback marker:\n{full}"
    );
    assert!(
        full.contains("idle_pct="),
        "no idle measurement in the frame stats:\n{full}"
    );

    // --- P3-T06: the frost only rewrites the damaged strip ----------------
    // Three 1152x648 glass windows, 320x100 damaged each: a whole-surface frost
    // would rewrite `covered_px`; the damage-scoped one adds only the 48 px blur
    // halo (24 px radius + 24 px refraction band) around each strip.
    let frost_px: u64 = value_usize(frame, "frost_px=") as u64;
    assert!(
        frost_px >= damage_px,
        "frost {frost_px} must at least cover the {damage_px} damaged pixels"
    );
    assert!(
        frost_px * 2 < covered_px,
        "frost {frost_px} should stay far below a full repaint of {covered_px} px: {frame}"
    );

    // Minimal has no blur, so its frost region is exactly the damaged strips.
    let minimal_frame = marker_line(&minimal, &["AETHER_COMPOSITOR_FRAME: n=", "windows=3"])
        .unwrap_or_else(|| panic!("no minimal frame marker:\n{minimal}"));
    let minimal_damage: u64 = value_usize(minimal_frame, "damage_px=") as u64;
    let minimal_frost: u64 = value_usize(minimal_frame, "frost_px=") as u64;
    assert_eq!(
        minimal_frost,
        minimal_damage * 3,
        "minimal tier frost must be the three damaged strips: {minimal_frame}"
    );
}

/// Run the compositor at `tier`, map three toplevels with damage and frame
/// callbacks, then return its log. Panics if the client could not connect.
fn run_scenario(tier: &str) -> String {
    let compositor = CompositorProcess::start(tier);
    let runtime_dir = compositor.runtime_dir.clone();

    std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
    std::env::set_var("WAYLAND_DISPLAY", "wayland-0");

    let mut state = ClientState::default();
    let conn = Connection::connect_to_env().expect("connect to compositor socket");
    let mut queue = conn.new_event_queue::<ClientState>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state).expect("registry roundtrip");

    let compositor_proxy = state
        .compositor
        .clone()
        .expect("compositor advertises wl_compositor");
    let shm = state.shm.clone().expect("compositor advertises wl_shm");
    let wm_base = state
        .wm_base
        .clone()
        .expect("compositor advertises xdg_wm_base");

    let mut buffers = Vec::new();
    for index in 0..3 {
        let surface = compositor_proxy.create_surface(&qh, ());
        let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        toplevel.set_title(format!("p3-t05-window-{index}"));
        // P3-T06: ask for a frame callback before the first commit so the next
        // presented frame answers it.
        let _callback = surface.frame(&qh, ());
        surface.commit();
        queue.roundtrip(&mut state).expect("toplevel configure");
        queue.roundtrip(&mut state).expect("toplevel configure");
        let serial = state
            .configures
            .pop()
            .expect("xdg_surface configure per toplevel");
        xdg.ack_configure(serial);

        let buffer = create_buffer(&shm, &qh, &runtime_dir, 320, 200, index);
        surface.attach(Some(&buffer.buffer), 0, 0);
        // Explicit partial damage: the top half of the window.
        surface.damage(0, 0, 320, 100);
        surface.commit();
        queue.roundtrip(&mut state).expect("toplevel map");
        buffers.push(buffer);
    }

    // Let the frame scheduler present a few frames and answer the callbacks.
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.frame_callbacks < 3 && Instant::now() < deadline {
        queue
            .roundtrip(&mut state)
            .expect("frame callback roundtrip");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        state.frame_callbacks >= 3,
        "compositor answered {} of 3 frame callbacks",
        state.frame_callbacks
    );

    let log = fs::read_to_string(runtime_dir.join("compositor.log")).expect("compositor log");
    if std::env::var_os("AETHER_COMPOSITOR_TEST_LOG").is_some() {
        println!("--- compositor log ({tier}) ---\n{log}");
    }
    log
}

/// First log line containing every needle.
fn marker_line<'a>(log: &'a str, needles: &[&str]) -> Option<&'a str> {
    log.lines()
        .find(|line| needles.iter().all(|needle| line.contains(needle)))
}

fn value_usize(line: &str, key: &str) -> usize {
    let rest = &line[line.find(key).unwrap_or_else(|| panic!("{key} in {line}")) + key.len()..];
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or_else(|_| panic!("no number after {key} in {line}"))
}

/// The compositor under test, killed and cleaned up on drop.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    fn start(tier: &str) -> Self {
        let runtime_dir = std::env::temp_dir().join(format!(
            "aether-p3-t05-{}-{tier}",
            std::process::id()
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
            .env("AETHER_COMPOSITOR_EFFECT_TIER", tier)
            .env("AETHER_COMPOSITOR_GRACE_MS", "15000")
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
    index: usize,
) -> ShmBuffer {
    use std::os::fd::AsFd;

    let len = (width * height * 4) as usize;
    let path = runtime_dir.join(format!("shm-{index}-{width}x{height}.raw"));
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .expect("create shm file");
    let mut pixels = vec![0u8; len];
    for (pixel, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        let shade = ((pixel / 7 + index * 40) % 200) as u8;
        chunk[0] = shade;
        chunk[1] = shade / 2;
        chunk[2] = 0x40;
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
