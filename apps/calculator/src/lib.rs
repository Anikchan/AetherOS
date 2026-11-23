//! AetherOS Calculator (P4-T11, DA-AC-03).
//!
//! * [`eval`] — recursive-descent expression evaluator with typed errors and
//!   float-noise-free formatting.
//! * [`history`] — bounded result history (F8: last 20) and the memory register.
//! * [`CalculatorController`] — the only place that mutates Slint properties.
//!
//! The window forwards key presses; every result shown is produced by `eval`.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

pub mod eval;
pub mod history;

pub use eval::{evaluate, evaluate_display, format_number, EvalError};
pub use history::{Entry, History, HISTORY_LIMIT};

slint::include_modules!();

struct State {
    expression: String,
    /// Last value that evaluated cleanly, used by M+/M- and by MR recall.
    value: f64,
    result: String,
    has_error: bool,
    /// Next digit starts a fresh expression (set after `=` or MR).
    fresh: bool,
    history: History,
}

/// Owns the calculator state and mirrors it into the window.
pub struct CalculatorController {
    ui: CalculatorApp,
    state: RefCell<State>,
}

impl CalculatorController {
    pub fn new(ui: CalculatorApp) -> Rc<CalculatorController> {
        let controller = Rc::new(CalculatorController {
            ui,
            state: RefCell::new(State {
                expression: String::new(),
                value: 0.0,
                result: "0".to_string(),
                has_error: false,
                fresh: false,
                history: History::new(),
            }),
        });
        controller.wire();
        controller.sync();
        controller
    }

    /// Demo state for the screenshot renderer: an expression, a result and a
    /// few history rows.
    pub fn with_demo(ui: CalculatorApp) -> Rc<CalculatorController> {
        let controller = CalculatorController::new(ui);
        {
            let mut state = controller.state.borrow_mut();
            for (expression, value) in [
                ("sqrt(144) + 2^5", 44.0),
                ("(18 - 6) / 4", 3.0),
                ("pi * 4.5", std::f64::consts::PI * 4.5),
                ("0.1 + 0.2", 0.1 + 0.2),
                ("12 + 30 * 0.5", 27.0),
            ] {
                state.history.push(expression, value);
            }
            state.expression = "12 + 30 * 0.5".to_string();
            state.result = "27".to_string();
            state.value = 27.0;
            state.fresh = true;
            state.history.memory_add(27.0);
        }
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &CalculatorApp {
        &self.ui
    }

    pub fn expression(&self) -> String {
        self.state.borrow().expression.clone()
    }

    pub fn result(&self) -> String {
        self.state.borrow().result.clone()
    }

    pub fn has_error(&self) -> bool {
        self.state.borrow().has_error
    }

    pub fn history_len(&self) -> usize {
        self.state.borrow().history.len()
    }

    pub fn memory(&self) -> f64 {
        self.state.borrow().history.memory()
    }

    pub fn memory_label(&self) -> String {
        self.state.borrow().history.memory_label()
    }

    /// Process one key from the keypad or the keyboard.
    pub fn press(&self, key: &str) {
        {
            let mut state = self.state.borrow_mut();
            match key {
                "C" => {
                    state.expression.clear();
                    state.result = "0".to_string();
                    state.value = 0.0;
                    state.has_error = false;
                    state.fresh = false;
                }
                "BACK" | "⌫" => {
                    state.expression.pop();
                    state.fresh = false;
                    state.has_error = false;
                    preview(&mut state);
                }
                "=" => evaluate_now(&mut state),
                "MC" => state.history.memory_clear(),
                "MR" => {
                    let memory = state.history.memory();
                    state.expression = format_number(memory);
                    state.result = state.expression.clone();
                    state.value = memory;
                    state.has_error = false;
                    state.fresh = true;
                }
                "M+" => {
                    let value = state.value;
                    state.history.memory_add(value);
                    state.fresh = false;
                }
                "M-" | "M−" => {
                    let value = state.value;
                    state.history.memory_subtract(value);
                    state.fresh = false;
                }
                other => {
                    let starts_value = other
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_digit() || ch == '.' || ch == '(');
                    if state.fresh && starts_value {
                        state.expression.clear();
                    }
                    state.fresh = false;
                    state.has_error = false;
                    state.expression.push_str(other);
                    preview(&mut state);
                }
            }
        }
        self.sync();
    }

    /// Recall a history row by position (0 is the newest).
    pub fn recall(&self, index: usize) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(entry) = state.history.recall(index).cloned() {
                state.expression = entry.expression;
                state.result = entry.result;
                state.value = entry.value;
                state.has_error = false;
                state.fresh = true;
            }
        }
        self.sync();
    }

    pub fn clear_history(&self) {
        self.state.borrow_mut().history.clear();
        self.sync();
    }

    fn wire(self: &Rc<Self>) {
        {
            let weak = Rc::downgrade(self);
            self.ui.on_key(move |key| {
                if let Some(controller) = weak.upgrade() {
                    controller.press(key.as_str());
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_recall(move |index| {
                if let Some(controller) = weak.upgrade() {
                    controller.recall(index.max(0) as usize);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_clear_history(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.clear_history();
                }
            });
        }
    }

    fn sync(&self) {
        let state = self.state.borrow();
        let ui = &self.ui;
        ui.set_expression(SharedString::from(state.expression.clone()));
        ui.set_result(SharedString::from(state.result.clone()));
        ui.set_has_error(state.has_error);
        ui.set_memory_label(SharedString::from(state.history.memory_label()));

        let rows: Vec<HistoryRow> = state
            .history
            .entries()
            .iter()
            .map(|entry| HistoryRow {
                expression: entry.expression.clone().into(),
                result: entry.result.clone().into(),
            })
            .collect();
        ui.set_history(ModelRc::new(VecModel::from(rows)));
    }
}

/// Show the running result while the expression still parses.
fn preview(state: &mut State) {
    if state.expression.trim().is_empty() {
        state.result = "0".to_string();
        return;
    }
    if let Ok(value) = evaluate(&state.expression) {
        state.result = format_number(value);
        state.value = value;
    }
}

/// Evaluate on `=` and keep the result in the history.
fn evaluate_now(state: &mut State) {
    let expression = state.expression.trim().to_string();
    match evaluate(&expression) {
        Ok(value) => {
            state.result = format_number(value);
            state.value = value;
            state.has_error = false;
            state.fresh = true;
            if !expression.is_empty() {
                state.history.push(expression, value);
            }
        }
        Err(error) => {
            state.result = error.to_string();
            state.has_error = true;
            state.fresh = false;
        }
    }
}
