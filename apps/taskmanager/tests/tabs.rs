//! Headless test for the Activity Manager tabs, shortcut hint and end-task
//! dialog (P5-T07, F13 TM-02/TM-04..TM-11).
//!
//! One `#[test]` per file: the Slint testing backend installs a process-wide
//! platform. The body is synchronous on purpose — property and model reads
//! against the testing backend do not need the event loop.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use i_slint_backend_testing::ElementQuery;
use slint::{ComponentHandle, Model};

use aether_taskmanager::collect::ProcFs;
use aether_taskmanager::model::{PerformanceHistory, SensorKind, SensorReading, SystemSample, Tab};
use aether_taskmanager::shortcut::KeyChord;
use aether_taskmanager::tabs::{battery_view, performance_view, sensors_view, BatteryTabView, SensorsTabView};
use aether_taskmanager::window::ActivityManagerWindow;
use aether_taskmanager::{ActivityManager, ShortcutBinding};

#[test]
fn tabs_shortcut_and_dialog_are_bound_to_the_core() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let manager = ActivityManager::new(ProcFs::at("/nonexistent-proc"), true);
    let tabs = manager.tabs();
    let window = ActivityManagerWindow::new().unwrap();
    window.bind_tabs(&tabs, Tab::Performance);
    let ui = window.ui();

    // --- tab bar ---------------------------------------------------------
    assert_eq!(ui.get_tab_labels().row_count(), tabs.len());
    assert_eq!(ui.get_active_tab().to_string(), "performance");
    assert_eq!(ui.get_tab_index(), 1);

    // --- shortcut contract ------------------------------------------------
    let binding = ShortcutBinding::default();
    window.bind_shortcut(&binding, true);
    assert_eq!(ui.get_shortcut_label().to_string(), "Ctrl+Shift+Esc");
    window.bind_shortcut(&binding, false);
    assert_eq!(ui.get_shortcut_label().to_string(), "Super+Escape");
    let pressed = KeyChord::from_event("\u{1b}", true, true, false, false).unwrap();
    assert!(binding.matches(&pressed, true));

    // --- performance tab --------------------------------------------------
    let system = SystemSample {
        cpu_total_percent: 50.0,
        per_core_percent: vec![20.0, 80.0],
        mem_total_bytes: 8_000,
        mem_available_bytes: 3_000,
        governor: Some("powersave".into()),
        uptime_secs: 3_600,
        gpu_busy_percent: Some(10.0),
        ..Default::default()
    };
    let mut history = PerformanceHistory::default();
    history.push(&system, 1_000);
    let view = performance_view(&system, &history);
    window.bind_performance(&view);
    assert_eq!(ui.get_graph_cards().row_count(), view.cards.len());
    let cpu_card = ui.get_graph_cards().row_data(0).unwrap();
    assert_eq!(cpu_card.key.to_string(), "cpu");
    assert_eq!(cpu_card.current.to_string(), "50.0%");
    assert_eq!(cpu_card.values.row_count(), 1);
    assert_eq!(ui.get_cpu_lines().row_count(), 6);
    assert_eq!(ui.get_core_bars().row_count(), 2);
    assert_eq!(ui.get_memory_lines().row_count(), 6);

    // --- battery tab ------------------------------------------------------
    let battery = BatteryTabView {
        percent: "79%".into(),
        state: "Discharging".into(),
        power: "9.8 W".into(),
        ..battery_view(None, None, 0)
    };
    window.bind_battery(&battery);
    assert_eq!(ui.get_battery_percent().to_string(), "79%");
    assert_eq!(ui.get_battery_state().to_string(), "Discharging");
    assert_eq!(ui.get_battery_lines().row_count(), 6);
    // Without history the note explains the contract's null label.
    assert!(ui.get_battery_note().to_string().contains("No battery data"));

    // --- sensors tab ------------------------------------------------------
    let sensors: SensorsTabView = sensors_view(
        &[
            SensorReading {
                name: "coretemp/Package id 0".into(),
                kind: SensorKind::TemperatureC,
                value: 96.0,
            },
            SensorReading {
                name: "thinkpad/fan1".into(),
                kind: SensorKind::FanRpm,
                value: 2_800.0,
            },
        ],
        &system,
    );
    window.bind_sensors(&sensors);
    assert_eq!(ui.get_temperature_sensors().row_count(), 1);
    assert_eq!(ui.get_fan_sensors().row_count(), 1);
    assert_eq!(ui.get_sensor_alerts().row_count(), 1);
    assert_eq!(ui.get_sensor_gpu_lines().row_count(), 2);

    // --- rendered layout --------------------------------------------------
    // Re-bind the tab bar so the Sensors tab is the active one, then show.
    window.bind_tabs(&tabs, Tab::Sensors);
    assert_eq!(ui.get_active_tab().to_string(), "sensors");
    ui.show().expect("window shows");
    let tab_labels: Vec<_> = ElementQuery::from_root(ui)
        .match_accessible_label("Sensors")
        .find_all();
    assert!(!tab_labels.is_empty(), "the Sensors tab button is rendered");
    let sensor_rows: Vec<_> = ElementQuery::from_root(ui)
        .match_accessible_label("coretemp/Package id 0")
        .find_all();
    assert!(
        sensor_rows.iter().any(|handle| handle.size().height > 0.0),
        "the sensor row is laid out on the active tab"
    );
    let sections: Vec<_> = ElementQuery::from_root(ui)
        .match_accessible_label("TEMPERATURES")
        .find_all();
    assert!(!sections.is_empty(), "the temperatures section is rendered");

    // --- end-task / force-quit dialog --------------------------------------
    let request = manager.request_end_task(4242, "AetherMail", true);
    let dialog = request.dialog().expect("hung apps need a dialog").clone();
    let calls: Arc<Mutex<Vec<(u32, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let calls = calls.clone();
        window.on_end_task(move |pid, force| {
            calls.lock().unwrap().push((pid, force));
        });
    }
    window.show_dialog(4242, &dialog);
    assert!(window.dialog_visible());
    assert_eq!(ui.get_dialog_title().to_string(), dialog.title);
    assert_eq!(ui.get_dialog_message().to_string(), dialog.message);
    assert_eq!(ui.get_dialog_confirm_label().to_string(), "Force Quit");
    assert!(ui.get_dialog_hung());
    assert_eq!(ui.get_dialog_pid(), 4242);

    // Cancel closes the dialog without ending anything.
    ui.invoke_dialog_choice(0);
    assert!(!window.dialog_visible());
    assert!(calls.lock().unwrap().is_empty());

    // "End task" sends SIGTERM (force = false).
    window.show_dialog(4242, &dialog);
    ui.invoke_dialog_choice(1);
    assert!(!window.dialog_visible());

    // "Force Quit" sends SIGKILL (force = true).
    window.show_dialog(4242, &dialog);
    ui.invoke_dialog_choice(2);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(4242, false), (4242, true)],
        "the dialog maps its buttons to signals"
    );
    // --- shortcut callback ------------------------------------------------
    let activated = Arc::new(AtomicUsize::new(0));
    {
        let activated = activated.clone();
        window.on_shortcut(move || {
            activated.fetch_add(1, Ordering::Relaxed);
        });
    }
    ui.invoke_shortcut_activated();
    assert_eq!(activated.load(Ordering::Relaxed), 1);
}
