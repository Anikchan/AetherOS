//! Browser security surface through the real `aether-browser` binary (P4-T18b).
//!
//! `--security-status` / `--security-check` are the acceptance-harness surface
//! for the security baseline and the update channel. The engine version is
//! injected with `AETHER_WEBKITGTK_VERSION` so these tests run on hosts without
//! WebKitGTK and still assert the exact on-image verdicts.

use std::path::PathBuf;
use std::process::Command;

fn temp_config_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "aether-browser-security-cli-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn browser_with_engine(
    config_home: &PathBuf,
    args: &[&str],
    engine: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aether-browser"));
    command
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .env_remove("AETHER_WEBKITGTK_VERSION");
    match engine {
        Some(version) => {
            command.env("AETHER_WEBKITGTK_VERSION", version);
        }
        // An empty PATH makes the `pkg-config` probe fail, which is exactly the
        // "adapter only" host the status command must describe honestly.
        None => {
            command.env("PATH", "");
        }
    }
    command.output().expect("run aether-browser")
}

#[test]
fn a_supported_engine_passes_the_default_baseline() {
    let home = temp_config_home("supported");
    std::fs::create_dir_all(&home).unwrap();
    let output = browser_with_engine(&home, &["--security-status"], Some("2.48.1"));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.starts_with("AETHER_BROWSER_SECURITY_OK\n"), "{text}");
    assert!(text.contains("channel=stable updates=os-image"), "{text}");
    assert!(
        text.contains("https-only=on mixed-content=blocked third-party-cookies=blocked"),
        "{text}"
    );
    assert!(
        text.contains("engine=webkitgtk version=2.48.1 minimum=2.44 verdict=supported"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn an_engine_below_the_security_floor_fails_the_check() {
    let home = temp_config_home("outdated");
    std::fs::create_dir_all(&home).unwrap();
    let output = browser_with_engine(&home, &["--security-check"], Some("2.42.3"));
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(4), "stdout:\n{text}");
    assert!(text.starts_with("AETHER_BROWSER_SECURITY_FAILED\n"), "{text}");
    assert!(text.contains("verdict=outdated"), "{text}");
    assert!(
        text.contains(
            "AETHER_BROWSER_SECURITY_PROBLEM: WebKitGTK 2.42.3 is older than the 2.44 security floor"
        ),
        "{text}"
    );
    assert!(text.contains("update the image (stable channel)"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_adapter_only_host_is_a_note_but_the_strict_check_requires_the_engine() {
    let home = temp_config_home("absent");
    std::fs::create_dir_all(&home).unwrap();

    let output = browser_with_engine(&home, &["--security-status"], None);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.contains("verdict=absent"), "{text}");
    assert!(text.contains("AETHER_BROWSER_SECURITY_NOTE"), "{text}");

    let output = browser_with_engine(&home, &["--security-check"], None);
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(5), "stdout:\n{text}");
    assert!(text.contains("AETHER_BROWSER_SECURITY_FAILED"), "{text}");
    assert!(text.contains("WebKitGTK is not installed"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn the_channel_and_a_weakened_baseline_are_reported() {
    let home = temp_config_home("beta");
    let browser_dir = home.join("aether").join("browser");
    std::fs::create_dir_all(&browser_dir).unwrap();

    // Beta channel with the baseline intact: a pass that names the stream.
    std::fs::write(
        browser_dir.join("security.toml"),
        "[browser]\nchannel = \"beta\"\nhttps-only = true\nmixed-content = \"block\"\n",
    )
    .unwrap();
    let output = browser_with_engine(&home, &["--security-status"], Some("2.48.1"));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "stdout:\n{text}");
    assert!(text.contains("channel=beta updates=os-image"), "{text}");

    // HTTPS-only off is a failing baseline, not a preference.
    std::fs::write(
        browser_dir.join("security.toml"),
        "[browser]\nchannel = \"beta\"\nhttps-only = false\n",
    )
    .unwrap();
    let output = browser_with_engine(&home, &["--security-check"], Some("2.48.1"));
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(5), "stdout:\n{text}");
    assert!(
        text.contains("AETHER_BROWSER_SECURITY_PROBLEM: HTTPS-only is disabled"),
        "{text}"
    );
    assert!(text.contains("https-only=off mixed-content=blocked"), "{text}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_malformed_security_file_is_reported_not_ignored() {
    let home = temp_config_home("malformed");
    let browser_dir = home.join("aether").join("browser");
    std::fs::create_dir_all(&browser_dir).unwrap();
    std::fs::write(browser_dir.join("security.toml"), "not = = toml").unwrap();

    let output = browser_with_engine(&home, &["--security-status"], Some("2.48.1"));
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not valid TOML"), "{stderr}");
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    let _ = std::fs::remove_dir_all(home);
}
