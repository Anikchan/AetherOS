//! Calculator history and memory (P4-T11, DA-AC-03).
//!
//! F8 DA-02 keeps the last 20 results. Entries are newest first so the window
//! can list them without reversing, and the memory register supports the four
//! usual operations (M+, M-, MR, MC).

use crate::eval::format_number;

/// F8 DA-02: "history of last 20 results".
pub const HISTORY_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub expression: String,
    pub result: String,
    pub value: f64,
}

impl Entry {
    pub fn new(expression: impl Into<String>, value: f64) -> Entry {
        Entry {
            expression: expression.into(),
            result: format_number(value),
            value,
        }
    }
}

#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<Entry>,
    limit: usize,
    memory: f64,
}

impl Default for History {
    fn default() -> Self {
        History::new()
    }
}

impl History {
    pub fn new() -> History {
        History::with_limit(HISTORY_LIMIT)
    }

    /// Custom cap, used by tests and by future per-app preferences.
    pub fn with_limit(limit: usize) -> History {
        History {
            entries: Vec::new(),
            limit: limit.max(1),
            memory: 0.0,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Push a result in front and drop the oldest entry past the cap.
    pub fn push(&mut self, expression: impl Into<String>, value: f64) {
        self.entries.insert(0, Entry::new(expression, value));
        while self.entries.len() > self.limit {
            self.entries.pop();
        }
    }

    /// Newest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn last(&self) -> Option<&Entry> {
        self.entries.first()
    }

    /// Recall by position, 0 being the newest.
    pub fn recall(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    // --- memory register --------------------------------------------------

    pub fn memory(&self) -> f64 {
        self.memory
    }

    /// M+: add `value`, returning the new memory.
    pub fn memory_add(&mut self, value: f64) -> f64 {
        self.memory += value;
        self.memory
    }

    /// M-: subtract `value`, returning the new memory.
    pub fn memory_subtract(&mut self, value: f64) -> f64 {
        self.memory -= value;
        self.memory
    }

    pub fn memory_clear(&mut self) {
        self.memory = 0.0;
    }

    /// `M 12.5` style label for the display.
    pub fn memory_label(&self) -> String {
        format!("M {}", format_number(self.memory))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_newest_first_and_capped() {
        let mut history = History::new();
        assert!(history.is_empty());
        for step in 0..25 {
            history.push(format!("{step} + 1"), (step + 1) as f64);
        }
        assert_eq!(history.len(), HISTORY_LIMIT);
        let newest = history.last().expect("history has entries");
        assert_eq!(newest.expression, "24 + 1");
        assert_eq!(newest.result, "25");
        // The oldest five entries were dropped.
        assert_eq!(history.entries().last().unwrap().expression, "5 + 1");
        assert!(history
            .entries()
            .iter()
            .all(|entry| entry.expression != "4 + 1"));
    }

    #[test]
    fn custom_limit_and_clear() {
        let mut history = History::with_limit(2);
        assert_eq!(history.limit(), 2);
        history.push("1 + 1", 2.0);
        history.push("2 + 2", 4.0);
        history.push("3 + 3", 6.0);
        assert_eq!(history.len(), 2);
        assert_eq!(history.entries()[0].expression, "3 + 3");
        history.clear();
        assert!(history.is_empty());
        assert!(history.recall(0).is_none());
    }

    #[test]
    fn recall_returns_expressions_and_results() {
        let mut history = History::new();
        history.push("6 * 7", 42.0);
        history.push("sqrt(16)", 4.0);
        let newest = history.recall(0).unwrap();
        assert_eq!(newest.expression, "sqrt(16)");
        assert_eq!(newest.result, "4");
        let older = history.recall(1).unwrap();
        assert_eq!(older.value, 42.0);
        assert!(history.recall(2).is_none());
    }

    #[test]
    fn memory_register_adds_subtracts_recalls_and_clears() {
        let mut history = History::new();
        assert_eq!(history.memory(), 0.0);
        assert_eq!(history.memory_label(), "M 0");
        assert_eq!(history.memory_add(12.5), 12.5);
        assert_eq!(history.memory_label(), "M 12.5");
        assert_eq!(history.memory_subtract(2.0), 10.5);
        assert_eq!(history.memory(), 10.5);
        assert_eq!(history.memory_subtract(10.5), 0.0);
        history.memory_add(3.0);
        history.memory_clear();
        assert_eq!(history.memory(), 0.0);
        assert_eq!(history.memory_label(), "M 0");
    }

    #[test]
    fn entries_format_results_without_noise() {
        let entry = Entry::new("0.1 + 0.2", 0.1 + 0.2);
        assert_eq!(entry.result, "0.3");
        assert_eq!(entry.expression, "0.1 + 0.2");
        let fresh = Entry::new("1 / 3", 1.0 / 3.0);
        assert_eq!(fresh.result, "0.3333333333");
    }
}
