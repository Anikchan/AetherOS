//! Accessibility CLI contract (F1 SHL-18) through the real `aether-shell` binary.
//!
//! `--a11y-status` is what the on-device harness calls: it prints the resolved
//! reduce-motion / reduce-transparency / text-scale values, the 2 px focus
//! indicator contract and the keyboard plan for every shell surface.
//! `--a11y-env` is sourced by the session launcher so apps follow the shell.

use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-a11y-cli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn config_home(tag: &str, tokens: &str) -> PathBuf {
    let home = temp_dir(tag);
    let theme = home.join("aether").join("theme");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("tokens.toml"), tokens).unwrap();
    home
}

fn shell(config_home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aether-shell"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .output()
        .expect("run aether-shell")
}

#[test]
fn a_fresh_profile_reports_accessibility_off_and_full_text() {
    let home = temp_dir("default");
    std::fs::create_dir_all(&home).unwrap();

    let output = shell(&home, &["--a11y-status"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("A11Y_REDUCE_MOTION: off"), "{text}");
    assert!(text.contains("A11Y_REDUCE_TRANSPARENCY: off"), "{text}");
    assert!(text.contains("A11Y_TEXT_SCALE: 100%"), "{text}");
    assert!(text.contains("A11Y_MOTION: slide_ms=180 fade_ms=120 motion_ms=180"), "{text}");
    assert!(text.contains("A11Y_FOCUS_INDICATOR: width=2px"), "{text}");
    // Every shell surface is in the keyboard plan.
    for surface in ["menubar", "dock", "launcher", "control-center", "notifications"] {
        assert!(
            text.contains(&format!("A11Y_KEYBOARD: surface={surface}")),
            "missing {surface}: {text}"
        );
    }
    assert!(text.contains("AETHER_SHELL_FOCUS: surface=menubar"), "{text}");
    assert!(text.trim_end().ends_with("AETHER_SHELL_A11Y_OK"), "{text}");

    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_persisted_settings_drive_the_status_and_the_env_contract() {
    let home = config_home(
        "persisted",
        "[theme]\nreduce-motion = true\nreduce-transparency = true\ntext-scale = \"150%\"\n",
    );

    let output = shell(&home, &["--a11y-status"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("A11Y_REDUCE_MOTION: on"), "{text}");
    assert!(text.contains("A11Y_REDUCE_TRANSPARENCY: on"), "{text}");
    assert!(text.contains("A11Y_TEXT_SCALE: 150% scale=1.50"), "{text}");
    assert!(
        text.contains("A11Y_MOTION: slide_ms=0 fade_ms=120 motion_ms=120"),
        "reduce motion collapses slides to the 120 ms fade: {text}"
    );

    let output = shell(&home, &["--a11y-env"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("export AETHER_SHELL_REDUCE_MOTION=1"), "{text}");
    assert!(text.contains("export AETHER_SHELL_REDUCE_TRANSPARENCY=1"), "{text}");
    assert!(text.contains("export AETHER_SHELL_TEXT_SCALE=1.50"), "{text}");
    assert!(text.contains("export AETHER_SHELL_MOTION_MS=120"), "{text}");
    assert!(text.contains("export AETHER_SHELL_FOCUS_RING_PX=2"), "{text}");
    assert!(text.contains("reduce-motion=true text-scale=150%"), "{text}");

    let _ = std::fs::remove_dir_all(home);
}
