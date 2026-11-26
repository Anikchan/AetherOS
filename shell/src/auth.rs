//! Authentication backends for the lock screen and login (P4-T06).
//!
//! Two paths exist, and both are behind [`Authenticator`] so the controller and
//! its tests never depend on the host:
//!
//! * **Login (greeter):** greetd runs as root, calls PAM itself and drives the
//!   conversation over IPC ([`crate::greeter`]).
//! * **Lock screen (inside a session):** there is no greetd to ask, so a Linux
//!   build links `libpam` and authenticates against the `aether-lock` service
//!   ([`pam::PamAuthenticator`]). The FFI is behind the `pam` feature because
//!   the macOS development host has no `libpam`; `cargo check --features pam`
//!   type-checks it.
//!
//! When neither is configured (development host, headless tests) the
//! [`LocalCredentials`] prototype keeps the demo account working: a salted
//! digest compared in constant time, never a plain-text password.
//!
//! PAM status codes and the conversation state machine are pure Rust and
//! unit-tested on every platform; only the syscalls are platform-specific.

use std::fmt;

use crate::model::Credential;

/// Result of one authentication attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Allowed,
    /// Wrong credentials (a new attempt is allowed).
    Denied(String),
    /// The backend cannot answer right now (service missing, PAM error).
    Unavailable(String),
}

impl AuthOutcome {
    pub fn is_allowed(&self) -> bool {
        matches!(self, AuthOutcome::Allowed)
    }

    /// Message for the lock screen; empty on success.
    pub fn message(&self) -> String {
        match self {
            AuthOutcome::Allowed => String::new(),
            AuthOutcome::Denied(reason) | AuthOutcome::Unavailable(reason) => reason.clone(),
        }
    }
}

pub trait Authenticator {
    /// Short backend name for `--status` and the debug log.
    fn name(&self) -> &'static str;

    fn authenticate(&self, user: &str, password: &str) -> AuthOutcome;
}

/// Development fallback: the demo account from [`Credential`].
pub struct LocalCredentials {
    credential: Credential,
    user: Option<String>,
}

impl Default for LocalCredentials {
    fn default() -> Self {
        LocalCredentials {
            credential: Credential::new("aether-demo", "aether"),
            user: None,
        }
    }
}

impl LocalCredentials {
    pub fn new(salt: &str, password: &str) -> Self {
        LocalCredentials {
            credential: Credential::new(salt, password),
            user: None,
        }
    }

    /// Only accept `user` in addition to the password.
    pub fn for_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }
}

impl Authenticator for LocalCredentials {
    fn name(&self) -> &'static str {
        "local credential (prototype)"
    }

    fn authenticate(&self, user: &str, password: &str) -> AuthOutcome {
        if let Some(expected) = &self.user {
            if expected != user {
                return AuthOutcome::Denied("Incorrect user or password — try again".into());
            }
        }
        if self.credential.verify(password) {
            AuthOutcome::Allowed
        } else {
            AuthOutcome::Denied("Incorrect password — try again".into())
        }
    }
}

/// Scripted backend for tests and screenshots.
pub struct MockAuthenticator {
    user: String,
    password: String,
    available: bool,
}

impl MockAuthenticator {
    pub fn new(user: &str, password: &str) -> Self {
        MockAuthenticator {
            user: user.into(),
            password: password.into(),
            available: true,
        }
    }

    /// A backend that reports itself unavailable (e.g. missing PAM service).
    pub fn unavailable() -> Self {
        MockAuthenticator {
            user: String::new(),
            password: String::new(),
            available: false,
        }
    }
}

impl Authenticator for MockAuthenticator {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn authenticate(&self, user: &str, password: &str) -> AuthOutcome {
        if !self.available {
            return AuthOutcome::Unavailable("authentication service unavailable".into());
        }
        if user == self.user && password == self.password {
            AuthOutcome::Allowed
        } else {
            AuthOutcome::Denied("Incorrect password — try again".into())
        }
    }
}

/// The backend a live session uses: PAM on Linux when built with the feature,
/// otherwise the local credential prototype.
pub fn default_authenticator() -> Box<dyn Authenticator> {
    #[cfg(all(target_os = "linux", feature = "pam"))]
    {
        return Box::new(pam::PamAuthenticator::new(pam::LOCK_SERVICE));
    }
    #[cfg(not(all(target_os = "linux", feature = "pam")))]
    {
        Box::new(LocalCredentials::default())
    }
}

/// Whether this build can authenticate through PAM directly.
pub fn pam_linked() -> bool {
    cfg!(all(target_os = "linux", feature = "pam"))
}

/// The user that owns the running session (the account PAM must check).
pub fn session_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .filter(|user| !user.is_empty())
        .unwrap_or_else(|| "aether".to_string())
}

// ------------------------------------------------------------------- PAM ---

/// PAM conversation message styles (Linux-PAM `pam_message.msg_style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PamMessageStyle {
    /// `PAM_PROMPT_ECHO_OFF` — ask for the password.
    PromptEchoOff,
    /// `PAM_PROMPT_ECHO_ON` — ask for a visible answer (usually the user name).
    PromptEchoOn,
    /// `PAM_ERROR_MSG` — show a message, no answer.
    ErrorMsg,
    /// `PAM_TEXT_INFO` — show information, no answer.
    TextInfo,
}

impl PamMessageStyle {
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(PamMessageStyle::PromptEchoOff),
            2 => Some(PamMessageStyle::PromptEchoOn),
            3 => Some(PamMessageStyle::ErrorMsg),
            4 => Some(PamMessageStyle::TextInfo),
            _ => None,
        }
    }

    pub fn code(self) -> i32 {
        match self {
            PamMessageStyle::PromptEchoOff => 1,
            PamMessageStyle::PromptEchoOn => 2,
            PamMessageStyle::ErrorMsg => 3,
            PamMessageStyle::TextInfo => 4,
        }
    }
}

/// The subset of Linux-PAM status codes the lock screen maps to UI text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PamStatus {
    Success,
    AuthErr,
    PermDenied,
    CredInsufficient,
    AuthInfoUnavailable,
    UserUnknown,
    MaxTries,
    AccountExpired,
    NewAuthTokRequired,
    ConvErr,
    SystemErr,
    ServiceErr,
    BufErr,
    Abort,
    Other(i32),
}

impl PamStatus {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => PamStatus::Success,
            7 => PamStatus::AuthErr,
            6 => PamStatus::PermDenied,
            8 => PamStatus::CredInsufficient,
            9 => PamStatus::AuthInfoUnavailable,
            10 => PamStatus::UserUnknown,
            11 => PamStatus::MaxTries,
            13 => PamStatus::AccountExpired,
            12 => PamStatus::NewAuthTokRequired,
            19 => PamStatus::ConvErr,
            4 => PamStatus::SystemErr,
            3 => PamStatus::ServiceErr,
            5 => PamStatus::BufErr,
            26 => PamStatus::Abort,
            other => PamStatus::Other(other),
        }
    }

    pub fn code(self) -> i32 {
        match self {
            PamStatus::Success => 0,
            PamStatus::AuthErr => 7,
            PamStatus::PermDenied => 6,
            PamStatus::CredInsufficient => 8,
            PamStatus::AuthInfoUnavailable => 9,
            PamStatus::UserUnknown => 10,
            PamStatus::MaxTries => 11,
            PamStatus::AccountExpired => 13,
            PamStatus::NewAuthTokRequired => 12,
            PamStatus::ConvErr => 19,
            PamStatus::SystemErr => 4,
            PamStatus::ServiceErr => 3,
            PamStatus::BufErr => 5,
            PamStatus::Abort => 26,
            PamStatus::Other(code) => code,
        }
    }

    /// Map a status to the backend result. Only `Success` unlocks.
    pub fn into_outcome(self) -> AuthOutcome {
        match self {
            PamStatus::Success => AuthOutcome::Allowed,
            PamStatus::AuthErr | PamStatus::PermDenied | PamStatus::MaxTries => {
                AuthOutcome::Denied("Incorrect password — try again".into())
            }
            PamStatus::CredInsufficient => {
                AuthOutcome::Denied("Authentication unavailable for this account".into())
            }
            PamStatus::AuthInfoUnavailable => {
                AuthOutcome::Unavailable("Authentication information unavailable".into())
            }
            PamStatus::UserUnknown => AuthOutcome::Denied("Unknown user".into()),
            PamStatus::AccountExpired => AuthOutcome::Denied("Account expired".into()),
            PamStatus::NewAuthTokRequired => {
                AuthOutcome::Denied("Password change required before unlocking".into())
            }
            PamStatus::ConvErr => AuthOutcome::Unavailable("PAM conversation failed".into()),
            PamStatus::SystemErr | PamStatus::ServiceErr | PamStatus::BufErr | PamStatus::Abort => {
                AuthOutcome::Unavailable(format!("Authentication service error ({self:?})"))
            }
            PamStatus::Other(code) => {
                AuthOutcome::Unavailable(format!("Authentication error (PAM {code})"))
            }
        }
    }
}

impl fmt::Display for PamStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PAM {}", self.code())
    }
}

/// Answers PAM conversation messages without touching the terminal.
///
/// The applied sequence `aether-lock` asks exactly two questions (password and
/// optionally the account name); text and error messages are recorded for the
/// UI instead of printed.
#[derive(Debug, Clone)]
pub struct PamConversation {
    user: String,
    password: String,
    notices: Vec<String>,
    prompts: usize,
}

impl PamConversation {
    pub fn new(user: &str, password: &str) -> Self {
        PamConversation {
            user: user.to_string(),
            password: password.to_string(),
            notices: Vec::new(),
            prompts: 0,
        }
    }

    /// Answer one PAM message. `None` means "show it, do not respond".
    pub fn response(&mut self, style: PamMessageStyle, text: &str) -> Option<String> {
        match style {
            PamMessageStyle::PromptEchoOff => {
                self.prompts += 1;
                Some(self.password.clone())
            }
            PamMessageStyle::PromptEchoOn => {
                self.prompts += 1;
                Some(self.user.clone())
            }
            PamMessageStyle::ErrorMsg | PamMessageStyle::TextInfo => {
                if !text.is_empty() {
                    self.notices.push(text.to_string());
                }
                None
            }
        }
    }

    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    pub fn prompts(&self) -> usize {
        self.prompts
    }
}

/// Linux-PAM binding (feature `pam`).
///
/// The FFI is small on purpose: `pam_start` + `pam_authenticate` +
/// `pam_acct_mgmt` + `pam_end` with one conversation callback. Responses are
/// allocated with `malloc` because PAM frees them with `free`.
#[cfg(feature = "pam")]
pub mod pam {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int, c_void};
    use std::ptr::null_mut;

    use super::{AuthOutcome, Authenticator, PamConversation, PamMessageStyle, PamStatus};

    /// PAM service file installed by the AetherOS image.
    pub const LOCK_SERVICE: &str = "aether-lock";
    const PAM_SUCCESS: c_int = 0;
    const PAM_CONV_ERR: c_int = 19;
    const PAM_BUF_ERR: c_int = 5;

    #[repr(C)]
    struct PamMessage {
        msg_style: c_int,
        msg: *const c_char,
    }

    #[repr(C)]
    struct PamResponse {
        resp: *mut c_char,
        resp_retcode: c_int,
    }

    type ConvCallback = unsafe extern "C" fn(
        num_msg: c_int,
        msg: *const *const PamMessage,
        resp: *mut *mut PamResponse,
        appdata_ptr: *mut c_void,
    ) -> c_int;

    #[repr(C)]
    struct PamConv {
        conv: Option<ConvCallback>,
        appdata_ptr: *mut c_void,
    }

    /// Opaque `pam_handle_t`.
    #[repr(C)]
    struct PamHandle {
        _private: [u8; 0],
    }

    extern "C" {
        fn pam_start(
            service: *const c_char,
            user: *const c_char,
            conv: *const PamConv,
            pamh: *mut *mut PamHandle,
        ) -> c_int;
        fn pam_authenticate(pamh: *mut PamHandle, flags: c_int) -> c_int;
        fn pam_acct_mgmt(pamh: *mut PamHandle, flags: c_int) -> c_int;
        fn pam_end(pamh: *mut PamHandle, pam_status: c_int) -> c_int;
        fn malloc(size: usize) -> *mut c_void;
        fn free(pointer: *mut c_void);
    }

    /// Allocate one NUL-terminated copy PAM can `free()`.
    unsafe fn dup_c_string(text: &str) -> *mut c_char {
        let bytes = text.as_bytes();
        let memory = malloc(bytes.len() + 1) as *mut u8;
        if memory.is_null() {
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory, bytes.len());
        *memory.add(bytes.len()) = 0;
        memory as *mut c_char
    }

    unsafe extern "C" fn converse(
        num_msg: c_int,
        messages: *const *const PamMessage,
        responses: *mut *mut PamResponse,
        appdata_ptr: *mut c_void,
    ) -> c_int {
        if num_msg <= 0 || messages.is_null() || responses.is_null() || appdata_ptr.is_null() {
            return PAM_CONV_ERR;
        }
        let conversation = &mut *(appdata_ptr as *mut PamConversation);
        let count = num_msg as usize;
        let answers = malloc(count * std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if answers.is_null() {
            return PAM_BUF_ERR;
        }
        for index in 0..count {
            let message = match (*messages.add(index)).as_ref() {
                Some(message) => message,
                None => {
                    for done in 0..index {
                        let answer = (*answers.add(done)).resp;
                        if !answer.is_null() {
                            free(answer as *mut c_void);
                        }
                    }
                    free(answers as *mut c_void);
                    return PAM_CONV_ERR;
                }
            };
            let text = if message.msg.is_null() {
                String::new()
            } else {
                CStr::from_ptr(message.msg).to_string_lossy().to_string()
            };
            let style = PamMessageStyle::from_code(message.msg_style)
                .unwrap_or(PamMessageStyle::TextInfo);
            let answer = conversation.response(style, &text);
            let has_answer = answer.is_some();
            let pointer = match answer {
                Some(answer) => dup_c_string(&answer),
                None => null_mut(),
            };
            if pointer.is_null() && has_answer {
                for done in 0..index {
                    let previous = (*answers.add(done)).resp;
                    if !previous.is_null() {
                        free(previous as *mut c_void);
                    }
                }
                free(answers as *mut c_void);
                return PAM_BUF_ERR;
            }
            answers.add(index).write(PamResponse {
                resp: pointer,
                resp_retcode: 0,
            });
        }
        *responses = answers;
        PAM_SUCCESS
    }

    pub struct PamAuthenticator {
        service: CString,
    }

    impl PamAuthenticator {
        pub fn new(service: &str) -> Self {
            PamAuthenticator {
                service: CString::new(service)
                    .unwrap_or_else(|_| CString::new(LOCK_SERVICE).expect("static service")),
            }
        }

        pub fn service(&self) -> &str {
            self.service.to_str().unwrap_or(LOCK_SERVICE)
        }

        fn run(&self, user: &str, password: &str) -> (PamStatus, Vec<String>) {
            let user_c = match CString::new(user) {
                Ok(user) => user,
                Err(_) => return (PamStatus::AuthErr, Vec::new()),
            };
            let mut conversation = PamConversation::new(user, password);
            let conv = PamConv {
                conv: Some(converse),
                appdata_ptr: &mut conversation as *mut _ as *mut c_void,
            };
            let mut handle: *mut PamHandle = null_mut();
            let started = unsafe {
                pam_start(
                    self.service.as_ptr(),
                    user_c.as_ptr(),
                    &conv,
                    &mut handle,
                )
            };
            if started != PAM_SUCCESS {
                return (PamStatus::from_code(started), conversation.notices().to_vec());
            }
            let authenticated = unsafe { pam_authenticate(handle, 0) };
            let status = if authenticated == PAM_SUCCESS {
                unsafe { pam_acct_mgmt(handle, 0) }
            } else {
                authenticated
            };
            unsafe {
                pam_end(handle, status);
            }
            (
                PamStatus::from_code(status),
                conversation.notices().to_vec(),
            )
        }
    }

    impl Authenticator for PamAuthenticator {
        fn name(&self) -> &'static str {
            "PAM"
        }

        fn authenticate(&self, user: &str, password: &str) -> AuthOutcome {
            let (status, notices) = self.run(user, password);
            let mut outcome = status.into_outcome();
            if let AuthOutcome::Denied(message) | AuthOutcome::Unavailable(message) = &mut outcome {
                if let Some(notice) = notices.last() {
                    if notice.len() <= 120 {
                        *message = format!("{message} ({notice})");
                    }
                }
            }
            outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_credentials_accept_the_demo_account_only() {
        let authenticator = LocalCredentials::default();
        assert!(authenticator.authenticate("aether", "aether").is_allowed());
        let denied = authenticator.authenticate("aether", "wrong");
        assert!(matches!(denied, AuthOutcome::Denied(_)));
        assert!(denied.message().contains("Incorrect"));
        assert_eq!(authenticator.name(), "local credential (prototype)");

        let bound = LocalCredentials::default().for_user("aether");
        assert!(!bound.authenticate("root", "aether").is_allowed());
        assert!(bound.authenticate("aether", "aether").is_allowed());
    }

    #[test]
    fn mock_backend_can_report_unavailable() {
        let mock = MockAuthenticator::new("aether", "secret");
        assert!(mock.authenticate("aether", "secret").is_allowed());
        assert!(!mock.authenticate("aether", "nope").is_allowed());

        let missing = MockAuthenticator::unavailable();
        let outcome = missing.authenticate("aether", "secret");
        assert!(matches!(outcome, AuthOutcome::Unavailable(_)));
        assert!(outcome.message().contains("unavailable"));
    }

    #[test]
    fn pam_status_codes_match_linux_pam() {
        assert_eq!(PamStatus::from_code(0), PamStatus::Success);
        assert_eq!(PamStatus::from_code(7), PamStatus::AuthErr);
        assert_eq!(PamStatus::from_code(11), PamStatus::MaxTries);
        assert_eq!(PamStatus::from_code(4), PamStatus::SystemErr);
        assert_eq!(PamStatus::from_code(19), PamStatus::ConvErr);
        assert_eq!(PamStatus::from_code(99), PamStatus::Other(99));

        for status in [
            PamStatus::Success,
            PamStatus::AuthErr,
            PamStatus::PermDenied,
            PamStatus::CredInsufficient,
            PamStatus::AuthInfoUnavailable,
            PamStatus::UserUnknown,
            PamStatus::MaxTries,
            PamStatus::AccountExpired,
            PamStatus::NewAuthTokRequired,
            PamStatus::ConvErr,
            PamStatus::SystemErr,
            PamStatus::ServiceErr,
            PamStatus::BufErr,
            PamStatus::Abort,
            PamStatus::Other(42),
        ] {
            assert_eq!(PamStatus::from_code(status.code()), status);
            // Only PAM_SUCCESS unlocks.
            assert_eq!(
                status.into_outcome().is_allowed(),
                status == PamStatus::Success
            );
        }
    }

    #[test]
    fn pam_status_messages_are_user_readable() {
        assert!(PamStatus::AuthErr
            .into_outcome()
            .message()
            .contains("Incorrect password"));
        assert!(PamStatus::MaxTries
            .into_outcome()
            .message()
            .contains("Incorrect password"));
        assert!(PamStatus::NewAuthTokRequired
            .into_outcome()
            .message()
            .contains("Password change required"));
        assert!(matches!(
            PamStatus::SystemErr.into_outcome(),
            AuthOutcome::Unavailable(_)
        ));
    }

    #[test]
    fn pam_conversation_answers_prompts_and_records_notices() {
        let mut conversation = PamConversation::new("aether", "hunter2");
        assert_eq!(
            conversation.response(PamMessageStyle::PromptEchoOff, "Password: "),
            Some("hunter2".to_string())
        );
        assert_eq!(
            conversation.response(PamMessageStyle::PromptEchoOn, "login: "),
            Some("aether".to_string())
        );
        assert_eq!(
            conversation.response(PamMessageStyle::TextInfo, "Welcome"),
            None
        );
        assert_eq!(
            conversation.response(PamMessageStyle::ErrorMsg, "Account locked"),
            None
        );
        assert_eq!(conversation.prompts(), 2);
        assert_eq!(conversation.notices(), ["Welcome", "Account locked"]);

        assert_eq!(PamMessageStyle::from_code(1), Some(PamMessageStyle::PromptEchoOff));
        assert_eq!(PamMessageStyle::from_code(4), Some(PamMessageStyle::TextInfo));
        assert_eq!(PamMessageStyle::from_code(9), None);
    }

    #[test]
    fn default_backend_is_reported_honestly() {
        let authenticator = default_authenticator();
        assert!(!authenticator.name().is_empty());
        assert_eq!(pam_linked(), cfg!(all(target_os = "linux", feature = "pam")));
    }
}
