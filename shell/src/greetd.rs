//! greetd IPC client for the AetherOS greeter (P4-T06, SHL-AC-08).
//!
//! greetd runs as root and owns authentication (it calls PAM itself); the
//! greeter only speaks the documented IPC protocol over a UNIX socket whose
//! path is in `GREETD_SOCK`:
//!
//! ```text
//! <payload-length: u32, native byte order> <JSON payload>
//! ```
//!
//! Requests: `create_session`, `post_auth_message_response`, `start_session`,
//! `cancel_session`. Responses: `auth_message`, `success`, `error`.
//!
//! This module is transport and protocol only. The login state machine lives in
//! [`crate::greeter`], and the lock-screen authenticator in [`crate::auth`].
//! Everything here is exercisable without greetd: [`GreetdConnection`] is
//! generic over `Read + Write`, so the tests drive it with a scripted stream.

use std::fmt;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

/// Hard cap on one frame. greetd messages are a few hundred bytes; anything
/// larger is a protocol error rather than an allocation request.
pub const MAX_FRAME: usize = 1024 * 1024;

/// Fallback socket path when `GREETD_SOCK` is unset (some greetd builds do not
/// export it to the greeter, e.g. when launched from a wrapper).
pub const DEFAULT_SOCKET: &str = "/run/greetd-ipc";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    CreateSession { username: String },
    PostAuthMessageResponse { response: Option<String> },
    StartSession {
        cmd: Vec<String>,
        #[serde(default)]
        env: Vec<String>,
    },
    CancelSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageType {
    Visible,
    Secret,
    Info,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    AuthError,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    AuthMessage {
        auth_message_type: AuthMessageType,
        auth_message: String,
    },
    Success,
    Error {
        error_type: ErrorType,
        description: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreetdError {
    /// greetd or its socket is not present (the greeter never panics on this).
    NotAvailable(String),
    Io(String),
    Protocol(String),
    FrameTooLarge(usize),
    /// greetd answered with `error`.
    Backend { error_type: ErrorType, description: String },
}

impl fmt::Display for GreetdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GreetdError::NotAvailable(what) => write!(f, "greetd is not available: {what}"),
            GreetdError::Io(message) => write!(f, "greetd socket error: {message}"),
            GreetdError::Protocol(message) => write!(f, "greetd protocol error: {message}"),
            GreetdError::FrameTooLarge(size) => {
                write!(f, "greetd frame of {size} bytes exceeds {MAX_FRAME}")
            }
            GreetdError::Backend { description, .. } => write!(f, "greetd: {description}"),
        }
    }
}

impl std::error::Error for GreetdError {}

/// Write one length-prefixed frame.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<(), GreetdError> {
    if payload.len() > MAX_FRAME {
        return Err(GreetdError::FrameTooLarge(payload.len()));
    }
    let length = payload.len() as u32;
    // Native byte order, as specified by greetd-ipc(7).
    writer
        .write_all(&length.to_ne_bytes())
        .and_then(|()| writer.write_all(payload))
        .and_then(|()| writer.flush())
        .map_err(|error| GreetdError::Io(error.to_string()))
}

/// Read one length-prefixed frame.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, GreetdError> {
    let mut header = [0u8; 4];
    let mut filled = 0;
    while filled < header.len() {
        match reader.read(&mut header[filled..]) {
            Ok(0) => {
                return Err(GreetdError::Io(if filled == 0 {
                    "connection closed by greetd".to_string()
                } else {
                    "truncated frame header".to_string()
                }));
            }
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(GreetdError::Io(error.to_string())),
        }
    }
    let length = u32::from_ne_bytes(header) as usize;
    if length > MAX_FRAME {
        return Err(GreetdError::FrameTooLarge(length));
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| GreetdError::Io(error.to_string()))?;
    Ok(payload)
}

/// A greetd connection over any `Read + Write` transport.
pub struct GreetdConnection<S> {
    pub(crate) stream: S,
}

#[cfg(unix)]
impl GreetdConnection<std::os::unix::net::UnixStream> {
    /// Path from `GREETD_SOCK`, falling back to [`DEFAULT_SOCKET`].
    pub fn socket_path() -> std::path::PathBuf {
        std::env::var_os("GREETD_SOCK")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_SOCKET))
    }

    /// Connect to the socket greetd handed to this greeter.
    pub fn connect() -> Result<Self, GreetdError> {
        GreetdConnection::connect_to(&GreetdConnection::<std::os::unix::net::UnixStream>::socket_path())
    }

    pub fn connect_to(path: &std::path::Path) -> Result<Self, GreetdError> {
        let stream = std::os::unix::net::UnixStream::connect(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                GreetdError::NotAvailable(format!("{}: {error}", path.display()))
            }
            _ => GreetdError::Io(format!("{}: {error}", path.display())),
        })?;
        Ok(GreetdConnection { stream })
    }
}

impl<S: Read + Write> GreetdConnection<S> {
    pub fn from_stream(stream: S) -> Self {
        GreetdConnection { stream }
    }

    pub fn send(&mut self, request: &Request) -> Result<(), GreetdError> {
        let payload = serde_json::to_vec(request)
            .map_err(|error| GreetdError::Protocol(format!("cannot encode request: {error}")))?;
        write_frame(&mut self.stream, &payload)
    }

    pub fn receive(&mut self) -> Result<Response, GreetdError> {
        let payload = read_frame(&mut self.stream)?;
        serde_json::from_slice(&payload)
            .map_err(|error| GreetdError::Protocol(format!("cannot decode response: {error}")))
    }

    /// Send one request and read its reply.
    pub fn request(&mut self, request: &Request) -> Result<Response, GreetdError> {
        self.send(request)?;
        self.receive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Scripted greetd: answers with queued frames and records what we wrote.
    struct Scripted {
        responses: std::collections::VecDeque<Vec<u8>>,
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
            while let Ok(payload) = read_frame(&mut reader) {
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

    #[test]
    fn requests_serialize_exactly_as_documented() {
        let json = serde_json::to_string(&Request::CreateSession {
            username: "bob".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"create_session","username":"bob"}"#);

        let json = serde_json::to_string(&Request::PostAuthMessageResponse {
            response: Some("correct horse".into()),
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"type":"post_auth_message_response","response":"correct horse"}"#
        );

        // Informative prompts are answered with no response at all.
        let json = serde_json::to_string(&Request::PostAuthMessageResponse { response: None })
            .unwrap();
        assert_eq!(json, r#"{"type":"post_auth_message_response","response":null}"#);

        let json = serde_json::to_string(&Request::StartSession {
            cmd: vec!["aether-shell".into()],
            env: vec![],
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"type":"start_session","cmd":["aether-shell"],"env":[]}"#
        );

        let json = serde_json::to_string(&Request::CancelSession).unwrap();
        assert_eq!(json, r#"{"type":"cancel_session"}"#);
    }

    #[test]
    fn frames_use_a_native_u32_length_prefix() {
        let mut frame = Vec::new();
        write_frame(&mut frame, b"{}").unwrap();
        assert_eq!(frame.len(), 6);
        assert_eq!(&frame[..4], &2u32.to_ne_bytes());
        assert_eq!(&frame[4..], b"{}");
        assert_eq!(read_frame(&mut Cursor::new(frame)).unwrap(), b"{}");
    }

    #[test]
    fn framing_rejects_oversized_and_truncated_frames() {
        let mut frame = Vec::new();
        let length = (MAX_FRAME as u32 + 1).to_ne_bytes();
        frame.extend_from_slice(&length);
        assert!(matches!(
            read_frame(&mut Cursor::new(frame)),
            Err(GreetdError::FrameTooLarge(_))
        ));

        // A header with fewer payload bytes than promised is an I/O error, not
        // a hang or a panic.
        let mut truncated = Vec::new();
        truncated.extend_from_slice(&8u32.to_ne_bytes());
        truncated.extend_from_slice(b"{}");
        assert!(matches!(
            read_frame(&mut Cursor::new(truncated)),
            Err(GreetdError::Io(_))
        ));

        assert!(matches!(
            read_frame(&mut Cursor::new(Vec::new())),
            Err(GreetdError::Io(_))
        ));
        assert!(matches!(
            write_frame(&mut Vec::new(), &vec![0u8; MAX_FRAME + 1]),
            Err(GreetdError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn responses_decode_every_variant() {
        let secret: Response = serde_json::from_str(
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
        )
        .unwrap();
        assert_eq!(
            secret,
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password: ".into()
            }
        );

        let error: Response = serde_json::from_str(
            r#"{"type":"error","error_type":"auth_error","description":"no"}"#,
        )
        .unwrap();
        assert_eq!(
            error,
            Response::Error {
                error_type: ErrorType::AuthError,
                description: "no".into()
            }
        );

        assert_eq!(
            serde_json::from_str::<Response>(r#"{"type":"success"}"#).unwrap(),
            Response::Success
        );
    }

    #[test]
    fn connection_round_trips_requests_and_responses() {
        let scripted = Scripted::new(&[
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
            r#"{"type":"success"}"#,
        ]);
        let mut connection = GreetdConnection::from_stream(scripted);

        let response = connection
            .request(&Request::CreateSession {
                username: "aether".into(),
            })
            .unwrap();
        assert!(matches!(
            response,
            Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                ..
            }
        ));

        assert_eq!(
            connection
                .request(&Request::PostAuthMessageResponse {
                    response: Some("hunter2".into())
                })
                .unwrap(),
            Response::Success
        );

        let sent = connection.stream.sent();
        assert_eq!(
            sent,
            vec![
                Request::CreateSession {
                    username: "aether".into()
                },
                Request::PostAuthMessageResponse {
                    response: Some("hunter2".into())
                }
            ]
        );
    }

    #[test]
    fn a_missing_socket_is_not_available_rather_than_a_panic() {
        let missing = std::env::temp_dir().join("-aether-no-greetd-socket-");
        let _ = std::fs::remove_file(&missing);
        #[cfg(unix)]
        {
            let error =
                GreetdConnection::connect_to(&missing).err().expect("no socket at that path");
            assert!(matches!(error, GreetdError::NotAvailable(_)));
            assert!(error.to_string().contains("not available"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn connects_over_a_real_unix_socket() {
        use std::io::BufReader;
        use std::os::unix::net::UnixListener;

        let path = std::env::temp_dir().join(format!(
            "aether-greetd-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let cleanup = path.clone();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let payload = read_frame(&mut reader).unwrap();
            let request: Request = serde_json::from_slice(&payload).unwrap();
            assert_eq!(
                request,
                Request::CreateSession {
                    username: "aether".into()
                }
            );
            let mut writer = stream;
            let reply = serde_json::to_vec(&Response::Success).unwrap();
            write_frame(&mut writer, &reply).unwrap();
            let _ = std::fs::remove_file(cleanup);
        });

        let mut connection = GreetdConnection::connect_to(&path).unwrap();
        assert_eq!(
            connection
                .request(&Request::CreateSession {
                    username: "aether".into()
                })
                .unwrap(),
            Response::Success
        );
        server.join().unwrap();
    }
}
