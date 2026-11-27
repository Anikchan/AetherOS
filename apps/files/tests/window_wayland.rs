//! Runtime window check: a launched app really creates a Wayland toplevel.
//!
//! This is the end-to-end proof for the app window backend (2026-11-28). The
//! test boots the repo's own compositor in headless mode on a private
//! `XDG_RUNTIME_DIR`, starts the `aether-files` binary with `WAYLAND_DISPLAY`
//! pointed at it (no flags: this is the shell's `--launch` path), and asserts
//! the compositor observed an xdg-shell toplevel and mapped a window.
//!
//! It exercises the real path the first graphical boot takes: Slint's winit
//! backend on Wayland with the software renderer → softbuffer → `wl_shm`.
//! There is no GPU and no X server involved.
//!
//! Linux only: winit's Wayland backend needs a Wayland client library at
//! runtime, which the macOS development host does not have. On every other
//! platform the test prints a skip line and passes.
//!
//! Run it directly with:
//!
//! ```text
//! cargo build -p aether-compositor --bin aether-compositor
//! cargo test -p aether-files --test window_wayland -- --nocapture
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Marker the compositor prints when an xdg-shell surface becomes a toplevel.
const XDG_TOPLEVEL: &str = "AETHER_COMPOSITOR_XDG_TOPLEVEL";
/// Marker the compositor prints when its WM places a mapped toplevel.
const WM_WINDOW: &str = "AETHER_COMPOSITOR_WM_WINDOW";

#[test]
fn launched_app_opens_a_toplevel_on_headless_wayland() {
    println!(
        "aether-files binary: {}",
        env!("CARGO_BIN_EXE_aether-files")
    );
    if !cfg!(target_os = "linux") {
        println!(
            "SKIP launched_app_opens_a_toplevel_on_headless_wayland: \
             runtime window creation is verified on Linux only (Wayland client + softbuffer)"
        );
        return;
    }

    let runtime_dir = RuntimeDir::new();
    let compositor_bin = compositor_binary();
    let mut compositor = Compositor::start(&compositor_bin, runtime_dir.path());
    let mut app = App::start(runtime_dir.path());

    // Wait for the app's toplevel to reach the compositor. The app has to
    // connect, negotiate xdg-shell, allocate an SHM buffer and commit it, so
    // give it a generous window and fail fast if it exits instead.
    let log_path = runtime_dir.path().join("compositor.log");
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let log = read(&log_path);
        if log.contains(XDG_TOPLEVEL) && log.contains(WM_WINDOW) {
            break;
        }
        if let Some(status) = app.exited() {
            panic!(
                "aether-files exited before the compositor saw a window ({status})\n\
                 --- app stdout ---\n{}\n--- app stderr ---\n{}\n--- compositor log ---\n{log}\n\
                 --- compositor stderr ---\n{}",
                app.stdout(),
                app.stderr(),
                read(&runtime_dir.path().join("compositor.err"))
            );
        }
        if Instant::now() >= deadline {
            panic!(
                "aether-files did not create a Wayland window within 45 s\n\
                 --- app stdout ---\n{}\n--- app stderr ---\n{}\n--- compositor log ---\n{log}\n\
                 --- compositor stderr ---\n{}",
                app.stdout(),
                app.stderr(),
                read(&runtime_dir.path().join("compositor.err"))
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Keep both logs visible on a real run (`--nocapture`), so a failure or a
    // pass both show what the compositor saw.
    let log = read(&log_path);
    println!("--- compositor log ---\n{log}");
    println!("--- app stdout ---\n{}", app.stdout());
    println!("--- app stderr ---\n{}", app.stderr());

    assert!(
        log.contains("AETHER_COMPOSITOR_BACKEND: headless"),
        "the compositor must run its headless backend:\n{log}"
    );
    assert!(
        log.contains(XDG_TOPLEVEL),
        "the app's xdg-shell toplevel reached the compositor:\n{log}"
    );
    assert!(
        log.contains(WM_WINDOW),
        "the app's window was mapped and placed:\n{log}"
    );
    // The app must still be running: a mapped window is not a window that
    // closed straight away (the shell would show it for a frame and lose it).
    assert!(
        app.exited().is_none(),
        "aether-files exited right after mapping its window"
    );

    app.stop();
    compositor.stop();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// Locate the compositor binary next to the app binary under test.
///
/// `env!("CARGO_BIN_EXE_..")` only covers binaries of the package under test,
/// so the workspace sibling is found in the same target profile directory. If
/// this test runs before anything built it (a bare `cargo test -p aether-files`
/// does not build the compositor), it is built once through Cargo.
fn compositor_binary() -> PathBuf {
    let app_bin = PathBuf::from(env!("CARGO_BIN_EXE_aether-files"));
    let profile_dir = app_bin
        .parent()
        .expect("the app binary has a target profile directory")
        .to_path_buf();
    let binary = profile_dir.join("aether-compositor");
    if binary.exists() {
        return binary;
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("apps/files -> repository root");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args([
            "build",
            "-p",
            "aether-compositor",
            "--bin",
            "aether-compositor",
        ])
        .current_dir(root)
        .status()
        .expect("run cargo build -p aether-compositor");
    assert!(
        status.success(),
        "cargo build -p aether-compositor failed ({status}); build it before this test"
    );
    assert!(
        binary.exists(),
        "aether-compositor was not produced at {}",
        binary.display()
    );
    binary
}

/// A private, 0700 `XDG_RUNTIME_DIR` removed on drop.
struct RuntimeDir {
    path: PathBuf,
}

impl RuntimeDir {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aether-app-window-{}-{stamp}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create runtime dir");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("chmod runtime dir");
        }
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RuntimeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct Compositor {
    child: Child,
}

impl Compositor {
    fn start(binary: &Path, runtime_dir: &Path) -> Self {
        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("compositor log");
        let stderr =
            fs::File::create(runtime_dir.join("compositor.err")).expect("compositor stderr");
        let child = Command::new(binary)
            .env("XDG_RUNTIME_DIR", runtime_dir)
            // Headless: no DRM, no GPU, no seat. The private runtime dir keeps
            // the test off the developer's real session.
            .env("AETHER_COMPOSITOR_BACKEND", "headless")
            // Stay alive long enough for the app to start and map its window
            // (the default grace is 3 s after the first mapped surface).
            .env("AETHER_COMPOSITOR_GRACE_MS", "20000")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn aether-compositor");

        // Wait for the socket before starting the client.
        let socket = runtime_dir.join("wayland-0");
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut this = Self { child };
        while Instant::now() < deadline {
            if socket.exists() {
                return this;
            }
            if let Ok(Some(status)) = this.child.try_wait() {
                panic!(
                    "compositor exited before creating its socket ({status})\n--- stderr ---\n{}",
                    read(&runtime_dir.join("compositor.err"))
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("compositor did not create {} in time", socket.display());
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct App {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl App {
    fn start(runtime_dir: &Path) -> Self {
        let stdout = runtime_dir.join("app.out");
        let stderr = runtime_dir.join("app.err");
        let child = Command::new(env!("CARGO_BIN_EXE_aether-files"))
            .env("XDG_RUNTIME_DIR", runtime_dir)
            .env("WAYLAND_DISPLAY", "wayland-0")
            // The app must pick its winit backend itself, not a testing one
            // inherited from the developer's shell.
            .env_remove("SLINT_BACKEND")
            .stdout(Stdio::from(
                fs::File::create(&stdout).expect("app stdout"),
            ))
            .stderr(Stdio::from(
                fs::File::create(&stderr).expect("app stderr"),
            ))
            .spawn()
            .expect("spawn aether-files");
        Self {
            child,
            stdout,
            stderr,
        }
    }

    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().expect("poll app")
    }

    fn stdout(&self) -> String {
        read(&self.stdout)
    }

    fn stderr(&self) -> String {
        read(&self.stderr)
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Even on a panic path the app process must not leak: it would keep
        // running against a compositor that is about to disappear.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
