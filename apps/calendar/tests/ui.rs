//! Headless interaction test for the Calendar window (P4-T16).
//!
//! One test per file: the Slint testing backend installs a process-wide
//! platform and a second `init` would panic. Every assertion reads either the
//! controller state (Rust) or the generated Slint properties after a real
//! pointer click / text input.

use std::path::PathBuf;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model};

use aether_calendar::calendar::{Date, EventStore, MonthYear};
use aether_calendar::{CalendarApp, CalendarController};

fn temp_store(tag: &str) -> EventStore {
    let base = std::env::temp_dir().join(format!(
        "aether-calendar-ui-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut store = EventStore::new(base.join("calendar").join("events.json"));
    store.load_or_default();
    store
}

/// Click the visible instance with this accessible label.
async fn click(ui: &CalendarApp, label: &str) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |handle: &ElementHandle| handle.size().width * handle.size().height;
        area(b).partial_cmp(&area(a)).unwrap()
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn calendar_window_clicking_updates_rust_state() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let store = temp_store("interaction");
        let store_path: PathBuf = store.path().to_path_buf();
        let ui = CalendarApp::new().unwrap();
        let controller = CalendarController::new(ui, store);
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1200, 800));

        // --- month navigation --------------------------------------------
        let start = MonthYear::from_date(Date::today());
        assert_eq!(controller.view(), start);
        assert_eq!(controller.ui().get_month_label(), start.label());

        click(controller.ui(), "Next month").await;
        let expected = start.shift(1);
        assert_eq!(controller.view(), expected, "Next month advances the view");
        assert_eq!(controller.ui().get_month_label(), expected.label());

        // --- day selection -------------------------------------------------
        let day = Date::new(expected.year, expected.month, 15).unwrap();
        click(controller.ui(), &day.iso()).await;
        assert_eq!(
            controller.selected(),
            day,
            "clicking a day cell selects that day"
        );
        assert_eq!(controller.ui().get_selected_label(), day.long_label());
        assert_eq!(controller.events_on_selected(), 0);

        // --- add an event through the real form ----------------------------
        let title = ElementHandle::find_by_accessible_label(controller.ui(), "Event title")
            .next()
            .expect("event title field is in the tree");
        title.set_accessible_value("Compiler sync");
        let start_time = ElementHandle::find_by_accessible_label(controller.ui(), "Start time")
            .next()
            .expect("start time field is in the tree");
        start_time.set_accessible_value("10:00");
        let end_time = ElementHandle::find_by_accessible_label(controller.ui(), "End time")
            .next()
            .expect("end time field is in the tree");
        end_time.set_accessible_value("11:00");

        click(controller.ui(), "Add event").await;

        assert_eq!(controller.store().len(), 1, "the event reached the store");
        let stored = controller.store().on_day(day);
        assert_eq!(stored.len(), 1);
        assert_eq!(
            (stored[0].title.as_str(), stored[0].start_minute, stored[0].end_minute),
            ("Compiler sync", 10 * 60, 11 * 60)
        );
        assert_eq!(controller.ui().get_events().row_count(), 1);
        assert_eq!(controller.events_on_selected(), 1);
        assert!(controller.ui().get_new_title().is_empty(), "the form resets");
        assert!(std::path::Path::new(&store_path).exists(), "the store is saved");

        // --- remove it again ----------------------------------------------
        click(controller.ui(), "Remove Compiler sync").await;
        assert_eq!(controller.store().len(), 0);
        assert_eq!(controller.ui().get_events().row_count(), 0);

        // --- week tab (v1 week view) ---------------------------------------
        click(controller.ui(), "Week").await;
        assert_eq!(controller.ui().get_tab(), 1, "the segmented control switched");
        assert_eq!(controller.ui().get_week_days().row_count(), 7);
        let week = controller.week();
        assert_eq!(week.start, controller.selected().start_of_week());
        assert_eq!(week.start.weekday_index(), 0, "Monday first");
        let before = controller.selected();
        click(controller.ui(), "Next week").await;
        assert_eq!(
            controller.selected(),
            before.add_days(7),
            "Next week moves the selection by seven days"
        );
        assert!(controller
            .ui()
            .get_week_label()
            .to_string()
            .contains(" – "));
        click(controller.ui(), "Previous week").await;
        assert_eq!(controller.selected(), before);

        // --- clock tab ------------------------------------------------------
        click(controller.ui(), "Clock").await;
        assert_eq!(controller.ui().get_tab(), 2, "the segmented control switches tab");
        assert!(
            controller.ui().get_clock_digital().to_string().contains(':'),
            "the digital clock is formatted"
        );
        assert!(
            controller.ui().get_hands_command().to_string().contains('L'),
            "the analogue hands are real path geometry"
        );

        // --- time zones (DA-07) --------------------------------------------
        assert_eq!(controller.zone().name, "UTC");
        click(controller.ui(), "New Delhi").await;
        assert_eq!(controller.zone().name, "New Delhi");
        assert!(controller
            .ui()
            .get_zone_label()
            .to_string()
            .contains("UTC+05:30"));
        assert!(controller.ui().get_zone_time().to_string().contains(':'));

        // --- alarms (DA-07) -------------------------------------------------
        let alarm_label = ElementHandle::find_by_accessible_label(controller.ui(), "Alarm label")
            .next()
            .expect("alarm label field is in the tree");
        alarm_label.set_accessible_value("Wake up");
        let alarm_time = ElementHandle::find_by_accessible_label(controller.ui(), "Alarm time")
            .next()
            .expect("alarm time field is in the tree");
        alarm_time.set_accessible_value("07:30");
        click(controller.ui(), "Add alarm").await;
        assert_eq!(controller.alarms().len(), 1);
        assert_eq!(controller.ui().get_alarms().row_count(), 1);
        assert!(controller.ui().get_alarms().row_data(0).unwrap().days == "Every day");

        let ringing = controller.poll_alarms(0, 7, 30);
        assert_eq!(ringing, vec!["Wake up · 07:30".to_string()]);
        assert!(
            controller.ui().get_alarm_banner().to_string().contains("Wake up"),
            "the banner shows the ringing alarm"
        );
        assert!(
            controller.poll_alarms(0, 7, 30).is_empty(),
            "an alarm rings once per minute"
        );
        click(controller.ui(), "Dismiss").await;
        assert!(controller.ui().get_alarm_banner().is_empty());

        click(controller.ui(), "Remove alarm Wake up").await;
        assert!(controller.alarms().is_empty());

        let _ = std::fs::remove_dir_all(
            std::path::Path::new(&store_path)
                .parent()
                .unwrap()
                .parent()
                .unwrap(),
        );
        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
