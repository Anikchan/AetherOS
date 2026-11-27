//! Headless interaction test for the Calculator (P4-T11).
//!
//! Clicks real keypad buttons and asserts the Rust-side evaluation, history and
//! memory state plus the properties the window shows. One test per file
//! because the testing backend installs a process-wide platform.

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model};

use aether_calculator::{AngleMode, CalculatorApp, CalculatorController};

/// Click the visible element with this accessible label.
async fn click(ui: &CalculatorApp, label: &str) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |h: &ElementHandle| h.size().width * h.size().height;
        area(b).partial_cmp(&area(a)).unwrap()
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn calculator_keypad_evaluates_and_remembers() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let ui = CalculatorApp::new().unwrap();
        let controller = CalculatorController::new(ui);
        controller.ui().show().expect("window shows");
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(420, 720));

        assert_eq!(controller.history_len(), 0);

        // 7 × 8 = 56 through real key clicks.
        click(controller.ui(), "7").await;
        click(controller.ui(), "×").await;
        click(controller.ui(), "8").await;
        assert_eq!(controller.expression(), "7*8");
        assert_eq!(
            controller.result(),
            "56",
            "live preview evaluates as you type"
        );
        click(controller.ui(), "=").await;

        assert_eq!(controller.result(), "56");
        assert_eq!(controller.ui().get_result().as_str(), "56");
        assert!(!controller.has_error());
        assert_eq!(controller.history_len(), 1);
        assert_eq!(controller.ui().get_history().row_count(), 1);
        assert_eq!(
            controller
                .ui()
                .get_history()
                .row_data(0)
                .unwrap()
                .expression,
            "7*8"
        );

        // Clicking a history row recalls its expression into the display.
        click(controller.ui(), "7*8").await;
        assert_eq!(controller.expression(), "7*8");
        assert_eq!(controller.result(), "56");

        // Memory add stores the last result and the pill follows.
        click(controller.ui(), "M+").await;
        assert_eq!(controller.memory(), 56.0);
        assert_eq!(controller.ui().get_memory_label().as_str(), "M 56");

        // A division by zero shows a clear error instead of a number.
        click(controller.ui(), "C").await;
        click(controller.ui(), "9").await;
        click(controller.ui(), "÷").await;
        click(controller.ui(), "0").await;
        click(controller.ui(), "=").await;
        assert!(controller.has_error());
        assert_eq!(controller.result(), "Division by zero");
        assert!(controller.ui().get_has_error());
        assert_eq!(
            controller.history_len(),
            1,
            "errors are not kept in history"
        );

        // Clearing the history empties the model the window renders.
        click(controller.ui(), "Clear history").await;
        assert_eq!(controller.history_len(), 0);
        assert_eq!(controller.ui().get_history().row_count(), 0);

        // Scientific mode (DA-02 v1): degrees by default, sin(30) = 0.5.
        assert!(controller.scientific());
        assert!(controller.ui().get_scientific());
        assert_eq!(controller.angle_mode(), AngleMode::Degrees);
        assert_eq!(controller.ui().get_angle_mode().as_str(), "DEG");
        click(controller.ui(), "C").await;
        click(controller.ui(), "sin(").await;
        click(controller.ui(), "3").await;
        click(controller.ui(), "0").await;
        click(controller.ui(), ")").await;
        click(controller.ui(), "=").await;
        assert_eq!(controller.result(), "0.5");
        assert!(!controller.has_error());

        // The toggle key switches to radians and the label follows.
        click(controller.ui(), "Toggle angle unit").await;
        assert_eq!(controller.angle_mode(), AngleMode::Radians);
        assert_eq!(controller.ui().get_angle_mode().as_str(), "RAD");

        // Factorial comes from the keypad, not a text shortcut.
        click(controller.ui(), "C").await;
        click(controller.ui(), "5").await;
        click(controller.ui(), "x!").await;
        assert_eq!(controller.expression(), "5!");
        click(controller.ui(), "=").await;
        assert_eq!(controller.result(), "120");

        // Hiding the scientific keypad is a header click.
        click(controller.ui(), "Scientific mode").await;
        assert!(!controller.scientific());
        assert!(!controller.ui().get_scientific());
    })
    .unwrap();
}
