//! Headless test for the generic table tabs (P5-T07: App history, Startup
//! apps, Users, Details, Services).
//!
//! One `#[test]` per file: the Slint testing backend installs a process-wide
//! platform.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use i_slint_backend_testing::ElementQuery;
use slint::{ComponentHandle, Model};

use aether_taskmanager::collect::ProcFs;
use aether_taskmanager::model::Tab;
use aether_taskmanager::pack_d::{AppCpuTime, AppNetworkCounters, AppNetworkHistory};
use aether_taskmanager::tabs::{
    app_history_view, details_view, services_view, startup_view, users_view,
};
use aether_taskmanager::window::ActivityManagerWindow;
use aether_taskmanager::ActivityManager;

/// Column order of the App history table (TM-12). The Pack D columns sit
/// between the energy windows and the impact/share columns.
const APP_HISTORY_HEADERS: [&str; 7] = [
    "App",
    "24 hours",
    "7 days",
    "CPU time",
    "Network",
    "Impact",
    "Share",
];

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aether-tm-table-{tag}-{}-{}",
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
fn table_tabs_render_their_rows_and_actions() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let manager = ActivityManager::new(ProcFs::at("/nonexistent-proc"), true);
    let tabs = manager.tabs();
    let window = ActivityManagerWindow::new().unwrap();
    let ui = window.ui();

    // --- app history: Pack D columns in the published order ---------------
    let mut analytics = aether_powerd::analytics::BatteryAnalytics::new(16);
    let now = 5 * 86_400;
    analytics.record_app_energy(now - 60, "org.aether.Browser", 12.0);
    // Pack D: CPU time integrated from the process samples, network from the
    // snapshot-derived counters the daemon persists.
    let demo = aether_taskmanager::demo_rows();
    let browser = demo[0].clone();
    assert_eq!(browser.cpu_percent, 62.5);
    let mut cpu = AppCpuTime::default();
    cpu.observe(std::slice::from_ref(&browser), (now - 70) * 1_000);
    cpu.observe(&[browser], (now - 60) * 1_000);
    let mut network = AppNetworkHistory::default();
    network.update(
        &[AppNetworkCounters {
            app: "org.aether.Browser".into(),
            rx_bytes: 1_000,
            tx_bytes: 200,
        }],
        (now - 70) * 1_000,
    );
    network.update(
        &[AppNetworkCounters {
            app: "org.aether.Browser".into(),
            rx_bytes: 5_096,
            tx_bytes: 1_224,
        }],
        (now - 60) * 1_000,
    );

    let view = app_history_view(Some(&analytics), Some(&cpu), Some(&network), now);
    window.bind_tabs(&tabs, Tab::AppHistory);
    window.bind_table(&view);
    assert_eq!(ui.get_active_tab().to_string(), "table");
    assert_eq!(ui.get_table_title().to_string(), "APP HISTORY");
    assert_eq!(ui.get_table_headers().row_count(), view.headers.len());
    assert_eq!(ui.get_table_rows().row_count(), 1);
    // Header order is pinned: the Pack D columns are 3 and 4.
    let headers: Vec<String> = (0..ui.get_table_headers().row_count())
        .map(|index| ui.get_table_headers().row_data(index).unwrap().to_string())
        .collect();
    assert_eq!(headers, APP_HISTORY_HEADERS);
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_count(), 7);
    assert_eq!(row.cells.row_data(0).unwrap().to_string(), "org.aether.Browser");
    assert_eq!(row.cells.row_data(2).unwrap().to_string(), "12.00 Wh");
    // 62.5% for 10 s is 6 CPU-seconds; the network cell is the 4 KiB delta.
    assert_eq!(row.cells.row_data(3).unwrap().to_string(), "6 s");
    assert_eq!(
        row.cells.row_data(4).unwrap().to_string(),
        "4.0 KB in · 1.0 KB out"
    );
    assert_eq!(row.cells.row_data(5).unwrap().to_string(), "High");
    assert_eq!(row.cells.row_data(6).unwrap().to_string(), "100%");
    assert_eq!(row.tone.to_string(), "danger");
    assert!(ui.get_table_note().to_string().contains("Pack D"));

    // Without Pack D the two columns read the contract null label, in place.
    let view = app_history_view(Some(&analytics), None, None, now);
    window.bind_table(&view);
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_data(3).unwrap().to_string(), "Not reported");
    assert_eq!(row.cells.row_data(4).unwrap().to_string(), "Not reported");

    // Render proof: both Pack D headers are laid out, in that order.
    window.bind_table(&app_history_view(Some(&analytics), Some(&cpu), Some(&network), now));
    ui.show().expect("window shows");
    let pack_d_headers: Vec<String> = ["CPU time", "Network"]
        .into_iter()
        .map(|label| {
            let found: Vec<_> = ElementQuery::from_root(ui)
                .match_accessible_label(label)
                .find_all();
            assert!(
                found.iter().any(|handle| handle.size().width > 0.0),
                "the {label} header is rendered"
            );
            label.to_string()
        })
        .collect();
    assert_eq!(pack_d_headers, vec!["CPU time", "Network"]);

    // --- startup apps (with a real enable/disable action id) ---------------
    let root = temp_dir("startup");
    let autostart = root.join("autostart");
    fs::create_dir_all(&autostart).unwrap();
    let desktop = autostart.join("aether-mail.desktop");
    fs::write(
        &desktop,
        "[Desktop Entry]\nType=Application\nName=Mail\nExec=aether-mail\n",
    )
    .unwrap();
    let items = manager.startup_items_at(&autostart, &root.join("system-autostart"));
    assert_eq!(items.len(), 1);
    let view = startup_view(&items);
    window.bind_table(&view);
    assert_eq!(ui.get_table_title().to_string(), "STARTUP APPS");
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_data(0).unwrap().to_string(), "Mail");
    assert_eq!(row.actions.row_count(), 1);
    let action = row.actions.row_data(0).unwrap();
    assert_eq!(action.label.to_string(), "Disable");
    assert_eq!(action.id.to_string(), desktop.to_string_lossy());

    // The button wiring maps the action id back to the caller.
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let received = received.clone();
        window.on_row_action(move |action| received.lock().unwrap().push(action));
    }
    ui.invoke_row_action(action.id.clone());
    assert_eq!(*received.lock().unwrap(), vec![desktop.to_string_lossy().to_string()]);

    // Render proof: the first row is laid out after show().
    ui.show().expect("window shows");
    let rows: Vec<_> = ElementQuery::from_root(ui)
        .match_accessible_label("Mail")
        .find_all();
    assert!(
        rows.iter().any(|handle| handle.size().height > 0.0),
        "the startup row is rendered"
    );

    // --- users ------------------------------------------------------------
    let sessions = vec![
        aether_taskmanager::UserSession {
            user: "anik".into(),
            uid: 1000,
            seat: Some("seat0".into()),
        },
        aether_taskmanager::UserSession {
            user: "build".into(),
            uid: 1001,
            seat: None,
        },
    ];
    let view = users_view(&sessions, 1000);
    window.bind_table(&view);
    assert_eq!(ui.get_table_title().to_string(), "USERS");
    assert_eq!(ui.get_table_rows().row_count(), 2);
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_data(3).unwrap().to_string(), "This session");

    // --- details ----------------------------------------------------------
    let view = details_view(&aether_taskmanager::demo_rows());
    window.bind_table(&view);
    assert_eq!(ui.get_table_title().to_string(), "DETAILS");
    assert_eq!(ui.get_table_rows().row_count(), 5);
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_count(), 7);
    assert_eq!(row.cells.row_data(6).unwrap().to_string(), "App scope");
    assert!(row.hint.to_string().starts_with("/usr/bin/"));

    // --- services ---------------------------------------------------------
    let services = vec![
        aether_taskmanager::ServiceRow {
            unit: "aether-powerd.service".into(),
            load: "loaded".into(),
            active: "active".into(),
            sub: "running".into(),
            description: "AetherOS power daemon".into(),
        },
        aether_taskmanager::ServiceRow {
            unit: "aether-updated.service".into(),
            load: "loaded".into(),
            active: "inactive".into(),
            sub: "dead".into(),
            description: "AetherOS update service".into(),
        },
    ];
    let view = services_view(&services);
    window.bind_table(&view);
    assert_eq!(ui.get_table_title().to_string(), "SERVICES");
    assert_eq!(ui.get_table_headers().row_count(), 4);
    assert_eq!(ui.get_table_rows().row_count(), 2);
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_data(0).unwrap().to_string(), "aether-powerd.service");
    assert_eq!(row.tone.to_string(), "success");
    assert_eq!(ui.get_table_summary().to_string(), "1 active of 2");
    // TM-17: the row carries the prompted Stop/Restart plans, not a command.
    assert_eq!(row.actions.row_count(), 2);
    assert_eq!(
        row.actions.row_data(0).unwrap().id.to_string(),
        "service:stop:aether-powerd.service"
    );
    assert_eq!(row.actions.row_data(0).unwrap().label.to_string(), "Stop");
    assert_eq!(
        row.actions.row_data(1).unwrap().id.to_string(),
        "service:restart:aether-powerd.service"
    );
    assert_eq!(row.actions.row_data(1).unwrap().label.to_string(), "Restart");
    // A stopped unit offers Start instead of Stop.
    let stopped = ui.get_table_rows().row_data(1).unwrap();
    assert_eq!(
        stopped.actions.row_data(0).unwrap().id.to_string(),
        "service:start:aether-updated.service"
    );
    assert_eq!(stopped.actions.row_data(0).unwrap().label.to_string(), "Start");
    assert_eq!(stopped.tone.to_string(), "text_muted");

    // The service action buttons are rendered as buttons on the active tab.
    ui.show().expect("window shows");
    for label in ["Stop", "Restart", "Start"] {
        let found: Vec<_> = ElementQuery::from_root(ui)
            .match_accessible_label(label)
            .find_all();
        assert!(
            found.iter().any(|handle| handle.size().height > 0.0),
            "the {label} service action button is rendered"
        );
    }

    // A button press maps back to the action id the prompt consumes.
    received.lock().unwrap().clear();
    ui.invoke_row_action(ui.get_table_rows().row_data(1).unwrap().actions.row_data(0).unwrap().id);
    assert_eq!(
        *received.lock().unwrap(),
        vec!["service:start:aether-updated.service".to_string()]
    );

    // --- empty table keeps its explanatory note ----------------------------
    let empty = services_view(&[]);
    window.bind_table(&empty);
    assert_eq!(ui.get_table_rows().row_count(), 0);
    assert!(ui.get_table_empty_text().to_string().contains("systemctl"));
    assert!(ui.get_table_note().to_string().contains("permission prompt"));

    let _ = fs::remove_dir_all(root);
}
