//! Login state machine for the AetherOS greeter (P4-T06, SHL-AC-08).
//!
//! The greeter is a normal user process: greetd owns PAM, and the greeter
//! answers `auth_message` prompts over IPC. [`GreetdSession`] turns that
//! conversation into a small, deterministic event stream
//! ([`LoginEvent`]) the UI can render — prompt, authenticated, wrong password
//! (with attempts left), locked out, unavailable, or backend error.
//!
//! PAM may ask any number of questions, including informative ones. `pump`
//! answers `info`/`error` messages with no response and returns the first real
//! prompt, so the UI never has to know the sequence.
//!
//! [`GreeterConfig`] is the greeter's own file (`/etc/aether/greetd/greeter.toml`):
//! the session command it asks greetd to start after a successful login.

use std::io::{Read, Write};

use serde::Deserialize;

use crate::greetd::{
    AuthMessageType, ErrorType, GreetdConnection, GreetdError, Request, Response,
};

/// Failed attempts before the greeter cancels the session (greetd/PAM enforce
/// their own limits as well).
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Bound on info/error messages answered inside one step, so a misbehaving
/// backend cannot spin forever.
const MAX_MESSAGES_PER_STEP: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Answer may be shown (normally the user name).
    Visible,
    /// Answer is a secret (password).
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginEvent {
    /// The UI must ask the user and call `respond`.
    Prompt(Prompt),
    /// Credentials accepted; `begin_session` may now start the session.
    Authenticated,
    /// Wrong credentials; another attempt is allowed.
    Denied { message: String, attempts_left: u32 },
    /// Attempt budget exhausted; the session was cancelled.
    LockedOut { message: String },
    /// greetd is not running or its socket is gone.
    Unavailable(String),
    /// greetd answered with a protocol/backend error.
    Error(String),
}

impl LoginEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            LoginEvent::Authenticated | LoginEvent::LockedOut { .. } | LoginEvent::Unavailable(_)
        )
    }

    /// One-line text for the login screen.
    pub fn message(&self) -> String {
        match self {
            LoginEvent::Prompt(prompt) => prompt.message.clone(),
            LoginEvent::Authenticated => String::new(),
            LoginEvent::Denied { message, attempts_left } => {
                format!("{message} ({attempts_left} attempts left)")
            }
            LoginEvent::LockedOut { message } => message.clone(),
            LoginEvent::Unavailable(message) | LoginEvent::Error(message) => message.clone(),
        }
    }
}

/// What the greeter surface drives. Tests substitute a scripted session.
pub trait LoginSession {
    fn start(&mut self, username: &str) -> LoginEvent;
    fn respond(&mut self, answer: &str) -> LoginEvent;
    fn begin_session(&mut self, cmd: &[String], env: &[String]) -> Result<(), GreetdError>;
    fn cancel(&mut self);
}

/// greetd-backed login session.
pub struct GreetdSession<S> {
    connection: GreetdConnection<S>,
    attempts: u32,
    max_attempts: u32,
}

impl<S: Read + Write> GreetdSession<S> {
    pub fn new(connection: GreetdConnection<S>) -> Self {
        GreetdSession::with_max_attempts(connection, DEFAULT_MAX_ATTEMPTS)
    }

    pub fn with_max_attempts(connection: GreetdConnection<S>, max_attempts: u32) -> Self {
        GreetdSession {
            connection,
            attempts: 0,
            max_attempts: max_attempts.max(1),
        }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Send one request and turn the reply stream into a [`LoginEvent`].
    fn step(&mut self, request: &Request) -> LoginEvent {
        if let Err(error) = self.connection.send(request) {
            return unavailable(error);
        }
        self.pump()
    }

    fn pump(&mut self) -> LoginEvent {
        for _ in 0..MAX_MESSAGES_PER_STEP {
            let response = match self.connection.receive() {
                Ok(response) => response,
                Err(error) => return unavailable(error),
            };
            match response {
                Response::Success => return LoginEvent::Authenticated,
                Response::AuthMessage {
                    auth_message_type: AuthMessageType::Secret,
                    auth_message,
                } => {
                    return LoginEvent::Prompt(Prompt {
                        kind: PromptKind::Secret,
                        message: auth_message,
                    })
                }
                Response::AuthMessage {
                    auth_message_type: AuthMessageType::Visible,
                    auth_message,
                } => {
                    return LoginEvent::Prompt(Prompt {
                        kind: PromptKind::Visible,
                        message: auth_message,
                    })
                }
                Response::AuthMessage {
                    auth_message_type: AuthMessageType::Info | AuthMessageType::Error,
                    ..
                } => {
                    // Informative message: show it (the UI keeps the last
                    // prompt text) and continue without an answer.
                    if let Err(error) = self.connection.send(&Request::PostAuthMessageResponse {
                        response: None,
                    }) {
                        return unavailable(error);
                    }
                }
                Response::Error {
                    error_type: ErrorType::AuthError,
                    description,
                } => {
                    self.attempts += 1;
                    let message = if description.trim().is_empty() {
                        "Incorrect user or password".to_string()
                    } else {
                        description
                    };
                    if self.attempts >= self.max_attempts {
                        let _ = self.connection.send(&Request::CancelSession);
                        return LoginEvent::LockedOut {
                            message: format!("{message} — too many attempts"),
                        };
                    }
                    return LoginEvent::Denied {
                        message,
                        attempts_left: self.max_attempts - self.attempts,
                    };
                }
                Response::Error {
                    error_type: ErrorType::Error,
                    description,
                } => return LoginEvent::Error(description),
            }
        }
        LoginEvent::Error("greetd sent too many messages without a prompt".into())
    }
}

impl<S: Read + Write> LoginSession for GreetdSession<S> {
    fn start(&mut self, username: &str) -> LoginEvent {
        self.attempts = 0;
        self.step(&Request::CreateSession {
            username: username.to_string(),
        })
    }

    fn respond(&mut self, answer: &str) -> LoginEvent {
        self.step(&Request::PostAuthMessageResponse {
            response: Some(answer.to_string()),
        })
    }

    fn begin_session(&mut self, cmd: &[String], env: &[String]) -> Result<(), GreetdError> {
        let response = self.connection.request(&Request::StartSession {
            cmd: cmd.to_vec(),
            env: env.to_vec(),
        })?;
        match response {
            Response::Success => Ok(()),
            Response::Error { description, .. } => Err(GreetdError::Backend {
                error_type: ErrorType::Error,
                description,
            }),
            Response::AuthMessage { .. } => Err(GreetdError::Protocol(
                "greetd asked a question after authentication".into(),
            )),
        }
    }

    fn cancel(&mut self) {
        let _ = self.connection.send(&Request::CancelSession);
    }
}

fn unavailable(error: GreetdError) -> LoginEvent {
    match error {
        GreetdError::NotAvailable(_) => LoginEvent::Unavailable(error.to_string()),
        other => LoginEvent::Error(other.to_string()),
    }
}

// ---------------------------------------------------------------- config ---

/// `/etc/aether/greetd/greeter.toml` (P4-T06).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GreeterConfig {
    /// User name pre-filled in the login field (not a security boundary).
    #[serde(default = "default_user")]
    pub user: String,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Command greetd starts after a successful login.
    #[serde(default = "default_session")]
    pub session: Vec<String>,
    /// Extra environment for the session (greetd adds PAM's environment too).
    #[serde(default)]
    pub env: Vec<String>,
}

fn default_user() -> String {
    "aether".to_string()
}

fn default_max_attempts() -> u32 {
    DEFAULT_MAX_ATTEMPTS
}

fn default_session() -> Vec<String> {
    vec!["aether-shell".to_string()]
}

impl Default for GreeterConfig {
    fn default() -> Self {
        GreeterConfig {
            user: default_user(),
            max_attempts: default_max_attempts(),
            session: default_session(),
            env: Vec::new(),
        }
    }
}

impl GreeterConfig {
    pub fn parse(text: &str) -> Result<GreeterConfig, String> {
        let mut config: GreeterConfig =
            toml::from_str(text).map_err(|error| format!("greeter.toml: {error}"))?;
        if config.session.is_empty() {
            config.session = default_session();
        }
        config.max_attempts = config.max_attempts.max(1);
        Ok(config)
    }

    pub fn load(path: &std::path::Path) -> Result<GreeterConfig, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        GreeterConfig::parse(&text)
    }

    /// The session command line as one string, for logs and the UI.
    pub fn session_label(&self) -> String {
        self.session.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::greetd::write_frame;
    use std::collections::VecDeque;
    use std::io::Cursor;

    struct Scripted {
        responses: VecDeque<Vec<u8>>,
        written: Vec<u8>,
    }

    impl Scripted {
        fn new(responses: &[&str]) -> Scripted {
            Scripted {
                responses: responses
                    .iter()
                    .map(|json| {
                        let mut frame = Vec::new();
                        write_frame(&mut frame, json.as_bytes()).unwrap();
                        frame
                    })
                    .collect(),
                written: Vec::new(),
            }
        }

        fn sent(&self) -> Vec<Request> {
            let mut reader = Cursor::new(self.written.clone());
            let mut requests = Vec::new();
            while let Ok(payload) = crate::greetd::read_frame(&mut reader) {
                requests.push(serde_json::from_slice(&payload).unwrap());
            }
            requests
        }
    }

    impl Read for Scripted {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let Some(frame) = self.responses.front_mut() else {
                return Ok(0);
            };
            let taken = frame.len().min(buffer.len());
            buffer[..taken].copy_from_slice(&frame[..taken]);
            frame.drain(..taken);
            if frame.is_empty() {
                self.responses.pop_front();
            }
            Ok(taken)
        }
    }

    impl Write for Scripted {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn session(responses: &[&str]) -> GreetdSession<Scripted> {
        GreetdSession::new(GreetdConnection::from_stream(Scripted::new(responses)))
    }

    #[test]
    fn password_prompt_then_success_authenticates() {
        let mut session = session(&[
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
            r#"{"type":"success"}"#,
        ]);
        assert_eq!(
            session.start("aether"),
            LoginEvent::Prompt(Prompt {
                kind: PromptKind::Secret,
                message: "Password: ".into()
            })
        );
        assert_eq!(session.respond("hunter2"), LoginEvent::Authenticated);
        assert_eq!(
            session.connection.stream.sent(),
            vec![
                Request::CreateSession {
                    username: "aether".into()
                },
                Request::PostAuthMessageResponse {
                    response: Some("hunter2".into())
                }
            ]
        );
        assert!(LoginEvent::Authenticated.is_terminal());
    }

    #[test]
    fn informative_messages_are_answered_and_skipped() {
        let mut session = session(&[
            r#"{"type":"auth_message","auth_message_type":"info","auth_message":"Welcome to AetherOS"}"#,
            r#"{"type":"auth_message","auth_message_type":"error","auth_message":"Last failed login"}"#,
            r#"{"type":"auth_message","auth_message_type":"visible","auth_message":"login: "}"#,
        ]);
        assert_eq!(
            session.start("aether"),
            LoginEvent::Prompt(Prompt {
                kind: PromptKind::Visible,
                message: "login: ".into()
            })
        );
        // Both informative messages were acknowledged with no response.
        let sent = session.connection.stream.sent();
        assert_eq!(sent.len(), 3);
        assert_eq!(
            sent[1],
            Request::PostAuthMessageResponse { response: None }
        );
        assert_eq!(
            sent[2],
            Request::PostAuthMessageResponse { response: None }
        );
    }

    #[test]
    fn wrong_password_reports_attempts_left() {
        let mut session = GreetdSession::with_max_attempts(
            GreetdConnection::from_stream(Scripted::new(&[
                r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
                r#"{"type":"error","error_type":"auth_error","description":"Authentication failure"}"#,
                r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
                r#"{"type":"success"}"#,
            ])),
            3,
        );
        assert!(matches!(session.start("aether"), LoginEvent::Prompt(_)));
        let denied = session.respond("wrong");
        assert_eq!(
            denied,
            LoginEvent::Denied {
                message: "Authentication failure".into(),
                attempts_left: 2
            }
        );
        assert!(denied.message().contains("2 attempts left"));
        assert!(matches!(session.respond("right"), LoginEvent::Prompt(_)));
        assert_eq!(session.respond("hunter2"), LoginEvent::Authenticated);
        assert_eq!(session.attempts(), 1);
    }

    #[test]
    fn exhausted_attempts_cancel_the_session() {
        let mut session = GreetdSession::with_max_attempts(
            GreetdConnection::from_stream(Scripted::new(&[
                r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
                r#"{"type":"error","error_type":"auth_error","description":"nope"}"#,
            ])),
            1,
        );
        assert!(matches!(session.start("aether"), LoginEvent::Prompt(_)));
        let locked = session.respond("wrong");
        assert!(matches!(locked, LoginEvent::LockedOut { .. }));
        assert!(locked.is_terminal());
        let sent = session.connection.stream.sent();
        assert_eq!(sent.last(), Some(&Request::CancelSession));
    }

    #[test]
    fn missing_greetd_is_unavailable_not_a_crash() {
        // A missing socket maps to Unavailable (terminal, never a panic).
        let event = unavailable(GreetdError::NotAvailable("/run/greetd-ipc: no such file".into()));
        assert!(matches!(event, LoginEvent::Unavailable(_)));
        assert!(event.message().contains("not available"));
        assert!(event.is_terminal());

        // A closed connection is an I/O error with an honest message.
        let mut session = GreetdSession::new(GreetdConnection::from_stream(Scripted::new(&[])));
        let event = session.start("aether");
        assert!(matches!(event, LoginEvent::Error(_)));
        assert!(event.message().contains("closed"));

        let mut session = session_with_backend_error(r#"{"type":"error","error_type":"error","description":"PAM service missing"}"#);
        let event = session.start("aether");
        assert_eq!(event, LoginEvent::Error("PAM service missing".into()));
    }

    fn session_with_backend_error(response: &str) -> GreetdSession<Scripted> {
        session(&[response])
    }

    #[test]
    fn begin_session_sends_command_and_environment() {
        let mut session = session(&[r#"{"type":"success"}"#]);
        session
            .begin_session(
                &["aether-shell".into(), "--session".into()],
                &["XDG_SESSION_TYPE=wayland".into()],
            )
            .unwrap();
        assert_eq!(
            session.connection.stream.sent(),
            vec![Request::StartSession {
                cmd: vec!["aether-shell".into(), "--session".into()],
                env: vec!["XDG_SESSION_TYPE=wayland".into()]
            }]
        );

        let mut failed = session_with_backend_error(
            r#"{"type":"error","error_type":"error","description":"session failed"}"#,
        );
        let error = failed
            .begin_session(&["aether-shell".into()], &[])
            .unwrap_err();
        assert!(error.to_string().contains("session failed"));
    }

    #[test]
    fn greeter_config_parses_and_falls_back() {
        let config = GreeterConfig::parse(
            r#"
            user = "anik"
            max_attempts = 5
            session = ["aether-shell", "--login"]
            env = ["XDG_SESSION_TYPE=wayland"]
            "#,
        )
        .unwrap();
        assert_eq!(config.user, "anik");
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.session_label(), "aether-shell --login");
        assert_eq!(config.env, ["XDG_SESSION_TYPE=wayland"]);

        // Empty/partial files still produce a startable session.
        let fallback = GreeterConfig::parse("user = \"aether\"").unwrap();
        assert_eq!(fallback.session, ["aether-shell"]);
        assert_eq!(fallback.max_attempts, DEFAULT_MAX_ATTEMPTS);
        let no_attempts = GreeterConfig::parse("max_attempts = 0").unwrap();
        assert_eq!(no_attempts.max_attempts, 1);
        assert!(GreeterConfig::parse("max_attempts = ").is_err());
    }
}
