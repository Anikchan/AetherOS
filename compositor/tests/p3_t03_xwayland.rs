//! P3-T03 XWayland acceptance test.
//!
//! Boots the real `aether-compositor` binary (which spawns XWayland) on a
//! private `XDG_RUNTIME_DIR`, waits for the XWayland display to come up, runs
//! the `xeyes` X11 client against it, and asserts the compositor prints
//! `AETHER_COMPOSITOR_XWAYLAND_MAPPED`.
//!
//! Skips (passes) when `Xwayland` or `xeyes` is not on PATH (e.g. macOS dev
//! host). Runs in CI where both are installed (`xwayland`, `x11-apps`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct CompositorProcess {
    runtime_dir: std::path::PathBuf,
    child: Child,
}

impl CompositorProcess {
    fn start() -> Self {
        let runtime_dir = std::env::temp_dir().join(format!("aether-p3-t03-{}", std::process::id()));
        let _ = fs::remove_dir_all(&runtime_dir);
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        #[cfg(unix)]
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700)).expect("chmod");
        let log = fs::File::create(runtime_dir.join("compositor.log")).expect("create log");
        let stderr = fs::File::create(runtime_dir.join("compositor.err")).expect("create stderr");
        let mut child = Command::new(env!("CARGO_BIN_EXE_aether-compositor"))
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("AETHER_COMPOSITOR_GRACE_MS", "20000")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn compositor");
        // wait for socket
        let socket = runtime_dir.join("wayland-0");
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if socket.exists() {
                break;
            }
            if let Ok(Some(s)) = child.try_wait() {
                panic!("compositor exited early: {s}");
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(socket.exists(), "compositor socket not created");
        Self { runtime_dir, child }
    }

    fn log(&self) -> String {
        fs::read_to_string(self.runtime_dir.join("compositor.log")).unwrap_or_default()
    }

    fn stderr_log(&self) -> String {
        fs::read_to_string(self.runtime_dir.join("compositor.err")).unwrap_or_default()
    }

    fn wait_for_display(&mut self) -> Option<u32> {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let log = self.log();
            for line in log.lines() {
                if let Some(rest) = line.strip_prefix("AETHER_COMPOSITOR_XWAYLAND_DISPLAY: ") {
                    if let Ok(n) = rest.trim().parse() {
                        return Some(n);
                    }
                }
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!(
                    "compositor exited before XWayland became ready ({status})\n--- stdout ---\n{}\n--- stderr (incl. XWayland) ---\n{}",
                    log,
                    self.stderr_log()
                );
            }
            thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for CompositorProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

#[test]
fn xwayland_xeyes_maps() {
    // Skip if Xwayland or xeyes missing.
    if Command::new("Xwayland").arg("-version").output().is_err()
        && Command::new("xeyes").arg("-version").output().is_err()
    {
        // try spawn xeyes help
    }
    // More robust: check availability via `which`-like spawn.
    let xwayland_ok = Command::new("Xwayland").arg("-version").output().is_ok()
        || Command::new("which").arg("Xwayland").output().map(|o| o.status.success()).unwrap_or(false);
    let xeyes_ok = Command::new("which").arg("xeyes").output().map(|o| o.status.success()).unwrap_or(false)
        || Command::new("xeyes").arg("-help").output().is_ok();
    if !xwayland_ok || !xeyes_ok {
        eprintln!("skipping: Xwayland or xeyes not available");
        return;
    }

    let mut comp = CompositorProcess::start();
    let display = match comp.wait_for_display() {
        Some(n) => n,
        None => {
            eprintln!(
                "compositor stdout:\n{}\ncompositor stderr (incl. XWayland):\n{}",
                comp.log(),
                comp.stderr_log()
            );
            panic!("XWayland did not become ready");
        }
    };

    // Run xeyes.
    let mut xeyes = Command::new("xeyes")
        .env("DISPLAY", format!(":{display}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn xeyes");

    // Wait for marker.
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut found = false;
    while Instant::now() < deadline {
        if comp.log().contains("AETHER_COMPOSITOR_XWAYLAND_MAPPED") {
            found = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let _ = xeyes.kill();
    let _ = xeyes.wait();

    assert!(found, "marker not found; log:\n{}", comp.log());
}