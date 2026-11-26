//! End-to-end Landlock enforcement test (P6-T04 follow-up).
//!
//! Drives the real `aether-landlock` shim binary:
//! - on every host the shim must refuse (exit 126, program never run) when
//!   Landlock is unavailable — the fail-closed contract;
//! - on Linux with Landlock it must allow a granted path and deny one that is
//!   outside the ruleset.
//!
//! The kernel test is skipped (with a printed reason) where the feature is
//! missing, so the suite stays green on the macOS dev host.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SHIM: &str = env!("CARGO_BIN_EXE_aether-landlock");
const REFUSED: i32 = 126;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aether-landlock-it-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn shim_probe_reports_a_verdict() {
    let output = Command::new(SHIM).arg("--probe").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if cfg!(target_os = "linux") && output.status.success() {
        assert!(
            stdout.starts_with("LANDLOCK_ABI "),
            "unexpected probe output: {stdout}{stderr}"
        );
    } else {
        assert!(
            stdout.contains("LANDLOCK_UNAVAILABLE"),
            "probe must report a reason: {stdout}{stderr}"
        );
        assert_eq!(
            output.status.code(),
            Some(REFUSED),
            "an unavailable kernel is a refusal, not a success"
        );
    }
}

#[test]
fn shim_never_runs_a_program_it_cannot_sandbox() {
    let dir = temp_dir("no-policy");
    let marker = dir.join("ran");
    let program = dir.join("app");
    fs::write(&program, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // No rules: Landlock would deny everything, so the shim refuses.
    let output = Command::new(SHIM)
        .arg("--")
        .arg(&program)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(REFUSED));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to start"),
        "the refusal is explained"
    );
    assert!(!marker.exists(), "the program must not run");

    // A genuinely missing `--` is a usage error, also without running anything.
    let output = Command::new(SHIM).arg(&program).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists(), "the program must not run");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ruleset_allows_granted_paths_and_denies_others() {
    let probe = Command::new(SHIM).arg("--probe").output().unwrap();
    if !cfg!(target_os = "linux") || !probe.status.success() {
        eprintln!(
            "skipped: Landlock unavailable on this host ({})",
            String::from_utf8_lossy(&probe.stdout).trim()
        );
        return;
    }

    let dir = temp_dir("ruleset");
    let allowed = dir.join("allowed.txt");
    fs::write(&allowed, "inside").unwrap();
    let outside = Path::new("/proc/version");

    let shim = Command::new(SHIM)
        .args(["--ro", "/usr", "--ro", "/bin", "--ro", "/lib", "--ro", "/lib64"])
        .args(["--ro", "/etc", "--ro", dir.to_str().unwrap()])
        .arg("--")
        .arg("/bin/cat")
        .arg(&allowed)
        .output()
        .unwrap();
    assert!(
        shim.status.success(),
        "reading a granted path must work: {}",
        String::from_utf8_lossy(&shim.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&shim.stdout).trim(), "inside");

    let denied = Command::new(SHIM)
        .args(["--ro", "/usr", "--ro", "/bin", "--ro", "/lib", "--ro", "/lib64"])
        .args(["--ro", dir.to_str().unwrap()])
        .arg("--")
        .arg("/bin/cat")
        .arg(outside)
        .output()
        .unwrap();
    assert!(
        !denied.status.success(),
        "a path outside the ruleset must be denied"
    );
    assert!(
        String::from_utf8_lossy(&denied.stdout).trim().is_empty(),
        "no file content may leak from outside the sandbox"
    );

    let _ = fs::remove_dir_all(dir);
}
