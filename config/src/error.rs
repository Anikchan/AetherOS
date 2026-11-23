//! Configuration errors with file and key context (F4 CUS-02).

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    UnknownKey(String),
    InvalidValue {
        key: String,
        expected: String,
        actual: String,
    },
    Schema(String),
    Hook(HookError),
    Features(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Parse {
                path,
                message,
                line,
                column,
            } => {
                write!(f, "{}", path.display())?;
                if let Some(line) = line {
                    write!(f, ":{line}")?;
                    if let Some(column) = column {
                        write!(f, ":{column}")?;
                    }
                }
                write!(f, ": {message}")
            }
            Self::UnknownKey(key) => write!(
                f,
                "unknown key {key}; run `aetherctl config check` for the schema"
            ),
            Self::InvalidValue {
                key,
                expected,
                actual,
            } => write!(f, "{key} must be {expected}, got {actual}"),
            Self::Schema(message) => write!(f, "schema error: {message}"),
            Self::Hook(error) => write!(f, "hook error: {error}"),
            Self::Features(message) => write!(f, "features: {message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookError {
    Unavailable(String),
    Failed { code: Option<i32>, stderr: String },
    Timeout { millis: u64 },
    Io(String),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "hook runner unavailable: {message}"),
            Self::Failed { code, stderr } => {
                write!(f, "hook failed (exit {code:?}): {stderr}")
            }
            Self::Timeout { millis } => write!(f, "hook exceeded {millis} ms and was stopped"),
            Self::Io(message) => write!(f, "hook i/o: {message}"),
        }
    }
}
