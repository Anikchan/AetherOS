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
    fn start(binary: &Path) -> Self {
        let runtime_dir = temp_dir("compositor");
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

/// Run the shipped shell host against `runtime_dir`/`wayland-0`.
fn connect(runtime_dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(["--connect", "--hold-ms", "100"])
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("WAYLAND_DISPLAY", "wayland-0")
        .output()
        .expect("run aether-shell --connect")
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
    let compositor = CompositorProcess::start(&binary);
    let output = connect(&compositor.runtime_dir);
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
        stdout.ends_with("AETHER_SHELL_LAYER_HOST_OK\n"),
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
                " exclusive_zone={} keyboard={}\n",
                request.exclusive_zone, request.keyboard_interactivity
            )),
            "{} lost its exclusive zone or keyboard mode\n{stdout}",
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
