//! P3-T09 acceptance test — output backend selection.
//!
//! The DRM/KMS backend itself is hardware-gated: it needs a DRM device, a
//! libseat session and a GPU, so it is not asserted here and no test claims it
//! ran. What *is* asserted is everything that decides whether the process takes
//! that path:
//!
//! 1. the pure selection rules (`headless` / `drm` / `auto` / unknown),
//! 2. that DRM can only be selected when the crate was built with the `drm`
//!    cargo feature,
//! 3. that the real binary prints the resolved backend before doing anything
//!    else, and that `AETHER_COMPOSITOR_BACKEND=headless` still boots the
//!    headless backend with its two synthetic outputs.
//!
//! The hardware residual (a real page flip on the Dell) is recorded in
//! `docs/2026/11/27/results/p3-t09-drm-backend.md`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aether_compositor::backend::{layer_location, BackendKind};
use aether_compositor::{is_unknown_backend, select_backend};

#[test]
fn explicit_headless_always_wins() {
    // Even on a machine with a GPU, CI and the dev host must be able to force
    // the backend the headless tests were written for.
    assert_eq!(
        select_backend(Some("headless"), true, true),
        BackendKind::Headless
    );
    assert_eq!(
        select_backend(Some("HEADLESS "), true, true),
        BackendKind::Headless
    );
}

#[test]
fn explicit_drm_is_honoured_even_without_a_device() {
    // The DRM backend then fails with a diagnosable error instead of silently
    // presenting a black headless desktop on the Dell.
    assert_eq!(select_backend(Some("drm"), false, true), BackendKind::Drm);
    assert_eq!(select_backend(Some("kms"), false, true), BackendKind::Drm);
}

#[test]
fn auto_follows_device_presence() {
    assert_eq!(select_backend(Some("auto"), true, true), BackendKind::Drm);
    assert_eq!(
        select_backend(Some("auto"), false, true),
        BackendKind::Headless
    );
    assert_eq!(select_backend(None, true, true), BackendKind::Drm);
    assert_eq!(select_backend(None, false, true), BackendKind::Headless);
}

#[test]
fn drm_requires_the_cargo_feature() {
    // A binary built without `--features drm` must never pick the hardware
    // backend, whatever the environment says.
    assert_eq!(
        select_backend(Some("drm"), true, false),
        BackendKind::Headless
    );
    assert_eq!(
        select_backend(Some("auto"), true, false),
        BackendKind::Headless
    );
}

#[test]
fn unknown_values_fall_back_to_headless_and_are_reported() {
    assert_eq!(
        select_backend(Some("drmmm"), true, true),
        BackendKind::Headless
    );
    assert!(is_unknown_backend(Some("drmmm")));
    assert!(is_unknown_backend(Some("wayland")));
    for known in ["headless", "drm", "kms", "auto", ""] {
        assert!(!is_unknown_backend(Some(known)), "{known}");
    }
    assert!(!is_unknown_backend(None));
}

#[test]
fn backend_names_round_trip() {
    for kind in [BackendKind::Headless, BackendKind::Drm] {
        assert_eq!(BackendKind::parse(kind.as_str()), Some(kind));
        assert!(aether_compositor::BACKEND_ENV.ends_with("BACKEND"));
    }
    assert_eq!(BackendKind::parse("nope"), None);
}

#[test]
fn layer_surfaces_are_placed_on_their_anchored_edge() {
    // The DRM renderer places shell chrome with this function only; the KMS
    // test itself needs the Dell.
    assert_eq!(
        layer_location((false, false, true, false), (1920, 32), (1920, 1080)),
        (0, 0)
    );
    assert_eq!(
        layer_location((false, false, false, true), (1920, 64), (1920, 1080)),
        (0, 1016)
    );
    assert_eq!(
        layer_location((false, false, false, false), (400, 300), (1920, 1080)),
        (760, 390)
    );
}

#[test]
fn binary_reports_the_selected_backend() {
    let log = TempDir::new("p3t09-log");
    let mut harness = Harness::new(&log, "headless");
    harness.wait_for_marker("AETHER_COMPOSITOR_BACKEND: headless", 10);
    // The headless backend still advertises both monitors.
    harness.wait_for_marker("AETHER_COMPOSITOR_OUTPUT_COUNT: 2", 10);
    assert!(
        !harness.log().contains("AETHER_COMPOSITOR_DRM_"),
        "headless run must not touch DRM: {}",
        harness.log()
    );
}

#[test]
fn unknown_backend_value_is_reported_and_headless_boots() {
    let log = TempDir::new("p3t09-unknown");
    let mut harness = Harness::new(&log, "not-a-backend");
    harness.wait_for_marker("AETHER_COMPOSITOR_BACKEND: headless", 10);
    harness.wait_for_marker("unknown AETHER_COMPOSITOR_BACKEND", 10);
}

struct Harness {
    child: Child,
    log_path: PathBuf,
    #[allow(dead_code)]
    runtime_dir: TempDir,
}

impl Harness {
    fn new(log: &TempDir, backend: &str) -> Self {
        let runtime_dir = TempDir::new("p3t09-runtime");
        fs::set_permissions(runtime_dir.path(), fs::Permissions::from_mode(0o700))
            .expect("chmod XDG_RUNTIME_DIR");
        let log_path = log.path().join("compositor.log");
        let stdout = fs::File::create(&log_path).expect("create log");
        let stderr = stdout.try_clone().expect("clone log");
        let child = Command::new(env!("CARGO_BIN_EXE_aether-compositor"))
            .env("XDG_RUNTIME_DIR", runtime_dir.path())
            .env("AETHER_COMPOSITOR_BACKEND", backend)
            // No client ever connects: exit quickly once the grace expires.
            .env("AETHER_COMPOSITOR_GRACE_MS", "200")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn aether-compositor");
        Self {
            child,
            log_path,
            runtime_dir,
        }
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap_or_default()
    }

    fn wait_for_marker(&mut self, marker: &str, budget_secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(budget_secs);
        while Instant::now() < deadline {
            if self.log().contains(marker) {
                return;
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!(
                    "compositor exited ({status}) before printing {marker:?}:\n{}",
                    self.log()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("marker {marker:?} not printed:\n{}", self.log());
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        let base = std::env::temp_dir();
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = base.join(unique);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
