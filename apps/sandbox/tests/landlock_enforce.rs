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

#[test]
fn read_write_wins_over_read_only_for_the_same_path() {
    // Kernel-level proof of the rule normalization: a path passed both `--rw`
    // and `--ro` must end up writable, whichever argument comes first.
    let probe = Command::new(SHIM).arg("--probe").output().unwrap();
    if !cfg!(target_os = "linux") || !probe.status.success() {
        eprintln!("skipped: Landlock unavailable on this host");
        return;
    }

    for (tag, first, second) in [
        ("rw-then-ro", "--rw", "--ro"),
        ("ro-then-rw", "--ro", "--rw"),
    ] {
        let dir = temp_dir(tag);
        let output = Command::new(SHIM)
            .args(["--ro", "/usr", "--ro", "/bin", "--ro", "/lib", "--ro", "/lib64"])
            .arg(first)
            .arg(&dir)
            .arg(second)
            .arg(&dir)
            .arg("--")
            .arg("/bin/sh")
            .arg("-c")
            .arg(format!("printf inside > '{}/written.txt'", dir.display()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{tag}: the path must stay writable: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(dir.join("written.txt")).unwrap_or_default(),
            "inside",
            "{tag}: the duplicate rule must not downgrade to read-only"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn a_missing_rule_path_refuses_the_launch() {
    // `shim_arguments` keeps a rule for a path that no longer exists, so the
    // launcher and the shim always agree on one rule set. The shim then fails
    // closed when it cannot open the path instead of running the app with a
    // silently different ruleset. On hosts without Landlock the refusal is the
    // same exit code for the same reason (fail-closed), so only the Linux
    // stderr assertion below is kernel-dependent.
    let dir = temp_dir("missing-rule");
    let marker = dir.join("ran");
    let program = dir.join("app");
    fs::write(&program, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let missing = "/nonexistent/aether/sandbox/path";
    let output = Command::new(SHIM)
        .args(["--ro", "/usr", "--ro", missing])
        .arg("--")
        .arg(&program)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(REFUSED),
        "a rule the kernel cannot open must refuse the launch"
    );
    assert!(!marker.exists(), "the program must not run");
    let probe = Command::new(SHIM).arg("--probe").output().unwrap();
    if cfg!(target_os = "linux") && probe.status.success() {
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(missing),
            "the refusal names the path it could not open: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let _ = fs::remove_dir_all(dir);
}
