//! End-to-end layer-shell handshake against the real compositor (P3-T02 → P4).
//!
//! Boots the shipped `aether-compositor` on its headless backend (the same
//! private-runtime-dir pattern as `compositor/tests/p3_t02_headless.rs`), then
//! runs the shipped `aether-shell --connect` against the resulting
//! `WAYLAND_DISPLAY`. No fake log: the shell creates its five layer surfaces
//! over the socket, receives their configure events, acks them, attaches an SHM
//! buffer for each, and the compositor's own markers are read back.
//!
//! Asserted in both directions:
//! * shell → compositor: `AETHER_COMPOSITOR_LAYER_SURFACE` for every planned
//!   namespace, in plan order, with `AETHER_COMPOSITOR_LAYER_CONFIGURE` at the
//!   size the shell plans for a 1920×1080 logical output, plus
//!   `AETHER_COMPOSITOR_CLIENT_MAPPED` once the buffers are attached;
//! * compositor → shell: `AETHER_SHELL_LAYER_CONFIGURE` per surface and the
//!   `AETHER_SHELL_LAYER_HOST_OK` verdict;
//! * contract: [`BindingReport::scan`] — the code behind
//!   `aether-shell --host-check` — accepts the real compositor journal, which
//!   is what the on-device acceptance harness runs.
//!
//! `cargo build -p aether-compositor` provides the binary; a `cargo test
//! --workspace` run builds it anyway. When it is absent the test prints
//! `AETHER_SHELL_HANDSHAKE_SKIPPED` instead of failing, so `cargo test -p
//! aether-shell` stays green on its own.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aether_shell::host::{plan, BindingReport};

/// The compositor binary next to `aether-shell` (same `target/<profile>`), or
/// `$AETHER_COMPOSITOR_BIN`.
fn compositor_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("AETHER_COMPOSITOR_BIN") {
        let path = PathBuf::from(path);
        return path.exists().then_some(path);
    }
    let shell = PathBuf::from(env!("CARGO_BIN_EXE_aether-shell"));
    let candidate = shell.parent()?.join("aether-compositor");
    candidate.exists().then_some(candidate)
}

fn temp_dir(tag: &str) -> PathBuf {
    // Short on purpose: the runtime dir holds the `wayland-0` socket and a unix
    // socket path must stay under SUN_LEN (104 bytes) even under a long TMPDIR.
    std::env::temp_dir().join(format!("aether-hs-{tag}-{}", std::process::id()))
}

/// Serialises the tests in this binary.
///
/// Every test boots its own compositor and shell on its own runtime dir, so the
/// environment does not collide — but they all compete for the CPU and for the
/// compositor's 10 ms frame clock, and on a loaded machine the injected
/// pointer/key replay can land after a short `--hold-ms` has expired. Running
/// them one at a time removes that timing dependency (and the shared
/// process-wide working directory of the `--frames-out` dumps).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Hold the serial guard for the whole test body.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The compositor under test, killed and cleaned up on drop — even when an
/// assertion panics.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    fn start(binary: &Path, tag: &str) -> Self {
        Self::start_with(binary, tag, &[])
    }

    /// Start the compositor with extra environment (test hooks such as
    /// `AETHER_COMPOSITOR_INJECT_KEYS`).
    fn start_with(binary: &Path, tag: &str, env: &[(&str, &str)]) -> Self {
        let runtime_dir = temp_dir(tag);
        let _ = fs::remove_dir_all(&runtime_dir);
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
            .expect("chmod runtime dir");

        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("create log");
        let err = fs::File::create(runtime_dir.join("compositor.err")).expect("create stderr log");
        let mut command = Command::new(binary);
        let child = command
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("AETHER_COMPOSITOR_BACKEND", "headless")
            // The default grace (3 s after the first mapped surface) is shorter
            // than a slow test run; the shell needs the socket to stay up.
            .env("AETHER_COMPOSITOR_GRACE_MS", "20000");
        // Test overrides last, so a test can shorten the grace to let the
        // compositor shut down and print its frame statistics.
        for (key, value) in env {
            child.env(key, value);
        }
        let child = child
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err))
            .spawn()
            .expect("spawn aether-compositor");

        let mut process = CompositorProcess { runtime_dir, child };
        process.wait_for_socket();
        process
    }

    fn wait_for_socket(&mut self) {
        let socket = self.runtime_dir.join("wayland-0");
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if socket.exists() {
                return;
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!(
                    "compositor exited before creating its socket: {status}\n{}",
                    self.stderr()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("compositor did not create {} in time", socket.display());
    }

    fn log(&self) -> String {
        fs::read_to_string(self.runtime_dir.join("compositor.log")).unwrap_or_default()
    }

    /// Wait for the compositor to exit on its own (`AETHER_COMPOSITOR_GRACE_MS`)
    /// so its shutdown line — `AETHER_COMPOSITOR_FRAME_STATS … buffers_released=` —
    /// reaches the journal before it is read.
    fn wait_for_shutdown(&mut self, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => panic!("wait for the compositor: {error}"),
            }
        }
        self.log()
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.runtime_dir.join("compositor.err")).unwrap_or_default()
    }
}

impl Drop for CompositorProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

/// Run the shipped shell host against `runtime_dir`/`wayland-0`, dumping one
/// PNG per painted surface into `shots`.
fn connect(runtime_dir: &Path, shots: &Path, act: Option<&str>) -> std::process::Output {
    connect_for(runtime_dir, shots, act, 300)
}

/// Same, with an explicit hold: the injected-key test needs time for the
/// compositor to replay its keys.
fn connect_for(
    runtime_dir: &Path,
    shots: &Path,
    act: Option<&str>,
    hold_ms: u64,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aether-shell"));
    command.args([
        "--connect",
        "--hold-ms",
        &hold_ms.to_string(),
        "--frames-out",
        shots.to_str().unwrap(),
    ]);
    if let Some(panel) = act {
        command.args(["--act", panel]);
    }
    command
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("WAYLAND_DISPLAY", "wayland-0")
        .output()
        .expect("run aether-shell --connect")
}

/// Decode a PNG dumped by `--frames-out` into RGB pixels.
fn read_png(path: &Path) -> (u32, u32, Vec<[u8; 3]>) {
    let file =
        fs::File::open(path).unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().expect("png info");
    let mut buffer = vec![0; reader.output_buffer_size().expect("png buffer size")];
    let info = reader.next_frame(&mut buffer).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgb, "RGB evidence");
    let pixels = buffer[..info.buffer_size()]
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();
    (info.width, info.height, pixels)
}

fn pixel(pixels: &[[u8; 3]], width: u32, x: u32, y: u32) -> [u8; 3] {
    pixels[(y * width + x) as usize]
}

fn near(pixel: [u8; 3], expected: [u8; 3], tolerance: i16) -> bool {
    pixel
        .iter()
        .zip(expected.iter())
        .all(|(value, expected)| (*value as i16 - *expected as i16).abs() <= tolerance)
}

#[test]
fn the_shell_creates_every_layer_surface_on_a_real_compositor() {
    let _serial = serial();
    let Some(binary) = compositor_binary() else {
        println!(
            "AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary next to {}; \
             run `cargo build -p aether-compositor` or set AETHER_COMPOSITOR_BIN",
            env!("CARGO_BIN_EXE_aether-shell")
        );
        return;
    };
    let compositor = CompositorProcess::start(&binary, "c");
    let shots = temp_dir("shots");
    let _ = fs::remove_dir_all(&shots);
    let output = connect(&compositor.runtime_dir, &shots, None);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let journal = compositor.log();

    if std::env::var_os("AETHER_SHELL_TEST_LOG").is_some() {
        println!("--- aether-shell --connect ---\n{stdout}\n{stderr}");
        println!("--- compositor journal ---\n{journal}");
    }

    assert!(
        output.status.success(),
        "the shell host failed (status {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}\n\
         compositor journal:\n{journal}",
        output.status.code()
    );
    assert!(
        stdout.contains("AETHER_SHELL_LAYER_HOST_OK\n"),
        "every surface must bind and be configured:\n{stdout}"
    );

    // --- Shell → compositor: the plan, in order, and what exists of it -----
    let planned = plan();
    assert_eq!(
        planned.len(),
        9,
        "six chrome surfaces + three lazy overlays"
    );
    let mut last_line = 0usize;
    for request in &planned {
        // Every planned surface is reported, created or not.
        let planned_line = format!(
            "AETHER_SHELL_LAYER_PLANNED: name={} namespace={} layer={} lazy={} created={}\n",
            request.name, request.namespace, request.layer_name, request.lazy, !request.lazy
        );
        let line = stdout
            .lines()
            .position(|line| line == planned_line.trim_end())
            .unwrap_or_else(|| panic!("no plan report for {}\n{stdout}", request.name));
        assert!(
            line > last_line,
            "surfaces are reported in plan order: {} came too early\n{stdout}",
            request.name
        );
        last_line = line;

        if request.lazy {
            // Created on demand: absent from a boot journal, and that is fine.
            assert!(
                !stdout.contains(&format!(
                    "AETHER_SHELL_LAYER_CREATED: name={} ",
                    request.name
                )),
                "{} is lazy and must not exist at boot\n{stdout}",
                request.name
            );
            assert!(
                !journal.contains(&format!("namespace={} ", request.namespace))
                    || !journal.contains("AETHER_COMPOSITOR_LAYER_SURFACE"),
                "the compositor never saw the lazily created {}\n{journal}",
                request.name
            );
            continue;
        }

        let created = format!(
            "AETHER_SHELL_LAYER_CREATED: name={} namespace={} layer={} anchors={} ",
            request.name, request.namespace, request.layer_name, request.anchors
        );
        assert!(
            stdout.contains(&created),
            "no host report for {}\n{stdout}",
            request.name
        );

        // Anchors, exclusive zone and keyboard interactivity reach the socket
        // exactly as the plan asks, and the configure carries a real size.
        let expected = request.negotiate((1920, 1080));
        let configure = format!(
            "AETHER_SHELL_LAYER_CONFIGURE: name={} namespace={} configured={}x{} serials=",
            request.name, request.namespace, expected.0, expected.1
        );
        let line = stdout
            .lines()
            .find(|line| line.starts_with(&configure))
            .unwrap_or_else(|| {
                panic!(
                    "{} was not configured at {}x{}\n{stdout}",
                    request.name, expected.0, expected.1
                )
            });
        let serials: usize = line
            .rsplit('=')
            .next()
            .and_then(|value| value.parse().ok())
            .expect("serials count");
        assert!(serials > 0, "{} received no configure event", request.name);
        assert!(
            stdout.contains(&format!(
                " exclusive_zone={} margin_top={} keyboard={}\n",
                request.exclusive_zone, request.margin_top, request.keyboard_interactivity
            )),
            "{} lost its exclusive zone, margin or keyboard mode\n{stdout}",
            request.name
        );
    }

    // --- Compositor's own view of the same five surfaces -------------------
    let surface_lines: Vec<&str> = journal
        .lines()
        .filter(|line| line.starts_with("AETHER_COMPOSITOR_LAYER_SURFACE: "))
        .collect();
    let eager: Vec<_> = planned.iter().filter(|request| !request.lazy).collect();
    assert_eq!(
        surface_lines.len(),
        eager.len(),
        "the compositor created one layer surface per eager plan entry:\n{journal}"
    );
    for (line, request) in surface_lines.iter().zip(&eager) {
        assert!(
            line.contains(&format!("namespace={}", request.namespace)),
            "creation order drifted: expected {} at {line}",
            request.namespace
        );
        assert!(
            line.contains(&format!("layer={}", request.layer_name)),
            "{} landed on the wrong layer: {line}",
            request.name
        );
    }
    for request in &eager {
        let expected = request.negotiate((1920, 1080));
        assert!(
            journal.contains(&format!(
                "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace={} size={}x{}",
                request.namespace, expected.0, expected.1
            )),
            "{} was not configured at {}x{}:\n{journal}",
            request.name,
            expected.0,
            expected.1
        );
    }
    assert!(
        journal.contains("AETHER_COMPOSITOR_OUTPUT: name=aether-headless-0"),
        "the headless primary output exists:\n{journal}"
    );
    assert!(
        journal.contains("AETHER_COMPOSITOR_CLIENT_MAPPED"),
        "the shell's buffers mapped:\n{journal}"
    );

    // --- The acceptance contract accepts the real journal ------------------
    let report = BindingReport::scan(&journal);
    assert!(
        report.is_complete(),
        "--host-check must accept the real compositor journal: {:?}",
        report.problems()
    );
    assert_eq!(
        report.output.as_ref().map(|output| output.logical_size),
        Some((1920, 1080))
    );

    // The keyboard is absent on the headless backend (no input devices); the
    // host says so instead of pretending. The on-device DRM backend adds the
    // seat, and `shell/src/keys.rs` takes over from the first key event.
    assert!(
        stdout.contains("seat=none") || stdout.contains("seat=keyboard"),
        "the host reports the seat it found:\n{stdout}"
    );

    // --- Painting: the panels are mapped, the closed ones are not ----------
    let open = |name: &str, expected: bool| {
        let marker = format!("AETHER_SHELL_LAYER_PAINT: name={name} mapped={expected} ");
        assert!(
            stdout.contains(&marker),
            "{name} must be mapped={expected}:\n{stdout}"
        );
    };
    open("background", true);
    open("menubar", true);
    open("dock", true);
    open("launcher", false);
    open("notifications", false);
    open("control-center", false);
    assert!(
        stdout.contains(
            "AETHER_SHELL_LAYER_HOST_READY: planned=9 created=6 configured=6 mapped=3 painted=3"
        ),
        "the wallpaper and two panels are up on a fresh session:\n{stdout}"
    );
    // The three full-screen overlays are lazy: no surface, no pixels.
    for name in ["settings", "overview", "lock"] {
        assert!(
            !stdout.contains(&format!("AETHER_SHELL_LAYER_CREATED: name={name} ")),
            "{name} must not exist until it opens:\n{stdout}"
        );
        assert!(
            !shots.join(format!("layer-{name}.png")).exists(),
            "a closed {name} must not be painted"
        );
    }
    // The background is on the Background layer, covers the output and reserves
    // nothing (exclusive zone -1).
    assert!(
        stdout.contains(
            "AETHER_SHELL_LAYER_CREATED: name=background namespace=aether-shell-background \
             layer=Background anchors=15 size=1920x1080 exclusive_zone=-1 margin_top=0 keyboard=0"
        ),
        "the wallpaper surface is a full-output Background surface:\n{stdout}"
    );
    assert!(
        journal.contains(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-background layer=Background"
        ),
        "the compositor created the background layer surface:\n{journal}"
    );
    assert!(
        journal.contains(
            "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-background size=1920x1080"
        ),
        "the compositor configured the wallpaper at the output size:\n{journal}"
    );
    // The closed launcher has no surface pixels at all.
    assert!(
        !shots.join("layer-launcher.png").exists(),
        "a closed launcher must not be painted"
    );
    assert!(
        !shots.join("layer-notifications.png").exists(),
        "a closed notification column must not be painted"
    );
    assert!(
        !shots.join("layer-notifications.png").exists(),
        "a closed notification column must not be painted"
    );

    // --- The pixels the compositor receives -------------------------------
    //
    // The headless backend has no rasteriser of its own, so these assertions
    // are on the exact SHM bytes the shell handed to `wl_surface.attach` (the
    // PNG `--frames-out` dumps come from the same buffer) plus the compositor's
    // own placement/damage markers below.
    let (menubar_width, menubar_height, menubar) = read_png(&shots.join("layer-menubar.png"));
    assert_eq!(
        (menubar_width, menubar_height),
        (1920, 28),
        "the menu-bar buffer is its configured logical size"
    );
    // `Tokens.background` #0E1116 at 86 % alpha, premultiplied: the glass tint,
    // not the palette fallback and not the wallpaper.
    let mut tint = 0;
    let mut sampled = 0;
    for x in (0..menubar_width).step_by(7) {
        for y in (0..menubar_height).step_by(2) {
            sampled += 1;
            if near(pixel(&menubar, menubar_width, x, y), [12, 14, 18], 3) {
                tint += 1;
            }
        }
    }
    assert!(
        tint * 2 > sampled,
        "the menu bar is its own rendered glass strip ({tint}/{sampled} tinted pixels)"
    );
    // The AetherOS logo sits at x=8 with the accent gradient: a blue-dominant
    // pixel must exist in the left 40 px, and nowhere near the right edge.
    let accent_like =
        |pixel: [u8; 3]| pixel[2] > 230 && pixel[2] as i16 > pixel[0] as i16 + 60 && pixel[1] > 90;
    let accent_left =
        (0..40).any(|x| (4..24).any(|y| accent_like(pixel(&menubar, menubar_width, x, y))));
    assert!(
        accent_left,
        "the accent logo is painted at the menu-bar left edge"
    );
    let accent_right = (menubar_width - 40..menubar_width)
        .any(|x| (4..24).any(|y| accent_like(pixel(&menubar, menubar_width, x, y))));
    assert!(!accent_right, "the accent logo is only on the left");

    // The wallpaper: opaque everywhere (no transparent holes for the compositor
    // to show its clear colour through) and not the panel tint. The alpha is
    // asserted on the exact ARGB bytes in `paint`'s unit test; the PNG dump
    // carries the colour, so a transparent pixel would read as 0,0,0 here.
    let (bg_width, bg_height, background) = read_png(&shots.join("layer-background.png"));
    assert_eq!((bg_width, bg_height), (1920, 1080));
    let holes = background
        .iter()
        .filter(|pixel| **pixel == [0, 0, 0])
        .count();
    assert_eq!(holes, 0, "the wallpaper is opaque everywhere");
    let tint_pixels = background
        .iter()
        .filter(|pixel| near(**pixel, [12, 14, 18], 3))
        .count();
    assert!(
        tint_pixels * 10 < background.len(),
        "the wallpaper is not the menu-bar tint ({tint_pixels} tinted pixels)"
    );
    // It is the design gradient: dark blue, with the glows lifting the middle.
    let corner = pixel(&background, bg_width, 0, 0);
    let centre = pixel(&background, bg_width, bg_width / 2, bg_height / 2);
    assert!(
        corner[2] > corner[0],
        "the top-left is blue-dominant: {corner:?}"
    );
    assert_ne!(corner, centre, "a gradient, not a flat fill");
    assert_eq!(
        pixel(&menubar, menubar_width, 100, 14),
        pixel(&menubar, menubar_width, 100, 14),
        "sanity: the panel sample is stable"
    );
    assert_ne!(
        centre,
        pixel(&menubar, menubar_width, 100, 14),
        "the wallpaper differs from the panel strip"
    );

    let (dock_width, dock_height, dock) = read_png(&shots.join("layer-dock.png"));
    assert_eq!((dock_width, dock_height), (720, 64));
    // The dock panel is centred in its 720 px surface (four pinned apps make it
    // 270 px wide), so the surface is transparent around it and the panel body
    // is `Tokens.surface` #1A1F27 at 82 % alpha (premultiplied: 21,25,31).
    let tinted = dock
        .iter()
        .filter(|pixel| near(**pixel, [21, 25, 31], 3))
        .count();
    assert!(
        tinted > 3000,
        "the dock paints its glass panel ({tinted} tinted pixels)"
    );
    assert!(
        near(
            pixel(&dock, dock_width, dock_width / 2, dock_height / 2),
            [21, 25, 31],
            3
        ),
        "the dock's centre is its glass tint, got {:?}",
        pixel(&dock, dock_width, dock_width / 2, dock_height / 2)
    );
    assert_eq!(
        pixel(&dock, dock_width, dock_width / 8, dock_height / 2),
        [0, 0, 0],
        "the surface is transparent where the panel does not reach"
    );
    // Rounded corners are transparent (the compositor shows the desktop), which
    // is only possible because the scene is painted with a transparent
    // background rather than the palette fallback.
    assert_eq!(
        pixel(&dock, dock_width, 1, 1),
        [0, 0, 0],
        "the dock's rounded corner is transparent"
    );
    // ... and it is not a flat block: the app icons and labels paint pixels
    // brighter than the tint.
    let bright = dock
        .iter()
        .filter(|pixel| pixel[0] > 120 && pixel[1] > 120 && pixel[2] > 120)
        .count();
    assert!(
        bright > 100,
        "the dock draws its app icons and labels ({bright} bright pixels)"
    );

    // The compositor shows the menu bar and the dock exactly where the shell
    // puts them: its own layer-damage placement has to match `placement()`.
    for name in ["menubar", "dock"] {
        let request = planned.iter().find(|request| request.name == name).unwrap();
        let (x, y) = request.placement((1920, 1080));
        let (width, height) = request.negotiate((1920, 1080));
        assert!(
            journal.contains(&format!(
                "AETHER_COMPOSITOR_LAYER_DAMAGE: namespace={} placement={x},{y} local=0,0,{}x{}",
                request.namespace, width, height
            )),
            "the compositor placed {name} at {x},{y} {}x{}:\n{journal}",
            width,
            height
        );
    }
    // Attaching the buffers made the compositor present frames.
    assert!(
        journal.contains("AETHER_COMPOSITOR_FRAME:"),
        "the compositor presented a frame with the shell's pixels:\n{journal}"
    );
    assert!(
        stdout.contains("AETHER_SHELL_LAYER_RUN: paints="),
        "the host reports its paint/commit counters:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&shots);
}

/// An opened panel is mapped, painted at its own placement, and the chrome
/// around it keeps painting: the launcher overlay is the strongest case because
/// it is an Overlay-layer surface with exclusive keyboard interactivity.
#[test]
fn an_opened_panel_is_mapped_painted_and_placed() {
    let _serial = serial();
    let Some(binary) = compositor_binary() else {
        println!("AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary");
        return;
    };
    let compositor = CompositorProcess::start(&binary, "open");
    let shots = temp_dir("shots-open");
    let _ = fs::remove_dir_all(&shots);
    let output = connect(&compositor.runtime_dir, &shots, Some("launcher"));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let journal = compositor.log();
    assert!(
        output.status.success(),
        "the shell host failed\nstdout:\n{stdout}\njournal:\n{journal}"
    );

    assert!(
        stdout.contains("AETHER_SHELL_LAYER_PAINT: name=launcher mapped=true buffer=640x420"),
        "the opened launcher is mapped and painted:\n{stdout}"
    );
    assert!(
        stdout.contains("AETHER_SHELL_LAYER_PAINT: name=background mapped=true buffer=1920x1080"),
        "the wallpaper stays mapped under the overlay:\n{stdout}"
    );
    assert!(
        stdout.contains("AETHER_SHELL_LAYER_PAINT: name=menubar mapped=true"),
        "the menu bar keeps painting under the overlay:\n{stdout}"
    );

    let path = shots.join("layer-launcher.png");
    assert!(path.exists(), "the launcher surface has pixels");
    let (width, height, launcher) = read_png(&path);
    assert_eq!((width, height), (640, 420));
    // `Tokens.surface` #1A1F27 at 94 % alpha (premultiplied: 24,29,37) is the
    // launcher panel; the search field and rows paint on top.
    let tinted = launcher
        .iter()
        .filter(|pixel| near(**pixel, [24, 29, 37], 4))
        .count();
    assert!(
        tinted > 2000,
        "the launcher panel is painted ({tinted} tinted pixels)"
    );
    assert_eq!(
        pixel(&launcher, width, 1, 1),
        [0, 0, 0],
        "the launcher's rounded corner is transparent"
    );

    // The compositor placed it where the plan says.
    let request = plan()
        .into_iter()
        .find(|request| request.name == "launcher")
        .unwrap();
    let (x, y) = request.placement((1920, 1080));
    let (panel_width, panel_height) = request.negotiate((1920, 1080));
    assert!(
        journal.contains(&format!(
            "AETHER_COMPOSITOR_LAYER_DAMAGE: namespace={} placement={x},{y} local=0,0,{}x{}",
            request.namespace, panel_width, panel_height
        )),
        "the compositor placed the launcher at {x},{y}:\n{journal}"
    );
    let _ = fs::remove_dir_all(&shots);
}

/// A full-screen overlay is created lazily and painted over the wallpaper: the
/// Settings surface only exists once Settings opens, it covers the output, it is
/// opaque everywhere and its pixels are not the wallpaper.
#[test]
fn an_opened_overlay_is_created_and_painted_over_the_wallpaper() {
    let _serial = serial();
    let Some(binary) = compositor_binary() else {
        println!("AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary");
        return;
    };
    let compositor = CompositorProcess::start(&binary, "overlay");
    let shots = temp_dir("shots-overlay");
    let _ = fs::remove_dir_all(&shots);
    let output = connect_for(&compositor.runtime_dir, &shots, Some("settings"), 600);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let journal = compositor.log();
    assert!(
        output.status.success(),
        "the shell host failed\nstdout:\n{stdout}\njournal:\n{journal}"
    );

    // The overlay exists now: Overlay layer, full output, exclusive keyboard.
    assert!(
        stdout.contains(
            "AETHER_SHELL_LAYER_CREATED: name=settings namespace=aether-shell-settings \
             layer=Overlay anchors=15 size=1920x1080 exclusive_zone=-1 margin_top=0 keyboard=1"
        ),
        "the settings overlay is a full-screen exclusive surface:\n{stdout}"
    );
    assert!(
        journal.contains(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-settings layer=Overlay"
        ),
        "the compositor created the overlay surface:\n{journal}"
    );
    assert!(
        stdout.contains(
            "AETHER_SHELL_LAYER_HOST_READY: planned=9 created=7 configured=7 mapped=4 painted=4"
        ),
        "the wallpaper, two panels and the overlay are up:\n{stdout}"
    );

    // Its pixels: opaque over the whole frame, and not the wallpaper.
    let (width, height, overlay) = read_png(&shots.join("layer-settings.png"));
    assert_eq!((width, height), (1920, 1080));
    assert!(
        !overlay.iter().any(|pixel| *pixel == [0, 0, 0]),
        "the overlay is opaque everywhere"
    );
    let (bg_width, _, background) = read_png(&shots.join("layer-background.png"));
    assert_eq!(bg_width, 1920);
    let differs = overlay
        .iter()
        .zip(background.iter())
        .filter(|(overlay, background)| overlay != background)
        .count();
    assert!(
        differs * 2 > overlay.len(),
        "the overlay differs from the wallpaper at most pixels ({differs}/{})",
        overlay.len()
    );

    let _ = fs::remove_dir_all(&shots);
}

/// The socket keyboard path, end to end: the compositor's test hook replays
/// evdev 15, 15, 28 (Tab, Tab, Enter) into whatever surface holds the keyboard.
/// The launcher overlay takes `exclusive` keyboard interactivity, so it is the
/// surface that receives them; the shell must report the evdev code unshifted,
/// move its focus ring twice, then activate.
///
/// This is also the regression test for the off-by-8: `wl_keyboard.key.key` is
/// the evdev code, so injecting 15 must log `evdev=15`, never `evdev=7`.
#[test]
fn injected_keys_reach_the_shell_and_move_the_ring() {
    let _serial = serial();
    let Some(binary) = compositor_binary() else {
        println!("AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary");
        return;
    };
    let compositor = CompositorProcess::start_with(
        &binary,
        "handshake-keys",
        &[("AETHER_COMPOSITOR_INJECT_KEYS", "15,15,28")],
    );
    let shots = temp_dir("shots-keys");
    let _ = fs::remove_dir_all(&shots);
    let output = connect_for(&compositor.runtime_dir, &shots, Some("launcher"), 2500);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let journal = compositor.log();
    if std::env::var_os("AETHER_SHELL_TEST_LOG").is_some() {
        println!("--- aether-shell --connect ---\n{stdout}\n{stderr}");
        println!("--- compositor journal ---\n{journal}");
    }
    assert!(
        output.status.success(),
        "the shell host failed\nstdout:\n{stdout}\njournal:\n{journal}"
    );

    // The seat exists (created by the injection hook) with a keyboard and a
    // pointer: this is the shell's `wl_pointer` binding over a real socket.
    assert!(
        stdout.contains("seat=keyboard+pointer"),
        "the host binds both seat capabilities:\n{stdout}"
    );

    // Three keys, in order, each attributed to the surface that received it.
    let keys: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("AETHER_SHELL_KEY: "))
        .collect();
    assert_eq!(
        keys.len(),
        3,
        "three keys were injected and handled:\n{stdout}"
    );
    assert_eq!(
        keys[0],
        "AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true \
         focus=aether-shell-launcher ring=control-center"
    );
    assert_eq!(
        keys[1],
        "AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true \
         focus=aether-shell-launcher ring=notifications"
    );
    assert_eq!(
        keys[2],
        "AETHER_SHELL_KEY: evdev=28 keysym=0xff0d binding=enter handled=true \
         focus=aether-shell-launcher ring=notifications"
    );
    // The ring moved on both Tabs, then Enter activated the focused item.
    assert!(
        stdout.contains("AETHER_SHELL_ACTIVATE: surface=notifications item=0"),
        "Enter activated the focused item:\n{stdout}"
    );
    // The compositor focused the launcher, and our marker says the same.
    assert!(
        journal.contains(
            "AETHER_COMPOSITOR_SEAT_FOCUS: surface=exclusive layer=aether-shell-launcher"
        ),
        "the exclusive launcher took the keyboard:\n{journal}"
    );
    for evdev in [15, 15, 28] {
        assert!(
            journal.contains(&format!(
                "AETHER_COMPOSITOR_KEY_INJECTED: evdev={evdev} focus=surface"
            )),
            "evdev {evdev} was injected into the focused surface:\n{journal}"
        );
    }
    // Off-by-8 guard: evdev 15 read as an XKB-minus-8 code would log 7.
    assert!(!stdout.contains("evdev=7"), "{stdout}");
    assert!(!stdout.contains("evdev=20"), "{stdout}");

    let _ = fs::remove_dir_all(&shots);
}

/// The socket pointer path, end to end: the compositor's test hook moves the
/// pointer into the dock tile and clicks it, the shell routes the event into the
/// scene, and the tile's handler activates the app.
///
/// The click lands at output (861, 1048), i.e. surface-local (261, 32) on the
/// dock surface (placed at 600,1016) — the centre of the first pinned tile.
#[test]
fn an_injected_click_on_the_dock_activates_its_tile() {
    let _serial = serial();
    let Some(binary) = compositor_binary() else {
        println!("AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary");
        return;
    };
    let compositor = CompositorProcess::start_with(
        &binary,
        "ptr",
        &[("AETHER_COMPOSITOR_INJECT_POINTER", "x=861,y=1048,button=1")],
    );
    let shots = temp_dir("shots-pointer");
    let _ = fs::remove_dir_all(&shots);
    let output = connect_for(&compositor.runtime_dir, &shots, None, 3000);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let journal = compositor.log();
    if std::env::var_os("AETHER_SHELL_TEST_LOG").is_some() {
        println!("--- aether-shell --connect ---\n{stdout}\n{stderr}");
        println!("--- compositor journal ---\n{journal}");
    }
    assert!(
        output.status.success(),
        "the shell host failed\nstdout:\n{stdout}\njournal:\n{journal}"
    );
    assert!(
        stdout.contains("seat=keyboard+pointer"),
        "the seat advertises a pointer and the shell binds it:\n{stdout}"
    );

    // The compositor delivered the click to the dock surface at the tile.
    for marker in [
        "AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=261 y=32 event=enter",
        "AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=261 y=32 event=button",
    ] {
        assert!(
            journal.contains(marker),
            "the compositor delivered {marker}:\n{journal}"
        );
    }
    assert!(
        journal.contains(
            "AETHER_COMPOSITOR_POINTER_INJECTION: backend=headless x=861 y=1048 button=1"
        ),
        "the injection hook ran:\n{journal}"
    );

    // The shell saw the pointer on the dock, in surface-local coordinates. The
    // button code is whatever the compositor put on the wire: the protocol's
    // BTN_LEFT (272) or the injection hook's `button=1`; both mean left.
    let pressed = stdout
        .lines()
        .find(|line| {
            line.starts_with("AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=")
                && line.contains("state=press")
        })
        .unwrap_or_else(|| panic!("no press marker for the dock tile:\n{stdout}"));
    assert!(
        pressed.ends_with("state=press window=861,1048"),
        "the press landed at the tile in window coordinates: {pressed}"
    );
    assert!(
        pressed.contains("button=272 ") || pressed.contains("button=1 "),
        "a left click: {pressed}"
    );
    assert!(
        stdout.lines().any(|line| {
            line.starts_with("AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=")
                && line.contains("state=release")
        }),
        "the release followed the press:\n{stdout}"
    );
    // ... and the tile's handler activated the first pinned app.
    assert!(
        stdout.contains("AETHER_SHELL_ACTIVATE: surface=dock item=aether-files"),
        "the dock click activated its tile:\n{stdout}"
    );
    // The pointer counters moved, and the activation repainted.
    let run = stdout
        .lines()
        .find(|line| line.starts_with("AETHER_SHELL_LAYER_RUN: "))
        .unwrap_or_else(|| panic!("no run summary:\n{stdout}"));
    assert!(
        !run.contains("pointers=0 "),
        "pointer events were routed: {run}"
    );

    let _ = fs::remove_dir_all(&shots);
}

/// Pacing: an idle shell must not repaint or recommit, and a real repaint must
/// go through `wl_surface.frame` + two buffers, which the compositor proves by
/// releasing the buffer it replaced.
#[test]
fn frames_follow_the_compositor_and_idle_shell_does_nothing() {
    let _serial = serial();
    let Some(binary) = compositor_binary() else {
        println!("AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary");
        return;
    };

    // 1. Idle: one frame for the three boot surfaces, then nothing at all.
    let mut compositor = CompositorProcess::start_with(
        &binary,
        "idle",
        // Short enough that the compositor shuts down and prints its stats
        // while the test waits, long enough for the shell to finish first.
        &[("AETHER_COMPOSITOR_GRACE_MS", "1200")],
    );
    let shots = temp_dir("shots-idle");
    let _ = fs::remove_dir_all(&shots);
    let output = connect_for(&compositor.runtime_dir, &shots, None, 400);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "stdout:\n{stdout}");
    let run = stdout
        .lines()
        .find(|line| line.starts_with("AETHER_SHELL_LAYER_RUN: "))
        .unwrap_or_else(|| panic!("no run summary:\n{stdout}"));
    assert!(
        run.contains("paints=1 commits=3 "),
        "an idle shell paints once and commits the three boot surfaces: {run}"
    );
    let journal = compositor.wait_for_shutdown(Duration::from_secs(5));
    let stats = journal
        .lines()
        .find(|line| line.starts_with("AETHER_COMPOSITOR_FRAME_STATS: "))
        .unwrap_or_else(|| panic!("the compositor printed no frame stats:\n{journal}"));
    assert!(
        stats.contains("buffers_released=0"),
        "an idle shell never replaces a buffer: {stats}"
    );
    let _ = fs::remove_dir_all(&shots);

    // 2. One injected click: a repaint, the second buffer, and a release.
    let mut compositor = CompositorProcess::start_with(
        &binary,
        "pace",
        &[
            ("AETHER_COMPOSITOR_INJECT_POINTER", "x=861,y=1048,button=1"),
            // The compositor replays the action on its own frame clock, which
            // starts after the first presented frame; give it room and let it
            // outlive the shell so it can print `FRAME_STATS`.
            ("AETHER_COMPOSITOR_GRACE_MS", "3000"),
        ],
    );
    let shots = temp_dir("shots-pace");
    let _ = fs::remove_dir_all(&shots);
    let output = connect_for(&compositor.runtime_dir, &shots, None, 2500);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if std::env::var_os("AETHER_SHELL_TEST_LOG").is_some() {
        println!("--- paced run ---\n{stdout}");
        println!(
            "--- paced stderr ---\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        println!("--- journal (live) ---\n{}", compositor.log());
    }
    assert!(output.status.success(), "stdout:\n{stdout}");
    let run = stdout
        .lines()
        .find(|line| line.starts_with("AETHER_SHELL_LAYER_RUN: "))
        .unwrap_or_else(|| panic!("no run summary:\n{stdout}"));
    assert!(
        run.contains("paints=2 "),
        "the click caused exactly one extra frame: {run}"
    );
    // The dock and the menu bar were recommitted; the wallpaper was not.
    let dock_paints = stdout
        .lines()
        .find(|line| line.starts_with("AETHER_SHELL_LAYER_PAINT: name=dock "))
        .unwrap_or_else(|| panic!("no dock paint line:\n{stdout}"));
    assert!(
        dock_paints.contains("paints=2"),
        "the dock was repainted once: {dock_paints}"
    );
    let background_paints = stdout
        .lines()
        .find(|line| line.starts_with("AETHER_SHELL_LAYER_PAINT: name=background "))
        .unwrap_or_else(|| panic!("no background paint line:\n{stdout}"));
    assert!(
        background_paints.contains("paints=1"),
        "the wallpaper is painted once: {background_paints}"
    );

    let journal = compositor.wait_for_shutdown(Duration::from_secs(5));
    let released: u64 = journal
        .lines()
        .find(|line| line.starts_with("AETHER_COMPOSITOR_FRAME_STATS: "))
        .and_then(|line| line.split("buffers_released=").nth(1))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_else(|| panic!("no buffers_released in the stats:\n{journal}"));
    assert!(
        released >= 1,
        "the compositor released the replaced buffer (released={released}):\n{journal}"
    );
    let _ = fs::remove_dir_all(&shots);
}

/// A second connection after the first one closed must be refused cleanly
/// rather than hanging: the shell prints a harness-readable error.
#[test]
fn a_missing_socket_is_an_error_not_a_hang() {
    let _serial = serial();
    let runtime_dir = temp_dir("nosocket");
    let _ = fs::remove_dir_all(&runtime_dir);
    fs::create_dir_all(&runtime_dir).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .arg("--connect")
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env("WAYLAND_DISPLAY", "wayland-does-not-exist")
        .output()
        .expect("run aether-shell --connect");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("cannot connect to wayland-does-not-exist"),
        "stderr:\n{stderr}"
    );
    let _ = fs::remove_dir_all(&runtime_dir);
}
