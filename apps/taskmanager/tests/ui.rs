//! Headless test for the Activity Manager window (P5-T07/P5-T15).
//!
//! One `#[test]` per file: the Slint testing backend installs a process-wide
//! platform. The assertions prove that the Energy column — header, null label,
//! level labels, tones and sort direction — comes from the P5-T15 contract and
//! changes when the contract changes, with no Slint edits.
//!
//! The body is synchronous on purpose: property and model reads against the
//! testing backend do not need the event loop, and a plain `assert!` panic fails
//! the test instead of dying inside a spawned task.

use i_slint_backend_testing::ElementQuery;
use slint::{ComponentHandle, Model};

use aether_taskmanager::model::EnergyImpact;
use aether_taskmanager::window::ActivityManagerApp;
use aether_taskmanager::{demo_rows, ActivityManagerWindow};

#[test]
fn energy_column_is_bound_to_the_contract() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let window = ActivityManagerWindow::new().unwrap();
    let contract = window.contract().clone();
    let rows = demo_rows();

    assert_eq!(window.bind(&rows, None, ""), rows.len());
    let ui = window.ui();
    ui.show().expect("window shows");

    // Header, null label and sort direction are contract values.
    assert_eq!(ui.get_energy_header().to_string(), contract.header);
    assert_eq!(ui.get_energy_null_label().to_string(), contract.null_label);
    assert_eq!(ui.get_energy_sort().to_string(), contract.default_sort);
    let headers: Vec<_> = ElementQuery::from_root(ui)
        .match_accessible_label(&contract.header)
        .find_all();
    assert!(
        !headers.is_empty(),
        "the rendered column header shows the contract header"
    );
    assert!(
        headers.iter().any(|handle| handle.size().width > 0.0),
        "the header has a rendered box"
    );

    // Rows are sorted by energy impact (contract default_sort = desc).
    let model = ui.get_rows();
    assert_eq!(model.row_count(), rows.len());
    let first = model.row_data(0).unwrap();
    assert_eq!(
        first.energy_label.to_string(),
        contract.label_for(EnergyImpact::High)
    );
    assert_eq!(
        first.energy_tone.to_string(),
        contract.tone_for(EnergyImpact::High)
    );
    assert_eq!(first.energy_tone.to_string(), "danger");
    let last = model.row_data(model.row_count() - 1).unwrap();
    assert_eq!(
        last.energy_label.to_string(),
        contract.null_label,
        "an app without measured energy shows the contract null label"
    );

    // A changed contract changes the window without Slint edits.
    let mut custom = contract.clone();
    custom.header = "Power".into();
    custom.null_label = "No data".into();
    custom.default_sort = "asc".into();
    for level in &mut custom.levels {
        level.label = format!("{}!", level.label);
    }
    let mut custom_window = ActivityManagerWindow::with_contract(
        ActivityManagerApp::new().unwrap(),
        custom.clone(),
    );
    custom_window.bind(&rows, None, "");
    assert_eq!(custom_window.ui().get_energy_header().to_string(), "Power");
    assert_eq!(custom_window.ui().get_energy_sort().to_string(), "asc");
    let ascending = custom_window.ui().get_rows();
    let first = ascending.row_data(0).unwrap();
    assert_eq!(
        first.energy_label.to_string(),
        custom.null_label,
        "ascending shows the lowest impact first"
    );
    let highest = ascending.row_data(ascending.row_count() - 1).unwrap();
    assert_eq!(
        highest.energy_label.to_string(),
        custom.label_for(EnergyImpact::High)
    );
    assert!(highest.energy_label.to_string().ends_with('!'));

    // Back to the shipped contract.
    custom_window.set_contract(contract.clone());
    assert_eq!(
        custom_window.ui().get_energy_header().to_string(),
        contract.header
    );

    // Search filters by process name.
    let visible = window.bind(&rows, None, "player");
    assert_eq!(visible, 1);
    assert_eq!(window.ui().get_process_count(), 1);
}
