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
    std::env::temp_dir().join(format!(
        "aether-shell-handshake-{tag}-{}",
        std::process::id()
    ))
}

/// The compositor under test, killed and cleaned up on drop — even when an
/// assertion panics.
struct CompositorProcess {
    runtime_dir: PathBuf,
    child: Child,
}

impl CompositorProcess {
    fn start(binary: &Path, tag: &str) -> Self {
        let runtime_dir = temp_dir(tag);
        let _ = fs::remove_dir_all(&runtime_dir);
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
            .expect("chmod runtime dir");

        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("create log");
        let err = fs::File::create(runtime_dir.join("compositor.err")).expect("create stderr log");
        let child = Command::new(binary)
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("AETHER_COMPOSITOR_BACKEND", "headless")
            // The default grace (3 s after the first mapped surface) is shorter
            // than a slow test run; the shell needs the socket to stay up.
            .env("AETHER_COMPOSITOR_GRACE_MS", "20000")
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_aether-shell"));
    command.args([
        "--connect",
        "--hold-ms",
        "300",
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
    let Some(binary) = compositor_binary() else {
        println!(
            "AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary next to {}; \
             run `cargo build -p aether-compositor` or set AETHER_COMPOSITOR_BIN",
            env!("CARGO_BIN_EXE_aether-shell")
        );
        return;
    };
    let compositor = CompositorProcess::start(&binary, "compositor");
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

    // --- Shell → compositor: the surfaces exist, in plan order -------------
    let planned = plan();
    let mut last_line = 0usize;
    for request in &planned {
        let created = format!(
            "AETHER_SHELL_LAYER_CREATED: name={} namespace={} layer={} anchors={} ",
            request.name, request.namespace, request.layer_name, request.anchors
        );
        let line = stdout
            .lines()
            .position(|line| line.starts_with(&created))
            .unwrap_or_else(|| panic!("no host report for {}\n{stdout}", request.name));
        assert!(
            line > last_line,
            "surfaces are created in plan order: {} came too early\n{stdout}",
            request.name
        );
        last_line = line;

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
    assert_eq!(
        surface_lines.len(),
        planned.len(),
        "the compositor created one layer surface per planned namespace:\n{journal}"
    );
    for (line, request) in surface_lines.iter().zip(&planned) {
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
    for request in &planned {
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
    open("menubar", true);
    open("dock", true);
    open("launcher", false);
    open("notifications", false);
    open("control-center", false);
    assert!(
        stdout
            .contains("AETHER_SHELL_LAYER_HOST_READY: surfaces=5 configured=5 mapped=2 painted=2"),
        "two panels are up on a fresh session:\n{stdout}"
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
    let Some(binary) = compositor_binary() else {
        println!("AETHER_SHELL_HANDSHAKE_SKIPPED: no aether-compositor binary");
        return;
    };
    let compositor = CompositorProcess::start(&binary, "handshake-open");
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

/// A second connection after the first one closed must be refused cleanly
/// rather than hanging: the shell prints a harness-readable error.
#[test]
fn a_missing_socket_is_an_error_not_a_hang() {
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
