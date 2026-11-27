//! Headless window test for the Activity Manager (P5-T07).
//!
//! The window's timer calls the same refresh path this test calls by hand, so
//! the marker and row binding are asserted without a display. One test per file
//! because the testing backend installs a process-wide platform.

use slint::ComponentHandle;

use aether_taskmanager::collect::ProcFs;
use aether_taskmanager::launch::{launch_mode, refresh_marker, LaunchMode};
use aether_taskmanager::manager::ActivityManager;
use aether_taskmanager::model::Tab;
use aether_taskmanager::window::ActivityManagerWindow;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|value| value.to_string()).collect()
}

#[test]
fn a_bare_launch_opens_the_window_and_every_tick_refreshes() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    // The launch rule: no flags (the registry strips `--launch <id>`) opens the
    // window; any CLI flag keeps the text contract.
    assert_eq!(launch_mode(&[]), LaunchMode::Window);
    for flags in [
        args(&["--once"]),
        args(&["--tab", "processes"]),
        args(&["--tab", "performance"]),
        args(&["--render", "/tmp/x"]),
        args(&["--pack-d-history", "/tmp/history.json"]),
        args(&[
            "--service-action",
            "aether-demo.service",
            "restart",
            "--yes",
        ]),
    ] {
        assert_eq!(launch_mode(&flags), LaunchMode::Cli, "{flags:?}");
    }

    let window = ActivityManagerWindow::new().expect("window builds headlessly");
    window.ui().show().expect("window shows");
    let mut manager = ActivityManager::new(ProcFs::system(), true);

    // Processes is the default tab of the launched window.
    window.bind_tabs(&manager.tabs(), Tab::Processes);
    assert_eq!(window.ui().get_active_tab().as_str(), "processes");

    // Tick 1: the live sampler (or its demo fallback on a host without /proc)
    // produces rows, and the marker names the tick and the row count.
    let first = window.refresh_processes(&mut manager, None, 1_000);
    assert!(first.visible > 0, "rows are bound");
    assert_eq!(
        first.rows.len(),
        first.visible,
        "empty query shows every row"
    );
    assert_eq!(window.ui().get_process_count(), first.visible as i32);
    assert!(!window.ui().get_cpu_summary().is_empty());
    let marker = refresh_marker(1, first.visible);
    assert!(
        marker.starts_with("AETHER_TM_WINDOW: refresh=1 rows="),
        "got {marker:?}"
    );
    assert!(marker.ends_with(&first.visible.to_string()));

    // Tick 2 increments the counter and rebinds the same window.
    let second = window.refresh_processes(&mut manager, None, 2_000);
    assert!(second.visible > 0);
    assert_ne!(refresh_marker(2, second.visible), marker);
    assert_eq!(window.ui().get_process_count(), second.visible as i32);
}
