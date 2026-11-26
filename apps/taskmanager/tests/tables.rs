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
use aether_taskmanager::tabs::{
    app_history_view, details_view, services_view, startup_view, users_view,
};
use aether_taskmanager::window::ActivityManagerWindow;
use aether_taskmanager::ActivityManager;

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

    // --- app history ------------------------------------------------------
    let mut analytics = aether_powerd::analytics::BatteryAnalytics::new(16);
    let now = 5 * 86_400;
    analytics.record_app_energy(now - 60, "org.aether.Browser", 12.0);
    let view = app_history_view(Some(&analytics), now);
    window.bind_tabs(&tabs, Tab::AppHistory);
    window.bind_table(&view);
    assert_eq!(ui.get_active_tab().to_string(), "table");
    assert_eq!(ui.get_table_title().to_string(), "APP HISTORY");
    assert_eq!(ui.get_table_headers().row_count(), view.headers.len());
    assert_eq!(ui.get_table_rows().row_count(), 1);
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_count(), 5);
    assert_eq!(row.cells.row_data(0).unwrap().to_string(), "org.aether.Browser");
    assert_eq!(row.cells.row_data(3).unwrap().to_string(), "High");
    assert_eq!(row.tone.to_string(), "danger");
    assert!(ui.get_table_note().to_string().contains("Pack D"));

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
    assert_eq!(row.action_label.to_string(), "Disable");
    assert_eq!(row.action.to_string(), desktop.to_string_lossy());

    // The button wiring maps the action id back to the caller.
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let received = received.clone();
        window.on_row_action(move |action| received.lock().unwrap().push(action));
    }
    ui.invoke_row_action(row.action.clone());
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
    let services = vec![aether_taskmanager::ServiceRow {
        unit: "aether-powerd.service".into(),
        load: "loaded".into(),
        active: "active".into(),
        sub: "running".into(),
        description: "AetherOS power daemon".into(),
    }];
    let view = services_view(&services);
    window.bind_table(&view);
    assert_eq!(ui.get_table_title().to_string(), "SERVICES");
    let row = ui.get_table_rows().row_data(0).unwrap();
    assert_eq!(row.cells.row_data(0).unwrap().to_string(), "aether-powerd.service");
    assert_eq!(row.tone.to_string(), "success");
    assert_eq!(ui.get_table_summary().to_string(), "1 active of 1");

    // --- empty table keeps its explanatory note ----------------------------
    let empty = services_view(&[]);
    window.bind_table(&empty);
    assert_eq!(ui.get_table_rows().row_count(), 0);
    assert!(ui.get_table_empty_text().to_string().contains("systemctl"));
    assert!(ui.get_table_note().to_string().contains("Read-only"));

    let _ = fs::remove_dir_all(root);
}
