//! Greeter controller: the only place that mutates the login surface
//! (P4-T06, SHL-AC-08).
//!
//! The controller maps [`LoginEvent`]s from a [`LoginSession`] onto the Slint
//! properties of `GreeterRoot`. It keeps one piece of state the UI does not:
//! which kind of prompt is pending, so a single "Sign in" press can drive a
//! whole create-session → password → begin-session exchange.
//!
//! Tests pass a scripted session; the real binary passes
//! [`crate::greeter::GreetdSession`] over the greetd socket.

use std::cell::RefCell;
use std::rc::Rc;

use crate::greeter::{
    GreeterConfig, LoginEvent, LoginSession, PromptKind,
};
use crate::GreeterRoot;

/// Where the login exchange is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// No exchange in flight: the next submit calls `create_session`.
    Idle,
    /// greetd asked for a password; the next submit answers it.
    AwaitingSecret,
    /// greetd asked a visible question; the answer comes from the user field.
    AwaitingVisible,
    /// Credentials accepted and `start_session` was requested.
    Authenticated,
    /// Too many attempts; submissions are ignored until the greeter restarts.
    LockedOut,
}

pub struct GreeterController {
    ui: GreeterRoot,
    session: RefCell<Box<dyn LoginSession>>,
    config: GreeterConfig,
    stage: RefCell<Stage>,
    last_prompt: RefCell<String>,
    submitted_sessions: RefCell<Vec<(Vec<String>, Vec<String>)>>,
}

impl GreeterController {
    pub fn new(ui: GreeterRoot, session: Box<dyn LoginSession>, config: GreeterConfig) -> Rc<Self> {
        let controller = Rc::new(GreeterController {
            ui,
            session: RefCell::new(session),
            config,
            stage: RefCell::new(Stage::Idle),
            last_prompt: RefCell::new(String::new()),
            submitted_sessions: RefCell::new(Vec::new()),
        });
        controller.ui.set_username(controller.config.user.clone().into());
        controller.wire();
        controller
    }

    pub fn ui(&self) -> &GreeterRoot {
        &self.ui
    }

    pub fn stage(&self) -> Stage {
        *self.stage.borrow()
    }

    pub fn config(&self) -> &GreeterConfig {
        &self.config
    }

    /// Sessions the controller asked greetd to start (empty until success).
    pub fn submitted_sessions(&self) -> std::cell::Ref<'_, Vec<(Vec<String>, Vec<String>)>> {
        self.submitted_sessions.borrow()
    }

    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.on_submitted(move |user, password| {
            if let Some(controller) = weak.upgrade() {
                controller.submit(user.as_str(), password.as_str());
            }
        });
    }

    /// One "Sign in" press: starts the exchange or answers the pending prompt.
    ///
    /// The form collects the user name and the password together, so when
    /// greetd answers `create_session` with a secret prompt the typed password
    /// is answered in the same press. Visible prompts (e.g. "login: ") use the
    /// user field instead.
    pub fn submit(&self, user: &str, password: &str) {
        let stage = self.stage();
        if matches!(stage, Stage::Authenticated | Stage::LockedOut) {
            return;
        }
        self.ui.set_busy(true);
        self.ui.set_failed(false);

        let mut event = match stage {
            Stage::AwaitingSecret => self.session.borrow_mut().respond(password),
            Stage::AwaitingVisible => self.session.borrow_mut().respond(user),
            _ => self.session.borrow_mut().start(user),
        };
        if let LoginEvent::Prompt(prompt) = &event {
            if prompt.kind == PromptKind::Secret && !password.is_empty() {
                event = self.session.borrow_mut().respond(password);
            }
        }
        self.ui.set_password(slint::SharedString::new());
        self.handle(event);
        self.ui.set_busy(false);
    }

    fn handle(&self, event: LoginEvent) {
        match event {
            LoginEvent::Prompt(prompt) => {
                *self.last_prompt.borrow_mut() = prompt.message.clone();
                match prompt.kind {
                    PromptKind::Secret => *self.stage.borrow_mut() = Stage::AwaitingSecret,
                    PromptKind::Visible => *self.stage.borrow_mut() = Stage::AwaitingVisible,
                }
                self.ui.set_message(prompt.message.into());
                self.ui.set_failed(false);
            }
            LoginEvent::Authenticated => {
                *self.stage.borrow_mut() = Stage::Authenticated;
                self.ui.set_message("Signing in…".into());
                self.ui.set_failed(false);
                let result = self.session.borrow_mut().begin_session(
                    &self.config.session,
                    &self.config.env,
                );
                match result {
                    Ok(()) => {
                        self.submitted_sessions
                            .borrow_mut()
                            .push((self.config.session.clone(), self.config.env.clone()));
                        self.ui.set_message("Starting session…".into());
                        self.ui
                            .set_hint(format!("Starting {}", self.config.session_label()).into());
                    }
                    Err(error) => {
                        // greetd refused to start the session: let the user try
                        // again rather than leaving a dead screen.
                        *self.stage.borrow_mut() = Stage::Idle;
                        self.ui.set_failed(true);
                        self.ui.set_message(error.to_string().into());
                    }
                }
            }
            LoginEvent::Denied {
                message,
                attempts_left,
            } => {
                *self.stage.borrow_mut() = Stage::Idle;
                self.ui.set_failed(true);
                self.ui.set_message(
                    format!("{message} — {attempts_left} attempt(s) left").into(),
                );
            }
            LoginEvent::LockedOut { message } => {
                *self.stage.borrow_mut() = Stage::LockedOut;
                self.ui.set_failed(true);
                self.ui.set_message(message.into());
                self.ui.set_hint("Too many attempts — wait or restart".into());
            }
            LoginEvent::Unavailable(message) | LoginEvent::Error(message) => {
                *self.stage.borrow_mut() = Stage::Idle;
                self.ui.set_failed(true);
                self.ui.set_message(message.into());
            }
        }
    }
}

/// A session that always accepts one password: used by `--render` and the
/// headless screenshot checks, never by the real login.
pub struct DemoSession {
    password: String,
    started: bool,
    fail_next: bool,
}

impl DemoSession {
    pub fn new(password: &str) -> Self {
        DemoSession {
            password: password.to_string(),
            started: false,
            fail_next: false,
        }
    }

    pub fn failing() -> Self {
        DemoSession {
            password: String::new(),
            started: false,
            fail_next: true,
        }
    }
}

impl LoginSession for DemoSession {
    fn start(&mut self, _username: &str) -> LoginEvent {
        self.started = true;
        LoginEvent::Prompt(crate::greeter::Prompt {
            kind: PromptKind::Secret,
            message: "Password: ".into(),
        })
    }

    fn respond(&mut self, answer: &str) -> LoginEvent {
        if !self.started {
            return LoginEvent::Error("session not started".into());
        }
        if self.fail_next || answer != self.password {
            return LoginEvent::Denied {
                message: "Incorrect password".into(),
                attempts_left: 2,
            };
        }
        LoginEvent::Authenticated
    }

    fn begin_session(
        &mut self,
        _cmd: &[String],
        _env: &[String],
    ) -> Result<(), crate::greetd::GreetdError> {
        Ok(())
    }

    fn cancel(&mut self) {}
}

