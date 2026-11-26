//! Layer-shell host acceptance through the real `aether-shell` binary (P3-T02).
//!
//! The shell plane and the compositor plane meet at the protocol. This test
//! drives the shipped `--host-check` / `--host-plan` commands against
//! compositor logs written in the compositor's own marker format
//! (`AETHER_COMPOSITOR_OUTPUT`, `AETHER_COMPOSITOR_LAYER_SURFACE`,
//! `AETHER_COMPOSITOR_LAYER_CONFIGURE`), so the on-device acceptance harness
//! runs exactly the code path verified here.
//!
//! The log fixtures are the contract; the compositor's own P3-T02 test proves
//! the server emits these markers for a real `zwlr_layer_shell_v1` client.

use std::path::PathBuf;
use std::process::Command;

use aether_shell::host::{plan, BindingReport, ANCHOR_LEFT, ANCHOR_RIGHT, ANCHOR_TOP};
use aether_shell::layers::required_namespaces;

const OUTPUT: &str = "AETHER_COMPOSITOR_OUTPUT: name=aether-headless-0 mode=1920x1080 scale=1 logical=1920x1080 position=0,0";

fn temp_log(tag: &str, text: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "aether-layer-host-{tag}-{}-{}.log",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, text).unwrap();
    path
}

/// A log in the compositor's marker format for every planned surface.
fn complete_log(output: (i32, i32)) -> String {
    let mut log = format!("{OUTPUT}\n");
    for request in plan() {
        let size = request.negotiate(output);
        log.push_str(&format!(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace={} layer={} output=aether-headless-0 scale=1\n",
            request.namespace, request.layer_name
        ));
        log.push_str(&format!(
            "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace={} size={}x{}\n",
            request.namespace, size.0, size.1
        ));
    }
    log.push_str("AETHER_COMPOSITOR_CLIENT_MAPPED\nAETHER_COMPOSITOR_DONE\n");
    log
}

fn shell(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(args)
        .output()
        .expect("run aether-shell")
}

#[test]
fn host_plan_covers_every_required_surface() {
    let plan = plan();
    for namespace in required_namespaces() {
        let request = plan
            .iter()
            .find(|request| request.namespace == *namespace)
            .unwrap_or_else(|| panic!("{namespace} is missing from the host plan"));
        assert!(
            request.namespace.starts_with("aether-shell-"),
            "namespaces are stable for the compositor log: {}",
            request.namespace
        );
    }
    let menubar = plan.iter().find(|request| request.name == "menubar").unwrap();
    assert_eq!(menubar.anchors, ANCHOR_TOP | ANCHOR_LEFT | ANCHOR_RIGHT);

    let output = shell(&["--host-plan"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for namespace in required_namespaces() {
        assert!(
            text.contains(&format!("namespace={namespace}")),
            "--host-plan must list {namespace}:\n{text}"
        );
    }
    assert!(text.ends_with("AETHER_SHELL_LAYER_PLAN_OK\n"));
}

#[test]
fn host_check_passes_a_complete_compositor_log() {
    let path = temp_log("ok", &complete_log((1920, 1080)));
    let output = shell(&["--host-check", path.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.contains("LAYER_HOST output=aether-headless-0 logical=1920x1080"));
    assert!(text.contains("LAYER_SURFACE name=menubar namespace=aether-shell-menubar bound=true"));
    assert!(text.contains("configured=1920x28 expected=1920x28 ok=true"));
    assert!(text.contains("configured=720x64 expected=720x64 ok=true"));
    assert!(text.ends_with("AETHER_SHELL_LAYER_HOST_OK\n"), "{text}");
    std::fs::remove_file(path).ok();
}

#[test]
fn host_check_fails_and_names_the_problem() {
    // A log where the notifications surface never bound and the dock was
    // configured at the wrong height.
    let mut log = complete_log((1920, 1080));
    log = log
        .lines()
        .filter(|line| !line.contains("namespace=aether-shell-notifications"))
        .collect::<Vec<_>>()
        .join("\n");
    log = log.replace(
        "namespace=aether-shell-dock size=720x64",
        "namespace=aether-shell-dock size=720x40",
    );
    let path = temp_log("bad", &log);
    let output = shell(&["--host-check", path.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{text}");
    assert!(text.contains("AETHER_SHELL_LAYER_HOST_FAILED"), "{text}");
    assert!(
        text.contains("notifications was never created"),
        "the missing surface is named:\n{text}"
    );
    assert!(
        text.contains("dock configured at 720x40, the plan expects 720x64"),
        "the size drift is named:\n{text}"
    );

    // Not a compositor log at all.
    let empty = temp_log("empty", "hello\n");
    let output = shell(&["--host-check", empty.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("no AETHER_COMPOSITOR_OUTPUT marker"));

    // A missing file is an error, not a panic.
    let mut missing = path.clone();
    missing.set_file_name("aether-layer-host-does-not-exist.log");
    let output = shell(&["--host-check", missing.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read compositor log"));

    std::fs::remove_file(path).ok();
    std::fs::remove_file(empty).ok();
}

#[test]
fn host_check_reads_stdin_and_scales_for_hidpi() {
    use std::io::Write;
    use std::process::Stdio;

    // 2880x1800 at 1.5x → 1920x1200 logical: the panel is still 28 tall and
    // stretches to the logical width, which is what the shell draws.
    let mut log = format!(
        "AETHER_COMPOSITOR_OUTPUT: name=eDP-1 mode=2880x1800 scale=1.5 logical=1920x1200 position=0,0\n"
    );
    for request in plan() {
        let size = request.negotiate((1920, 1200));
        log.push_str(&format!(
            "AETHER_COMPOSITOR_LAYER_SURFACE: namespace={} layer={} output=eDP-1 scale=1.5\n",
            request.namespace, request.layer_name
        ));
        log.push_str(&format!(
            "AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace={} size={}x{}\n",
            request.namespace, size.0, size.1
        ));
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(["--host-check", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn aether-shell");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(log.as_bytes())
        .expect("write log to stdin");
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.contains("LAYER_HOST output=eDP-1 logical=1920x1200"));
    assert!(text.contains("name=menubar namespace=aether-shell-menubar bound=true configured=1920x28"));
    assert!(text.ends_with("AETHER_SHELL_LAYER_HOST_OK\n"));
}

#[test]
fn report_scan_flags_a_surface_configured_before_it_bound() {
    // Defensive: a configure line with no matching layer-surface line must not
    // be reported as a working surface.
    let mut log = String::from(OUTPUT);
    log.push('\n');
    log.push_str("AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-menubar size=1920x28\n");
    let report = BindingReport::scan(&log);
    let menubar = report
        .surfaces
        .iter()
        .find(|surface| surface.name == "menubar")
        .unwrap();
    assert!(!menubar.bound);
    assert_eq!(menubar.configured_size, Some((1920, 28)));
    assert!(!menubar.is_ok());
    assert!(!report.is_complete());
}
