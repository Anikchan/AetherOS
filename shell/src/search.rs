//! Launcher search (P4, S3 — F1 SHL-03).
//!
//! Three sources in one ranked list: apps, settings keys and inline
//! calculations. Ranking is pure and allocation-light so the 200 ms launcher
//! budget (F1 SHL-AC-03) is spent on rendering, not searching.

use crate::IconKind;

use crate::config::SettingDef;
use crate::model::{App, SearchHit, SearchKind};

const MAX_RESULTS: usize = 12;

/// Rank `query` against apps and settings and prepend a calculation result when
/// the query looks like arithmetic.
pub fn search(query: &str, apps: &[App], settings: &[SettingDef]) -> Vec<SearchHit> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return apps
            .iter()
            .take(8)
            .map(|app| SearchHit {
                id: app.id.clone(),
                title: app.name.clone(),
                subtitle: if app.running {
                    "Running".to_string()
                } else if app.pinned {
                    "Pinned".to_string()
                } else {
                    "App".to_string()
                },
                icon: app.icon,
                kind: SearchKind::App,
                score: 0,
            })
            .collect();
    }

    let mut hits: Vec<SearchHit> = Vec::new();

    if looks_like_calculation(trimmed) {
        if let Some(value) = eval_expression(trimmed) {
            hits.push(SearchHit {
                id: "calc:result".to_string(),
                title: format!("= {}", format_number(value)),
                subtitle: trimmed.to_string(),
                icon: IconKind::Calculator,
                kind: SearchKind::Calculation,
                score: 0,
            });
        }
    }

    for app in apps {
        if let Some(score) = score_match(trimmed, &app.name) {
            hits.push(SearchHit {
                id: app.id.clone(),
                title: app.name.clone(),
                subtitle: if app.running {
                    "Running".to_string()
                } else {
                    app.exec.clone()
                },
                icon: app.icon,
                kind: SearchKind::App,
                score,
            });
        }
    }

    for setting in settings {
        if let Some(score) = score_match(trimmed, setting.label) {
            hits.push(SearchHit {
                id: format!("setting:{}", setting.key),
                title: setting.label.to_string(),
                subtitle: format!("{}  ·  {}", setting.file, setting.key),
                icon: setting.category_icon(),
                kind: SearchKind::Settings,
                score: score + 1,
            });
        }
    }

    hits.sort_by(|a, b| a.score.cmp(&b.score).then_with(|| a.title.cmp(&b.title)));
    hits.truncate(MAX_RESULTS);
    hits
}

/// Score a match: lower is better. `None` means no match.
pub fn score_match(query: &str, candidate: &str) -> Option<u32> {
    let query = query.to_lowercase();
    let candidate = candidate.to_lowercase();

    if query.is_empty() {
        return Some(10);
    }
    if candidate == query {
        return Some(0);
    }
    if candidate.starts_with(&query) {
        return Some(1);
    }
    if candidate
        .split([' ', '-', '_', '/', '.'])
        .any(|word| word.starts_with(&query))
    {
        return Some(2);
    }
    if candidate.contains(&query) {
        return Some(3);
    }
    if is_subsequence(&query, &candidate) {
        return Some(5);
    }
    None
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut haystack = haystack.chars();
    needle
        .chars()
        .all(|needle_char| haystack.any(|candidate| candidate == needle_char))
}

// ---------------------------------------------------------------- calculator ---

/// A search string is treated as arithmetic only when it contains an operator,
/// so typing "notes" never produces a calculation card.
fn looks_like_calculation(input: &str) -> bool {
    input.chars().any(|c| matches!(c, '+' | '-' | '*' | '/' | '%' | '^'))
        && input.chars().any(|c| c.is_ascii_digit())
}

/// Format a result without binary floating point noise: 132, 3.5, 0.1.
pub fn format_number(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        let text = format!("{value:.6}");
        let trimmed = text.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }
}

/// Recursive-descent evaluator for `+ - * / % ^`, parentheses, unary signs and
/// the constants `pi` and `e`.
pub fn eval_expression(input: &str) -> Option<f64> {
    // `1 2` is a typo, not twenty-twelve: reject digits split by whitespace
    // instead of silently concatenating them.
    let chars: Vec<char> = input.chars().collect();
    for window in chars.windows(3) {
        if window[0].is_ascii_digit() && window[1].is_whitespace() && window[2].is_ascii_digit() {
            return None;
        }
    }
    let mut parser = Parser {
        chars: input.chars().filter(|c| !c.is_whitespace()).collect(),
        position: 0,
    };
    let value = parser.expression()?;
    if parser.position != parser.chars.len() || !value.is_finite() {
        return None;
    }
    Some(value)
}

struct Parser {
    chars: Vec<char>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.position += 1;
        Some(c)
    }

    fn expression(&mut self) -> Option<f64> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some('+') => {
                    self.bump();
                    value += self.term()?;
                }
                Some('-') => {
                    self.bump();
                    value -= self.term()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut value = self.unary()?;
        loop {
            match self.peek() {
                Some('*') => {
                    self.bump();
                    value *= self.unary()?;
                }
                Some('/') => {
                    self.bump();
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value /= divisor;
                }
                Some('%') => {
                    self.bump();
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value %= divisor;
                }
                _ => return Some(value),
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some('-') => {
                self.bump();
                Some(-self.unary()?)
            }
            Some('+') => {
                self.bump();
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.primary()?;
        if self.peek() == Some('^') {
            self.bump();
            let exponent = self.power()?;
            return Some(base.powf(exponent));
        }
        Some(base)
    }

    fn primary(&mut self) -> Option<f64> {
        match self.peek()? {
            '(' => {
                self.bump();
                let value = self.expression()?;
                if self.bump() != Some(')') {
                    return None;
                }
                Some(value)
            }
            'p' if self.chars[self.position..].starts_with(&['p', 'i']) => {
                self.position += 2;
                Some(std::f64::consts::PI)
            }
            'e' => {
                self.position += 1;
                Some(std::f64::consts::E)
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = self.position;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_digit() || c == '.')
                {
                    self.bump();
                }
                let literal: String = self.chars[start..self.position].iter().collect();
                literal.parse::<f64>().ok()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, running: bool) -> App {
        App {
            id: id.to_string(),
            name: name.to_string(),
            icon: IconKind::Folder,
            exec: format!("/usr/bin/{id}"),
            running,
            pinned: false,
            badge: 0,
        }
    }

    #[test]
    fn empty_query_lists_apps() {
        let apps = vec![app("a", "AetherPad", false), app("f", "Files", true)];
        let hits = search("", &apps, &[]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[1].subtitle, "Running");
    }

    #[test]
    fn prefix_beats_substring() {
        assert!(score_match("file", "Files").unwrap() < score_match("file", "MyFiles").unwrap());
        assert_eq!(score_match("FILES", "files"), Some(0));
        assert_eq!(score_match("xyz", "Files"), None);
    }

    #[test]
    fn subsequence_matches_late() {
        let score = score_match("apd", "AetherPad").unwrap();
        assert_eq!(score, 5);
    }

    #[test]
    fn calculation_is_prepended_once() {
        let hits = search("12+30*4", &[], &[]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "= 132");
        assert_eq!(hits[0].kind, SearchKind::Calculation);
    }

    #[test]
    fn app_name_with_digits_is_not_a_calculation() {
        assert!(search("AetherPad", &[app("t", "AetherPad", false)], &[]).len() == 1);
        assert!(!looks_like_calculation("12"));
        assert!(!looks_like_calculation("Route 66"));
    }

    #[test]
    fn evaluator_handles_precedence_and_parens() {
        assert_eq!(eval_expression("12+30*4"), Some(132.0));
        assert_eq!(eval_expression("(2+3)^2"), Some(25.0));
        assert_eq!(eval_expression("-5 + 8"), Some(3.0));
        assert_eq!(eval_expression("5 % 3"), Some(2.0));
        assert_eq!(eval_expression("2^3^2"), Some(512.0));
    }

    #[test]
    fn evaluator_rejects_invalid_input() {
        assert_eq!(eval_expression("1/0"), None);
        assert_eq!(eval_expression("(1+2"), None);
        assert_eq!(eval_expression("abc"), None);
        assert_eq!(eval_expression(""), None);
        assert_eq!(eval_expression("1 2"), None);
    }

    #[test]
    fn number_formatting_is_clean() {
        assert_eq!(format_number(132.0), "132");
        assert_eq!(format_number(3.5), "3.5");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(-2.25), "-2.25");
    }

    #[test]
    fn results_are_capped() {
        let apps: Vec<App> = (0..30)
            .map(|i| app(&format!("app{i}"), &format!("App {i}"), false))
            .collect();
        assert_eq!(search("app", &apps, &[]).len(), MAX_RESULTS);
    }
}
