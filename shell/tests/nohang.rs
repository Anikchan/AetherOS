//! G-NH2 command tests through the real `aether-shell` binary (P3-T08 ↔ P4).
//!
//! The shell and the compositor meet at the journal markers, so these tests
//! feed fixture journals in the compositor's own format to `--nohang-check`
//! and drive the start ledger with `--start-begin`, `--start-confirm` and
//! `--safe-mode-reset` against a temporary `XDG_CONFIG_HOME`. The on-device
//! acceptance harness runs exactly the code path verified here.
//!
//! The compositor's `tests/p3_t08_no_hang.rs` proves the markers are emitted.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-nohang-cli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn shell_with_config(config_home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .output()
        .expect("run aether-shell")
}

fn shell_stdin(config_home: &Path, args: &[&str], input: &str) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn aether-shell");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn temp_log(tag: &str, text: &str) -> PathBuf {
    let path = temp_dir(tag).with_extension("log");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// A hang detected at 5.1 s (inside the 6 s G-NH2 bound) that recovers, with a
/// shell that stalled once and committed again.
fn healthy_log() -> String {
    "\
AETHER_COMPOSITOR_NO_HANG_CONFIG: ping_interval_ms=2000 ping_timeout_ms=5000 frame_gap_ms=1000 shell_watchdog_ms=5000
AETHER_COMPOSITOR_PING: client=7 serial=1
AETHER_COMPOSITOR_APP_STATE: client=7 state=ping_pending after_ms=0
AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100
AETHER_COMPOSITOR_APP_STATE: client=7 state=responsive recovered=true
AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6100 timeout_ms=5000 timeouts=1
AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true timeouts=1
AETHER_COMPOSITOR_NO_HANG_SUMMARY: clients=1 responsive=1 ping_pending=0 not_responding=0 timeout_ms=5000
"
    .to_string()
}

#[test]
fn the_check_accepts_a_hang_inside_the_six_second_bound() {
    let home = temp_dir("ok");
    std::fs::create_dir_all(&home).unwrap();
    let log = temp_log("ok", &healthy_log());

    let output = shell_with_config(&home, &["--nohang-check", log.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(
        text.contains("NOHANG_HANG client=7 after_ms=5100 within_limit=true"),
        "{text}"
    );
    assert!(text.contains("NOHANG_RECOVERED client=7"), "{text}");
    assert!(
        text.contains("NOHANG_WATCHDOG timeouts=1 last_state=ok recovered=true"),
        "{text}"
    );
    assert!(text.contains("restart_required=false"), "{text}");
    assert!(text.contains("NOHANG_START attempts=0"), "{text}");
    assert!(text.ends_with("AETHER_SHELL_NOHANG_OK\n"), "{text}");

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(log);
}

#[test]
fn the_check_rejects_a_late_detection_and_a_missing_marker() {
    let home = temp_dir("late");
    std::fs::create_dir_all(&home).unwrap();
    let late = temp_log(
        "late",
        "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=8000\n",
    );

    let output = shell_with_config(&home, &["--nohang-check", late.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{text}");
    assert!(
        text.contains(
            "AETHER_SHELL_NOHANG_PROBLEM: client=7 was marked not responding after 8000 ms; G-NH2 allows 6000 ms"
        ),
        "{text}"
    );
    assert!(text.ends_with("AETHER_SHELL_NOHANG_FAILED\n"), "{text}");

    // Not a compositor journal at all.
    let empty = temp_log("empty", "hello\n");
    let output = shell_with_config(&home, &["--nohang-check", empty.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text.contains("no AETHER_COMPOSITOR_APP_STATE marker"),
        "{text}"
    );

    // A missing file is an error, not a panic.
    let output = shell_with_config(&home, &["--nohang-check", "/nonexistent/aether.log"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read compositor log"));

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(late);
    let _ = std::fs::remove_file(empty);
}

#[test]
fn a_shell_that_stopped_committing_needs_a_restart() {
    let home = temp_dir("stalled");
    std::fs::create_dir_all(&home).unwrap();
    let stalled = temp_log(
        "stalled",
        "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100\n\
         AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=9000 timeout_ms=5000 timeouts=2\n",
    );

    let output = shell_with_config(&home, &["--nohang-check", stalled.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{text}");
    assert!(
        text.contains("AETHER_COMPOSITOR_SHELL_WATCHDOG last state=timeout"),
        "{text}"
    );
    assert!(text.contains("must restart it"), "{text}");

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(stalled);
}

#[test]
fn the_check_reads_stdin_for_the_harness() {
    let home = temp_dir("stdin");
    std::fs::create_dir_all(&home).unwrap();
    let output = shell_stdin(&home, &["--nohang-check", "-"], &healthy_log());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.ends_with("AETHER_SHELL_NOHANG_OK\n"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn three_failed_starts_enter_safe_mode_and_reset_leaves_it() {
    let home = temp_dir("ledger");
    std::fs::create_dir_all(&home).unwrap();
    let session = home.join("aether").join("shell").join("session.json");

    let first = shell_with_config(&home, &["--start-begin"]);
    let text = String::from_utf8_lossy(&first.stdout);
    assert!(first.status.success());
    assert!(
        text.contains("AETHER_SHELL_START: attempt=1 safe_mode=false previous=clean"),
        "{text}"
    );
    assert!(session.exists(), "the ledger is persisted");

    // Two more starts without a `--start-confirm`: the shell is in a loop.
    shell_with_config(&home, &["--start-begin"]);
    let third = shell_with_config(&home, &["--start-begin"]);
    let text = String::from_utf8_lossy(&third.stdout);
    assert!(
        text.contains("AETHER_SHELL_START: attempt=3 safe_mode=true previous=unconfirmed"),
        "{text}"
    );
    assert!(
        text.contains("AETHER_SHELL_SAFE_MODE: attempts=3"),
        "{text}"
    );
    assert!(text.contains("effects=minimal"), "{text}");

    // A good start clears the attempt count but keeps the banner on.
    let confirmed = shell_with_config(&home, &["--start-confirm"]);
    assert!(String::from_utf8_lossy(&confirmed.stdout)
        .contains("AETHER_SHELL_START_CONFIRMED: attempts=0"));

    // The safe-mode reset leaves it and the next start is clean.
    let reset = shell_with_config(&home, &["--safe-mode-reset"]);
    assert!(String::from_utf8_lossy(&reset.stdout)
        .contains("AETHER_SHELL_SAFE_MODE_RESET: attempts=0 safe_mode=false"));
    let next = shell_with_config(&home, &["--start-begin"]);
    let text = String::from_utf8_lossy(&next.stdout);
    assert!(
        text.contains("attempt=1 safe_mode=false previous=clean"),
        "{text}"
    );

    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn watchdog_timeouts_arm_safe_mode_from_the_journal() {
    let home = temp_dir("watchdog");
    std::fs::create_dir_all(&home).unwrap();
    let log = temp_log(
        "watchdog",
        "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6000 timeout_ms=5000 timeouts=3\n",
    );

    let output = shell_with_config(&home, &["--start-begin", log.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(
        text.contains("AETHER_SHELL_SAFE_MODE: attempts=1 timeouts=3"),
        "{text}"
    );
    assert!(text.contains("reason=the shell stalled 3 times"), "{text}");

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(log);
}

#[test]
fn status_reports_safe_mode_and_the_dialog_state() {
    let home = temp_dir("status");
    std::fs::create_dir_all(&home).unwrap();
    for _ in 0..3 {
        shell_with_config(&home, &["--start-begin"]);
    }

    let output = shell_with_config(&home, &["--status"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.contains("safe-mode: true"), "{text}");
    assert!(text.contains("start-attempts: 3"), "{text}");
    assert!(text.contains("nohang: idle"), "{text}");
    assert!(text.contains("effects-effective: minimal"), "{text}");
    assert!(text.contains("effects-env: AETHER_COMPOSITOR_EFFECT_TIER=minimal"));
    assert!(text.ends_with("AETHER_SHELL_STATUS_OK\n"), "{text}");

    let _ = std::fs::remove_dir_all(home);
}
