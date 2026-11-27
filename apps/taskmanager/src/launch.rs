//! Launch path selection for the Activity Manager (P5-T07, F13).
//!
//! The CLI contract is unchanged: any flag keeps the text/contract behaviour
//! (`--tab`, `--render`, `--pack-d-history`, `--service-action … --yes`, …).
//! A bare launch — the shell's `<exec> --launch <id>`, whose launch flags the
//! launcher registry strips — opens the Slint window on the Processes tab.

use std::time::Duration;

/// One-second refresh while the window is visible (reduced motion pauses it).
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Which entry point an argument list selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    /// No arguments: open the Slint window and run its event loop.
    Window,
    /// Any argument: keep the existing CLI contract.
    Cli,
}

/// The launch rule. Pure, so the decision is testable without a display.
pub fn launch_mode(args: &[String]) -> LaunchMode {
    if args.is_empty() {
        LaunchMode::Window
    } else {
        LaunchMode::Cli
    }
}

/// `AETHER_SHELL_REDUCE_MOTION` as exported by the shell's a11y env (`1`/`true`/`on`).
pub fn reduce_motion_from(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some("1") | Some("true") | Some("on") | Some("TRUE") | Some("ON")
    )
}

/// The shell exports [`reduce_motion_from`]`(AETHER_SHELL_REDUCE_MOTION)` for apps.
pub fn reduce_motion() -> bool {
    reduce_motion_from(std::env::var("AETHER_SHELL_REDUCE_MOTION").ok().as_deref())
}

/// Marker logged on every live refresh so a headless test can assert the timer
/// ticked. The window prints it once per tick while visible.
pub fn refresh_marker(tick: u64, rows: usize) -> String {
    format!("AETHER_TM_WINDOW: refresh={tick} rows={rows}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn no_flags_open_the_window() {
        assert_eq!(launch_mode(&[]), LaunchMode::Window);
        // `--launch aether-taskmanager` arrives empty: the registry stripped it.
    }

    #[test]
    fn every_cli_flag_keeps_the_cli_contract() {
        for flags in [
            args(&["--once"]),
            args(&["--json"]),
            args(&["--tui"]),
            args(&["--tab", "performance"]),
            args(&["--render", "/tmp/x"]),
            args(&["--pack-d-history", "/tmp/history.json"]),
            args(&[
                "--service-action",
                "aether-demo.service",
                "restart",
                "--yes",
            ]),
            args(&["--limit", "10"]),
            args(&["--help"]),
        ] {
            assert_eq!(launch_mode(&flags), LaunchMode::Cli, "{flags:?}");
        }
    }

    #[test]
    fn reduce_motion_matches_the_shell_environment_values() {
        assert!(reduce_motion_from(Some("1")));
        assert!(reduce_motion_from(Some("true")));
        assert!(reduce_motion_from(Some("on")));
        assert!(reduce_motion_from(Some(" ON ")));
        assert!(!reduce_motion_from(Some("0")));
        assert!(!reduce_motion_from(Some("false")));
        assert!(!reduce_motion_from(None));
    }

    #[test]
    fn refresh_marker_is_stable() {
        assert_eq!(
            refresh_marker(1, 42),
            "AETHER_TM_WINDOW: refresh=1 rows=42".to_string()
        );
        assert_eq!(refresh_marker(0, 0), "AETHER_TM_WINDOW: refresh=0 rows=0");
        assert_eq!(REFRESH_INTERVAL.as_secs(), 1);
    }
}
