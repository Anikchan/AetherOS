//! Headless tests for the greetd greeter controller (P4-T06, SHL-AC-08).
//!
//! The Slint testing backend installs one process-wide platform, so every
//! scenario runs inside a single test function. The session is scripted: no
//! greetd, PAM or display server is needed.

use std::collections::VecDeque;
use std::rc::Rc;

use aether_shell::greeter::{GreeterConfig, LoginEvent, LoginSession, Prompt, PromptKind};
use aether_shell::greeter_app::{GreeterController, Stage};
use aether_shell::greetd::{ErrorType, GreetdError};
use aether_shell::GreeterRoot;
use slint::ComponentHandle;

struct ScriptedSession {
    events: VecDeque<LoginEvent>,
    started_usernames: Vec<String>,
    answers: Vec<String>,
    begin: Vec<(Vec<String>, Vec<String>)>,
    begin_result: Result<(), String>,
}

impl ScriptedSession {
    fn new(events: Vec<LoginEvent>) -> Self {
        ScriptedSession {
            events: events.into(),
            started_usernames: Vec::new(),
            answers: Vec::new(),
            begin: Vec::new(),
            begin_result: Ok(()),
        }
    }

    fn refused_begin() -> Self {
        let mut session = ScriptedSession::new(vec![LoginEvent::Authenticated]);
        session.begin_result = Err("session failed".into());
        session
    }
}

impl LoginSession for ScriptedSession {
    fn start(&mut self, username: &str) -> LoginEvent {
        self.started_usernames.push(username.to_string());
        self.events
            .pop_front()
            .unwrap_or(LoginEvent::Error("no scripted event".into()))
    }

    fn respond(&mut self, answer: &str) -> LoginEvent {
        self.answers.push(answer.to_string());
        self.events
            .pop_front()
            .unwrap_or(LoginEvent::Error("no scripted event".into()))
    }

    fn begin_session(&mut self, cmd: &[String], env: &[String]) -> Result<(), GreetdError> {
        self.begin.push((cmd.to_vec(), env.to_vec()));
        self.begin_result
            .as_ref()
            .map(|_| ())
            .map_err(|error| GreetdError::Backend {
                error_type: ErrorType::Error,
                description: error.clone(),
            })
    }

    fn cancel(&mut self) {}
}

fn controller(session: ScriptedSession) -> Rc<GreeterController> {
    let ui = GreeterRoot::new().expect("login surface builds headlessly");
    GreeterController::new(ui, Box::new(session), GreeterConfig::default())
}

fn secret_prompt() -> LoginEvent {
    LoginEvent::Prompt(Prompt {
        kind: PromptKind::Secret,
        message: "Password: ".into(),
    })
}

#[test]
fn greeter_login_flow_is_driven_by_the_controller() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    // --- a single press completes create-session + password + start ------
    {
        let controller = controller(ScriptedSession::new(vec![
            secret_prompt(),
            LoginEvent::Authenticated,
        ]));
        controller.submit("aether", "hunter2");
        assert_eq!(controller.stage(), Stage::Authenticated);
        assert_eq!(
            controller.ui().get_message().to_string(),
            "Starting session…"
        );
        assert!(controller.ui().get_password().is_empty(), "password is not kept");
        assert_eq!(controller.ui().get_hint().to_string(), "Starting aether-shell");
        assert_eq!(controller.submitted_sessions().len(), 1);
        assert_eq!(
            controller.submitted_sessions()[0].0,
            vec!["aether-shell".to_string()]
        );
    }

    // --- a wrong password shows attempts left and allows a retry ---------
    {
        let controller = controller(ScriptedSession::new(vec![
            secret_prompt(),
            LoginEvent::Denied {
                message: "Incorrect password".into(),
                attempts_left: 2,
            },
            secret_prompt(),
            LoginEvent::Authenticated,
        ]));
        controller.submit("aether", "wrong");
        assert_eq!(controller.stage(), Stage::Idle);
        assert!(controller.ui().get_failed());
        assert!(controller.ui().get_message().contains("2 attempt(s) left"));

        controller.submit("aether", "hunter2");
        assert_eq!(controller.stage(), Stage::Authenticated);
        assert_eq!(controller.submitted_sessions().len(), 1);
    }

    // --- a visible PAM question uses the user field ----------------------
    {
        let controller = controller(ScriptedSession::new(vec![
            LoginEvent::Prompt(Prompt {
                kind: PromptKind::Visible,
                message: "login: ".into(),
            }),
            LoginEvent::Authenticated,
        ]));
        controller.submit("aether", "");
        assert_eq!(controller.stage(), Stage::AwaitingVisible);
        controller.submit("anik", "");
        assert_eq!(controller.stage(), Stage::Authenticated);
    }

    // --- lockout and unavailable sessions are explained, not silent ------
    {
        let controller = controller(ScriptedSession::new(vec![LoginEvent::LockedOut {
            message: "Incorrect password — too many attempts".into(),
        }]));
        controller.submit("aether", "wrong");
        assert_eq!(controller.stage(), Stage::LockedOut);
        assert!(controller.ui().get_hint().contains("Too many attempts"));
        controller.submit("aether", "wrong");
        assert_eq!(controller.stage(), Stage::LockedOut, "locked out ignores input");
    }
    {
        let controller = controller(ScriptedSession::new(vec![LoginEvent::Unavailable(
            "greetd is not available: /run/greetd-ipc".into(),
        )]));
        controller.submit("aether", "x");
        assert_eq!(controller.stage(), Stage::Idle);
        assert!(controller.ui().get_message().contains("not available"));
    }

    // --- greetd refusing start_session returns to the login form ---------
    {
        let controller = controller(ScriptedSession::refused_begin());
        controller.submit("aether", "x");
        assert_eq!(controller.stage(), Stage::Idle);
        assert!(controller.ui().get_failed());
        assert!(controller.ui().get_message().contains("session failed"));
        assert!(controller.submitted_sessions().is_empty());
    }

    // --- the greeting surface exposes accessible fields ------------------
    {
        let ui = GreeterRoot::new().unwrap();
        ui.show().unwrap();
        assert_eq!(ui.get_username().to_string(), "aether");
        assert_eq!(ui.get_host_label().to_string(), "AetherOS");
        let session = scripted_session_starts_without_prompt();
        let controller = GreeterController::new(ui, Box::new(session), GreeterConfig::default());
        assert_eq!(controller.stage(), Stage::Idle);
    }
}

fn scripted_session_starts_without_prompt() -> ScriptedSession {
    ScriptedSession::new(vec![secret_prompt()])
}
