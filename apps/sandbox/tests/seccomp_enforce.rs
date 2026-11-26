//! End-to-end seccomp test (P6-T04 final layer).
//!
//! Drives the real `aether-landlock` shim with `--seccomp`:
//! - on every host the shim must refuse (exit 126, program never run) when the
//!   kernel cannot install the filter — the fail-closed contract;
//! - on Linux with Landlock and seccomp it must still run an ordinary program,
//!   and must reject a syscall from the deny list.
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
        "aether-seccomp-it-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn shim(args: &[&str]) -> std::process::Output {
    Command::new(SHIM).args(args).output().unwrap()
}

fn kernels_ready() -> bool {
    let probe = shim(&["--probe", "--json"]);
    if !probe.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&probe.stdout);
    text.contains("\"state\": \"available\"") && text.contains("\"available\": true")
}

#[test]
fn seccomp_filter_never_runs_a_program_it_cannot_protect() {
    // No --seccomp: the shim behaves as the Landlock-only shim (covered by the
    // Landlock test). With --seccomp on a host without kernel support, the app
    // must not start.
    if kernels_ready() {
        eprintln!("skipped refusal assertion: this kernel can enforce seccomp");
        return;
    }
    let dir = temp_dir("refuse");
    let marker = dir.join("ran");
    let program = dir.join("app");
    fs::write(&program, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let output = shim(&[
        "--seccomp",
        "--ro",
        "/usr",
        "--ro",
        dir.to_str().unwrap(),
        "--",
        program.to_str().unwrap(),
    ]);
    assert_eq!(
        output.status.code(),
        Some(REFUSED),
        "the shim refuses instead of running without its filters"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("landlock") || stderr.to_lowercase().contains("seccomp"),
        "the refusal names the layer: {stderr}"
    );
    assert!(!marker.exists(), "the program must not run");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn seccomp_filter_runs_ordinary_programs_and_denies_blocked_syscalls() {
    if !cfg!(target_os = "linux") || !kernels_ready() {
        eprintln!("skipped: seccomp/Landlock unavailable on this host");
        return;
    }

    let dir = temp_dir("rules");
    let allowed = dir.join("allowed.txt");
    fs::write(&allowed, "inside").unwrap();
    let base = [
        "--seccomp",
        "--ro",
        "/usr",
        "--ro",
        "/bin",
        "--ro",
        "/lib",
        "--ro",
        "/lib64",
        "--ro",
        "/etc",
        "--ro",
        dir.to_str().unwrap(),
    ];

    // An ordinary program still runs under the filter.
    let mut args: Vec<&str> = base.to_vec();
    args.extend(["--", "/bin/cat", allowed.to_str().unwrap()]);
    let output = shim(&args);
    assert!(
        output.status.success(),
        "a filtered program must still run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "inside");

    // A syscall from the deny list is rejected with EPERM. `personality` is
    // unprivileged, so `setarch -R` succeeds only when the filter is absent.
    let setarch = Path::new("/usr/bin/setarch");
    if !setarch.exists() {
        eprintln!("skipped denial assertion: setarch is not installed");
        let _ = fs::remove_dir_all(dir);
        return;
    }
    let mut unfiltered: Vec<&str> = Vec::new();
    unfiltered.extend([
        "--ro", "/usr", "--ro", "/bin", "--ro", "/lib", "--ro", "/lib64", "--ro", "/etc",
    ]);
    unfiltered.extend(["--", "/usr/bin/setarch", "--addr-no-randomize", "/bin/true"]);
    let without = shim(&unfiltered);
    assert!(
        without.status.success(),
        "setarch must work without the filter: {}",
        String::from_utf8_lossy(&without.stderr)
    );

    let mut filtered: Vec<&str> = unfiltered.to_vec();
    filtered.insert(0, "--seccomp");
    let with = shim(&filtered);
    assert!(
        !with.status.success(),
        "personality() must be denied by the filter"
    );
    assert!(
        String::from_utf8_lossy(&with.stdout).trim().is_empty(),
        "nothing may run after the denial"
    );

    let _ = fs::remove_dir_all(dir);
}
