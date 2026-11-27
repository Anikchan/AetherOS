//! Calculator expression evaluator (P4-T11, DA-AC-03).
//!
//! A recursive-descent parser over `+ - * / % ^`, unary sign, parentheses,
//! factorials (`n!`), decimals, the constants `pi`/`e` and the functions
//! `sqrt`, `abs`, `round`, `sin`, `cos`, `tan`, `ln`, `log` and `exp`.
//! Trigonometry follows the [`AngleMode`] (degrees in the window, radians for
//! [`evaluate`]). Precedence follows the usual calculator rules:
//!
//! * `^` binds tightest and is right associative (`2^3^2` = 512),
//! * unary minus binds looser than `^` (`-2^2` = -4),
//! * `!` binds to the value it follows (`3! + 1` = 7),
//! * `* / %` before `+ -`.
//!
//! Errors are typed so the window can tell "incomplete input" from a real
//! failure, and [`format_number`] removes floating point noise.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// Nothing to evaluate.
    EmptyExpression,
    /// A character that is not part of the expression language.
    UnexpectedCharacter(char),
    /// A token appeared where a value was expected.
    UnexpectedToken(&'static str),
    /// A closing parenthesis is missing.
    UnbalancedParenthesis,
    /// A name that is neither a constant nor a function.
    UnknownIdentifier(String),
    /// A function name that is not implemented.
    UnknownFunction(String),
    /// A function was called with the wrong number of arguments.
    BadArity {
        name: String,
        expected: usize,
        got: usize,
    },
    /// Division or modulo by zero.
    DivisionByZero,
    /// The function input is outside its domain.
    DomainError(String),
    /// The result is not a finite number.
    Overflow,
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::EmptyExpression => write!(f, "Enter an expression"),
            EvalError::UnexpectedCharacter(ch) => write!(f, "Unexpected character '{ch}'"),
            EvalError::UnexpectedToken(token) => write!(f, "Unexpected {token}"),
            EvalError::UnbalancedParenthesis => write!(f, "Unbalanced parentheses"),
            EvalError::UnknownIdentifier(name) => write!(f, "Unknown name '{name}'"),
            EvalError::UnknownFunction(name) => write!(f, "Unknown function '{name}'"),
            EvalError::BadArity {
                name,
                expected,
                got,
            } => write!(f, "{name}() takes {expected} argument(s), got {got}"),
            EvalError::DivisionByZero => write!(f, "Division by zero"),
            EvalError::DomainError(message) => write!(f, "{message}"),
            EvalError::Overflow => write!(f, "Result is out of range"),
        }
    }
}

impl std::error::Error for EvalError {}

impl EvalError {
    /// True when the expression just stops early, which happens while typing.
    pub fn is_incomplete(&self) -> bool {
        matches!(
            self,
            EvalError::EmptyExpression
                | EvalError::UnbalancedParenthesis
                | EvalError::UnexpectedToken("value")
                | EvalError::UnexpectedToken("number")
                | EvalError::UnexpectedToken("')'")
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Op(char),
    Open,
    Close,
}

/// Angle unit used by `sin`, `cos` and `tan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngleMode {
    /// Radians: what [`evaluate`] and the acceptance tests use.
    #[default]
    Radians,
    /// Degrees: the Calculator window's default, like the DA-02 keypad.
    Degrees,
}

impl AngleMode {
    /// Label shown on the toggle key.
    pub fn label(self) -> &'static str {
        match self {
            AngleMode::Radians => "RAD",
            AngleMode::Degrees => "DEG",
        }
    }

    /// The other unit, for the `DEG`/`RAD` toggle key.
    pub fn toggled(self) -> AngleMode {
        match self {
            AngleMode::Radians => AngleMode::Degrees,
            AngleMode::Degrees => AngleMode::Radians,
        }
    }

    fn to_radians(self, value: f64) -> f64 {
        match self {
            AngleMode::Radians => value,
            AngleMode::Degrees => value.to_radians(),
        }
    }
}

fn tokenize(input: &str) -> Result<Vec<Token>, EvalError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            ' ' | '\t' | '\n' | '\r' => index += 1,
            '0'..='9' | '.' => {
                let start = index;
                let mut seen_dot = false;
                while index < chars.len() {
                    let next = chars[index];
                    if next.is_ascii_digit() {
                        index += 1;
                    } else if next == '.' && !seen_dot {
                        seen_dot = true;
                        index += 1;
                    } else {
                        break;
                    }
                }
                let text: String = chars[start..index].iter().collect();
                if text == "." {
                    return Err(EvalError::UnexpectedCharacter('.'));
                }
                let value = text.parse::<f64>().map_err(|_| EvalError::Overflow)?;
                tokens.push(Token::Number(value));
            }
            'a'..='z' | 'A'..='Z' => {
                let start = index;
                while index < chars.len() && chars[index].is_ascii_alphabetic() {
                    index += 1;
                }
                let name: String = chars[start..index]
                    .iter()
                    .collect::<String>()
                    .to_lowercase();
                tokens.push(Token::Ident(name));
            }
            '+' | '-' | '*' | '/' | '%' | '^' | '!' | '×' | '÷' | '−' => {
                let normalised = match ch {
                    '×' => '*',
                    '÷' => '/',
                    '−' => '-',
                    other => other,
                };
                tokens.push(Token::Op(normalised));
                index += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                index += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                index += 1;
            }
            other => return Err(EvalError::UnexpectedCharacter(other)),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    angle: AngleMode,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn expect_close(&mut self) -> Result<(), EvalError> {
        match self.next() {
            Some(Token::Close) => Ok(()),
            Some(_) => Err(EvalError::UnexpectedToken("')'")),
            None => Err(EvalError::UnbalancedParenthesis),
        }
    }

    fn expression(&mut self) -> Result<f64, EvalError> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some(Token::Op('+')) => {
                    self.position += 1;
                    value += self.term()?;
                }
                Some(Token::Op('-')) => {
                    self.position += 1;
                    value -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<f64, EvalError> {
        let mut value = self.unary()?;
        loop {
            match self.peek() {
                Some(Token::Op('*')) => {
                    self.position += 1;
                    value *= self.unary()?;
                }
                Some(Token::Op('/')) => {
                    self.position += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return Err(EvalError::DivisionByZero);
                    }
                    value /= divisor;
                }
                Some(Token::Op('%')) => {
                    self.position += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return Err(EvalError::DivisionByZero);
                    }
                    value %= divisor;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<f64, EvalError> {
        match self.peek() {
            Some(Token::Op('-')) => {
                self.position += 1;
                Ok(-self.unary()?)
            }
            Some(Token::Op('+')) => {
                self.position += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64, EvalError> {
        let base = self.primary()?;
        if let Some(Token::Op('^')) = self.peek() {
            self.position += 1;
            // Right associative and the exponent may carry its own sign.
            let exponent = self.unary()?;
            Ok(base.powf(exponent))
        } else {
            Ok(base)
        }
    }

    fn primary(&mut self) -> Result<f64, EvalError> {
        let value = match self.next() {
            Some(Token::Number(value)) => Ok(value),
            Some(Token::Open) => {
                let value = self.expression()?;
                self.expect_close()?;
                Ok(value)
            }
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::Open)) {
                    self.position += 1;
                    if matches!(self.peek(), Some(Token::Close)) {
                        self.position += 1;
                        return Err(EvalError::BadArity {
                            name,
                            expected: 1,
                            got: 0,
                        });
                    }
                    let argument = self.expression()?;
                    self.expect_close()?;
                    apply_function(&name, argument, self.angle)
                } else {
                    match name.as_str() {
                        "pi" => Ok(std::f64::consts::PI),
                        "e" => Ok(std::f64::consts::E),
                        _ => Err(EvalError::UnknownIdentifier(name)),
                    }
                }
            }
            Some(Token::Close) => Err(EvalError::UnbalancedParenthesis),
            Some(_) => Err(EvalError::UnexpectedToken("expression")),
            None => Err(EvalError::EmptyExpression),
        }?;
        // Postfix factorial: `3!` = 6, `(2+2)!` = 24.
        let mut value = value;
        while let Some(Token::Op('!')) = self.peek() {
            self.position += 1;
            value = factorial(value)?;
        }
        Ok(value)
    }
}

fn apply_function(name: &str, value: f64, angle: AngleMode) -> Result<f64, EvalError> {
    match name {
        "sqrt" => {
            if value < 0.0 {
                return Err(EvalError::DomainError(
                    "sqrt() needs a non-negative number".to_string(),
                ));
            }
            Ok(value.sqrt())
        }
        "abs" => Ok(value.abs()),
        "round" => Ok(value.round()),
        "sin" => Ok(snap_trig(angle.to_radians(value).sin())),
        "cos" => Ok(snap_trig(angle.to_radians(value).cos())),
        "tan" => {
            let tangent = angle.to_radians(value).tan();
            if tangent.is_finite() {
                Ok(tangent)
            } else {
                Err(EvalError::DomainError(
                    "tan() is undefined at this angle".to_string(),
                ))
            }
        }
        "ln" | "log" => {
            if value <= 0.0 {
                return Err(EvalError::DomainError(format!(
                    "{name}() needs a positive number"
                )));
            }
            Ok(if name == "ln" {
                value.ln()
            } else {
                value.log10()
            })
        }
        "exp" => Ok(value.exp()),
        _ => Err(EvalError::UnknownFunction(name.to_string())),
    }
}

/// Trigonometry on a calculator snaps tan(90°) noise to the exact value users
/// expect (cos 90° is 6.1e-17 in f64, which would print as `6.12e-17`).
fn snap_trig(value: f64) -> f64 {
    if value.abs() < 1e-12 {
        0.0
    } else if (value.abs() - 1.0).abs() < 1e-12 {
        value.signum()
    } else {
        value
    }
}

/// `n!` for whole numbers the f64 result can hold exactly enough to compare.
fn factorial(value: f64) -> Result<f64, EvalError> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return Err(EvalError::DomainError(
            "factorial needs a whole number from 0 to 170".to_string(),
        ));
    }
    if value > 170.0 {
        return Err(EvalError::Overflow);
    }
    let mut result = 1.0;
    let mut factor = 2.0;
    while factor <= value {
        result *= factor;
        factor += 1.0;
    }
    Ok(result)
}

/// Evaluate a complete expression in radians.
pub fn evaluate(input: &str) -> Result<f64, EvalError> {
    evaluate_mode(input, AngleMode::Radians)
}

/// Evaluate with an explicit angle unit (the window passes [`AngleMode::Degrees`]).
pub fn evaluate_mode(input: &str, angle: AngleMode) -> Result<f64, EvalError> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err(EvalError::EmptyExpression);
    }
    let mut parser = Parser {
        tokens,
        position: 0,
        angle,
    };
    let value = parser.expression()?;
    if parser.position < parser.tokens.len() {
        return Err(match &parser.tokens[parser.position] {
            Token::Close => EvalError::UnbalancedParenthesis,
            _ => EvalError::UnexpectedToken("token"),
        });
    }
    if !value.is_finite() {
        return Err(EvalError::Overflow);
    }
    Ok(value)
}

/// Evaluate and format in one step, the way the window shows a result.
pub fn evaluate_display(input: &str) -> Result<String, EvalError> {
    evaluate(input).map(format_number)
}

/// Like [`evaluate_display`], with an explicit angle unit.
pub fn evaluate_display_mode(input: &str, angle: AngleMode) -> Result<String, EvalError> {
    evaluate_mode(input, angle).map(format_number)
}

/// Format without floating point noise: at most 10 decimals, no trailing
/// zeros, `-0` shown as `0`, and scientific notation at the extremes.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "Error".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if !(1e-9..1e15).contains(&magnitude) {
        return format!("{value:e}");
    }
    let rounded = (value * 1e10).round() / 1e10;
    let text = format!("{rounded:.10}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" || trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(input: &str) -> String {
        evaluate_display(input).unwrap()
    }

    #[test]
    fn respects_precedence_and_parentheses() {
        assert_eq!(format("2 + 3 * 4"), "14");
        assert_eq!(format("(2 + 3) * 4"), "20");
        assert_eq!(format("10 - 4 / 2"), "8");
        assert_eq!(format("10 % 4 + 1"), "3");
        assert_eq!(format("2 + 3 * 4 ^ 2"), "50");
        assert_eq!(format("-2^2"), "-4", "unary minus is looser than ^");
        assert_eq!(format("(-2)^2"), "4");
        assert_eq!(format("2^-2"), "0.25");
        assert_eq!(format("2^3^2"), "512", "^ is right associative");
        assert_eq!(format("3 * (4 - (1 + 1))"), "6");
    }

    #[test]
    fn parses_decimals_constants_and_functions() {
        assert_eq!(format(".5 + .25"), "0.75");
        assert_eq!(format("2.5 * 2"), "5");
        assert_eq!(format("pi"), "3.1415926536");
        assert_eq!(format("e"), "2.7182818285");
        assert_eq!(format("sqrt(16)"), "4");
        assert_eq!(format("sqrt(2)^2"), "2");
        assert_eq!(format("abs(-7)"), "7");
        assert_eq!(format("round(2.6) + round(-2.6)"), "0");
        assert_eq!(format("sqrt(abs(-9))"), "3");
        // Typographic operators are normalised.
        assert_eq!(format("6 × 7"), "42");
        assert_eq!(format("9 ÷ 3"), "3");
        assert_eq!(format("5 − 8"), "-3");
    }

    #[test]
    fn reports_division_by_zero_and_domain_errors() {
        assert_eq!(evaluate("1 / 0"), Err(EvalError::DivisionByZero));
        assert_eq!(evaluate("5 % 0"), Err(EvalError::DivisionByZero));
        assert_eq!(
            evaluate("sqrt(-1)"),
            Err(EvalError::DomainError(
                "sqrt() needs a non-negative number".to_string()
            ))
        );
        // A power that leaves the finite range is reported as overflow.
        assert_eq!(evaluate("10 ^ 400"), Err(EvalError::Overflow));
    }

    #[test]
    fn rejects_invalid_input_with_specific_errors() {
        assert_eq!(evaluate(""), Err(EvalError::EmptyExpression));
        assert_eq!(evaluate("   "), Err(EvalError::EmptyExpression));
        assert_eq!(evaluate("1 + @"), Err(EvalError::UnexpectedCharacter('@')));
        assert_eq!(evaluate("(1 + 2"), Err(EvalError::UnbalancedParenthesis));
        assert_eq!(evaluate("1 + 2)"), Err(EvalError::UnbalancedParenthesis));
        assert_eq!(
            evaluate("foo"),
            Err(EvalError::UnknownIdentifier("foo".to_string()))
        );
        assert_eq!(
            evaluate("cbrt(1)"),
            Err(EvalError::UnknownFunction("cbrt".to_string()))
        );
        assert_eq!(
            evaluate("1 + * 2"),
            Err(EvalError::UnexpectedToken("expression"))
        );
        assert_eq!(
            evaluate("sqrt()"),
            Err(EvalError::BadArity {
                name: "sqrt".to_string(),
                expected: 1,
                got: 0,
            })
        );
        assert_eq!(evaluate("2 3"), Err(EvalError::UnexpectedToken("token")));
        assert!(evaluate("1 +").unwrap_err().is_incomplete());
        assert!(!evaluate("1 / 0").unwrap_err().is_incomplete());
    }

    #[test]
    fn scientific_functions_and_factorials() {
        // Radians (the library default).
        assert_eq!(format("sin(pi / 2)"), "1");
        assert_eq!(format("cos(pi)"), "-1");
        assert_eq!(format("ln(e)"), "1");
        assert_eq!(format("log(1000)"), "3");
        assert_eq!(format("exp(0)"), "1");
        assert_eq!(format("5!"), "120");
        assert_eq!(format("0!"), "1");
        assert_eq!(format("3! + 1"), "7");
        assert_eq!(format("(2 + 2)!"), "24");
        assert_eq!(
            format("2^3!"),
            "64",
            "factorial binds inside the power base"
        );
        // Degrees (to_radians): sin(30°) is exactly 0.5 within float noise.
        assert_eq!(
            evaluate_display_mode("sin(30)", AngleMode::Degrees),
            Ok("0.5".to_string())
        );
        assert_eq!(
            evaluate_display_mode("cos(90)", AngleMode::Degrees),
            Ok("0".to_string())
        );
        assert_eq!(
            evaluate_display_mode("tan(45)", AngleMode::Degrees),
            Ok("1".to_string())
        );
        assert_eq!(
            evaluate_display_mode("sin(90)", AngleMode::Degrees),
            Ok("1".to_string())
        );
        assert_eq!(AngleMode::Degrees.toggled(), AngleMode::Radians);
        assert_eq!(AngleMode::Degrees.label(), "DEG");
        assert_eq!(AngleMode::default(), AngleMode::Radians);

        // Domain errors are specific, not silent NaNs.
        assert_eq!(
            evaluate("ln(0)"),
            Err(EvalError::DomainError(
                "ln() needs a positive number".to_string()
            ))
        );
        assert_eq!(
            evaluate("log(-1)"),
            Err(EvalError::DomainError(
                "log() needs a positive number".to_string()
            ))
        );
        assert_eq!(
            evaluate("2.5!"),
            Err(EvalError::DomainError(
                "factorial needs a whole number from 0 to 170".to_string()
            ))
        );
        assert_eq!(evaluate("200!"), Err(EvalError::Overflow));
    }

    #[test]
    fn formatting_removes_float_noise() {
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1.0 / 3.0), "0.3333333333");
        assert_eq!(format_number(2.0 / 3.0), "0.6666666667");
        assert_eq!(format_number(4.0), "4");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(2.50), "2.5");
        assert_eq!(format_number(1e20), "1e20");
        assert_eq!(format_number(1e-12), "1e-12");
        assert_eq!(format_number(f64::INFINITY), "Error");
        assert_eq!(format_number(f64::NAN), "Error");
        assert_eq!(format("0.1 + 0.2"), "0.3");
        assert_eq!(format("1 / 3"), "0.3333333333");
    }

    #[test]
    fn evaluates_a_table_of_expressions() {
        // DA-AC-03: a small table of independent cases, including precedence,
        // signs, powers and modulo.
        let cases: [(&str, f64); 22] = [
            ("1 + 1", 2.0),
            ("8 - 3", 5.0),
            ("6 * 7", 42.0),
            ("9 / 3", 3.0),
            ("9 / 4", 2.25),
            ("7 % 3", 1.0),
            ("2 ^ 10", 1024.0),
            ("-5 + 5", 0.0),
            ("-(5 + 5)", -10.0),
            ("3 + 4 * 2 / (1 - 5) ^ 2", 3.5),
            ("1 + 2 * 3 - 4 / 2", 5.0),
            ("((1 + 2) * (3 + 4))", 21.0),
            ("100 / 8", 12.5),
            ("0.1 + 0.2", 0.30000000000000004),
            ("2 * 3.5", 7.0),
            ("sqrt(81)", 9.0),
            ("abs(-3.5)", 3.5),
            ("round(1.5)", 2.0),
            ("pi * 2", std::f64::consts::TAU),
            ("e ^ 1", std::f64::consts::E),
            ("5 % 2 * 3", 3.0),
            ("2 ^ 0", 1.0),
        ];
        for (expression, expected) in cases {
            let value = evaluate(expression).unwrap_or_else(|error| {
                panic!("{expression} failed: {error}");
            });
            assert!(
                (value - expected).abs() < 1e-9,
                "{expression} -> {value}, expected {expected}"
            );
        }
    }
}
