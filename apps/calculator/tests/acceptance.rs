//! DA-AC-03: the calculator produces correct results for a 50-case test set.
//!
//! Each case is (expression, expected display). The set covers precedence,
//! associativity, unary signs, parentheses, percentages/modulo, powers,
//! decimals, constants, functions and the documented error cases.

use aether_calculator::{evaluate, format_number, EvalError};

const CASES: &[(&str, &str)] = &[
    ("1 + 1", "2"),
    ("2 - 5", "-3"),
    ("6 * 7", "42"),
    ("9 / 3", "3"),
    ("10 % 4", "2"),
    ("2 ^ 10", "1024"),
    ("2 ^ 3 ^ 2", "512"),
    ("-5 + 8", "3"),
    ("+7", "7"),
    ("-(3 + 4)", "-7"),
    ("(2 + 3) * 4", "20"),
    ("2 + 3 * 4", "14"),
    ("(10 - 4) / 2", "3"),
    ("1 / 4", "0.25"),
    ("0.1 + 0.2", "0.3"),
    ("1.5 * 2", "3"),
    ("3.75 - 0.25", "3.5"),
    ("100 / 8", "12.5"),
    ("7 % 2", "1"),
    ("2 ^ -2", "0.25"),
    ("4 ^ 0.5", "2"),
    ("sqrt(81)", "9"),
    ("sqrt(2) * sqrt(2)", "2"),
    ("abs(-12.5)", "12.5"),
    ("abs(3)", "3"),
    ("round(2.6)", "3"),
    ("round(-2.6)", "-3"),
    ("pi", "3.1415926536"),
    ("e", "2.7182818285"),
    ("2 * pi", "6.2831853072"),
    ("(1 + 2) ^ 3", "27"),
    ("((2))", "2"),
    ("1 - -1", "2"),
    ("8 / 2 / 2", "2"),
    ("8 / (2 / 2)", "8"),
    ("2 * 3 + 4 * 5", "26"),
    ("100 - 25 % 7", "96"),
    ("1 + 2 * 3 - 4 / 2", "5"),
    ("0 * 1000", "0"),
    ("-0.5 + 1", "0.5"),
    ("999999 * 1000000", "999999000000"),
    ("1 / 8", "0.125"),
    ("5 ^ 3 - 5 * 5", "100"),
    ("(3 + 4) * (2 - 1)", "7"),
    ("sqrt(16) + abs(-4)", "8"),
    ("10 / 4", "2.5"),
    ("2 ^ 16", "65536"),
    ("1.1 * 1.1", "1.21"),
    ("7 / 7", "1"),
    ("3 - 2 * 4 ^ 2", "-29"),
];

#[test]
fn fifty_cases_are_correct() {
    assert_eq!(CASES.len(), 50, "DA-AC-03 requires a 50-case set");
    let mut failures = Vec::new();
    for (expression, expected) in CASES {
        match evaluate(expression) {
            Ok(value) => {
                let rendered = format_number(value);
                if rendered != *expected {
                    failures.push(format!("{expression}: got {rendered}, expected {expected}"));
                }
            }
            Err(error) => failures.push(format!("{expression}: errored {error:?}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} case(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn documented_error_cases_stay_errors() {
    assert!(matches!(evaluate("1 / 0"), Err(EvalError::DivisionByZero)));
    assert!(evaluate("2 +").is_err());
    assert!(evaluate("(1 + 2").is_err());
    assert!(evaluate("sqrt(-4)").is_err());
    assert!(evaluate("hello").is_err());
    assert!(evaluate("").is_err());
}
