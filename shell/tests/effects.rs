//! Effect-tier control tests through the real `aether-shell` binary (P4-T04).
//!
//! The control centre and the compositor meet at one environment variable
//! (`AETHER_COMPOSITOR_EFFECT_TIER`) and the compositor's own markers. These
//! tests drive the shipped `--effects-tier`, `--effects-env` and
//! `--effects-check` commands against a temporary `XDG_CONFIG_HOME`, so the
//! on-device acceptance harness runs exactly the code path verified here.
//!
//! The log fixtures are the compositor's P3-T05/T06 marker format; the
//! compositor's own `p3_t05_t06_effects_frames` test proves it emits them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-effects-cli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Write `theme/tokens.toml` into a temporary `XDG_CONFIG_HOME` and return it.
fn config_home(tag: &str, tokens: &str) -> PathBuf {
    let home = temp_dir(tag);
    let theme = home.join("aether").join("theme");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("tokens.toml"), tokens).unwrap();
    home
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
    std::fs::write(&path, text).unwrap();
    path
}

/// A compositor log at `tier` that satisfies every effects/frame check.
fn compositor_log(tier: &str, est_ms: &str) -> String {
    format!(
        "AETHER_COMPOSITOR_OUTPUT_COUNT: 1\n\
         AETHER_COMPOSITOR_FRAME_RATE: 60\n\
         AETHER_COMPOSITOR_EFFECT_TIER: {tier}\n\
         AETHER_COMPOSITOR_EFFECT_PIPELINE: tier={tier} blur=on\n\
         AETHER_COMPOSITOR_FRAME: n=1 time_ms=16 damage_rects=1 damage_px=4096 windows=2 tier={tier} est_ms={est_ms}\n\
         AETHER_COMPOSITOR_FRAME_CALLBACKS: n=4 time_ms=16\n\
         AETHER_COMPOSITOR_IDLE: idle_ms=820 idle_pct=95.1 frames=9\n\
         AETHER_COMPOSITOR_FRAME_STATS: frames=9 idle_skips=41 idle_ms=820 idle_pct=95.1 presented_px=36864 avg_gap_ms=16.01 last_gap_ms=16.00 target_fps=60\n\
         AETHER_COMPOSITOR_EFFECT_SUMMARY: tier={tier} tier_switches=0 frame_callbacks=4\n\
         AETHER_COMPOSITOR_DONE\n"
    )
}

#[test]
fn a_fresh_profile_selects_the_full_tier() {
    let home = temp_dir("default");
    std::fs::create_dir_all(&home).unwrap();

    let output = shell_with_config(&home, &["--effects-tier"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "full\n");

    let output = shell_with_config(&home, &["--effects-env"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("export AETHER_COMPOSITOR_EFFECT_TIER=full"),
        "{text}"
    );
    assert!(text.contains("requested=full"), "{text}");

    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_persisted_setting_drives_the_env_contract() {
    let home = config_home(
        "persisted",
        "[theme]\neffects-tier = \"minimal\"\nreduce-transparency = false\n",
    );

    let output = shell_with_config(&home, &["--effects-tier"]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "minimal\n");

    let output = shell_with_config(&home, &["--effects-env"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("export AETHER_COMPOSITOR_EFFECT_TIER=minimal"),
        "{text}"
    );

    // The compositor agreed: the check passes end to end.
    let log = temp_log("ok", &compositor_log("minimal", "1.100"));
    let output = shell_with_config(&home, &["--effects-check", log.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(
        text.contains("EFFECT_TIER expected=minimal compositor=minimal switches=0"),
        "{text}"
    );
    assert!(text.contains("EFFECT_FRAMES frames=9 idle_skips=41 idle_pct=95.1"));
    assert!(text.ends_with("AETHER_SHELL_EFFECTS_OK\n"), "{text}");

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(log);
}

#[test]
fn reduce_transparency_forces_minimal_over_the_user_choice() {
    let home = config_home(
        "accessibility",
        "[theme]\neffects-tier = \"full\"\nreduce-transparency = true\n",
    );
    let output = shell_with_config(&home, &["--effects-tier"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "minimal\n",
        "D1 section 9: solid panels are a hard accessibility requirement"
    );
    let output = shell_with_config(&home, &["--effects-env"]);
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("export AETHER_COMPOSITOR_EFFECT_TIER=minimal"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_check_fails_and_names_a_tier_mismatch() {
    let home = config_home("mismatch", "[theme]\neffects-tier = \"reduced\"\n");
    let log = temp_log("mismatch", &compositor_log("full", "2.440"));

    let output = shell_with_config(&home, &["--effects-check", log.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{text}");
    assert!(
        text.contains("AETHER_SHELL_EFFECTS_PROBLEM: compositor runs tier full, the setting resolves to reduced"),
        "{text}"
    );
    assert!(text.contains("restart the compositor to apply it"), "{text}");
    assert!(text.ends_with("AETHER_SHELL_EFFECTS_FAILED\n"), "{text}");

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(log);
}

#[test]
fn the_check_fails_without_frames_idle_or_a_marker_surface() {
    let home = temp_dir("empty");
    std::fs::create_dir_all(&home).unwrap();

    // Not a compositor log at all.
    let empty = temp_log("empty", "hello\n");
    let output = shell_with_config(&home, &["--effects-check", empty.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(text.contains("no AETHER_COMPOSITOR_EFFECT_TIER marker"), "{text}");
    assert!(text.contains("no AETHER_COMPOSITOR_FRAME_STATS marker"), "{text}");

    // A frame-cost overrun is reported with the budget it broke.
    let overrun = temp_log("overrun", &compositor_log("full", "18.400"));
    let output = shell_with_config(&home, &["--effects-check", overrun.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{text}");
    assert!(
        text.contains("18.40 ms exceeds the 16.67 ms budget at tier full"),
        "{text}"
    );

    // A missing file is an error, not a panic.
    let output = shell_with_config(&home, &["--effects-check", "/nonexistent/aether.log"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read compositor log"));

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_file(empty);
    let _ = std::fs::remove_file(overrun);
}

#[test]
fn the_check_reads_stdin_for_the_harness() {
    let home = temp_dir("stdin");
    std::fs::create_dir_all(&home).unwrap();
    let output = shell_stdin(
        &home,
        &["--effects-check", "-"],
        &compositor_log("full", "2.440"),
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.contains("EFFECT_TIER expected=full compositor=full switches=0"));
    assert!(text.ends_with("AETHER_SHELL_EFFECTS_OK\n"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}
