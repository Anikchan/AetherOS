//! Lifecycle protocol for apps (P5-T03, D3 section 5).
//!
//! Apps that use the AetherOS SDK bind a Unix socket named after their app
//! identity. The daemon sends newline-delimited JSON:
//!
//! ```text
//! {"type":"will-suspend","app":"org.example.Mail"}
//! {"ack":"accepted","app":"org.example.Mail"}
//! ```
//!
//! `WillSuspend` waits up to 2 s for `accepted`; on timeout the app is
//! suspended anyway unless it holds an exception. `MemoryWarning` waits up to
//! 500 ms. `DidResume` and `BackgroundBudgetExceeded` are informational and do
//! not wait. Apps that do not use the SDK still get suspension, just without
//! the messages.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::{AppId, SCOPE_PREFIX};

/// Messages sent to an app (D3 section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleMessage {
    WillSuspend,
    DidResume,
    MemoryWarning,
    BackgroundBudgetExceeded,
}

impl LifecycleMessage {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::WillSuspend => "will-suspend",
            Self::DidResume => "did-resume",
            Self::MemoryWarning => "memory-warning",
            Self::BackgroundBudgetExceeded => "background-budget-exceeded",
        }
    }

    pub fn expects_ack(self) -> bool {
        matches!(self, Self::WillSuspend | Self::MemoryWarning)
    }

    pub fn ack_timeout(self) -> Duration {
        match self {
            Self::WillSuspend => Duration::from_secs(2),
            Self::MemoryWarning => Duration::from_millis(500),
            _ => Duration::from_millis(0),
        }
    }

    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "will-suspend" => Some(Self::WillSuspend),
            "did-resume" => Some(Self::DidResume),
            "memory-warning" => Some(Self::MemoryWarning),
            "background-budget-exceeded" => Some(Self::BackgroundBudgetExceeded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ack {
    Accepted,
    Timeout,
}

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("no SDK client for the app (socket missing)")]
    NoClient,
    #[error("socket error: {0}")]
    Socket(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// App-facing notifications (D3 section 4).
pub trait LifecycleNotifier {
    fn notify(&self, id: &AppId, message: LifecycleMessage) -> Result<Ack, NotifyError>;
}

/// Socket file name for an app, derived from the same unit name as its scope
/// (`aether-app-<id>-<hash>.sock`).
pub fn socket_name(id: &AppId) -> String {
    let unit = crate::identity::scope_unit_name(id);
    let stem = unit
        .strip_prefix(SCOPE_PREFIX)
        .and_then(|s| s.strip_suffix(".scope"))
        .unwrap_or(&unit);
    format!("{SCOPE_PREFIX}{stem}.sock")
}

/// Notifier that connects to `$XDG_RUNTIME_DIR/aether/lifecycle/`.
#[derive(Debug, Clone)]
pub struct SocketNotifier {
    dir: PathBuf,
}

impl SocketNotifier {
    pub fn system() -> Self {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/1000"))
            .join("aether/lifecycle");
        Self { dir }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn socket_path(&self, id: &AppId) -> PathBuf {
        self.dir.join(socket_name(id))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WireRequest {
    #[serde(rename = "type")]
    kind: String,
    app: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WireAck {
    ack: String,
    app: String,
}

impl LifecycleNotifier for SocketNotifier {
    fn notify(&self, id: &AppId, message: LifecycleMessage) -> Result<Ack, NotifyError> {
        let path = self.socket_path(id);
        let mut stream = match UnixStream::connect(&path) {
            Ok(stream) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(NotifyError::NoClient),
            Err(e) => return Err(NotifyError::Socket(e)),
        };

        let timeout = message.ack_timeout();
        if timeout > Duration::ZERO {
            stream.set_read_timeout(Some(timeout))?;
        }
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;

        let request = WireRequest {
            kind: message.wire_name().to_string(),
            app: id.as_str().to_string(),
        };
        let mut line = serde_json::to_string(&request)
            .map_err(|e| NotifyError::Protocol(e.to_string()))?;
        line.push('\n');
        if let Err(error) = stream.write_all(line.as_bytes()).and_then(|_| stream.flush()) {
            // A client that closed between connect and write is as unresponsive
            // as one that never answers; do not surface a transient reset.
            if is_peer_gone(&error) {
                return Ok(if message.expects_ack() {
                    Ack::Timeout
                } else {
                    Ack::Accepted
                });
            }
            return Err(NotifyError::Socket(error));
        }

        if !message.expects_ack() {
            return Ok(Ack::Accepted);
        }

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        match reader.read_line(&mut response) {
            Ok(0) => Ok(Ack::Timeout),
            Ok(_) => {
                let ack: WireAck = serde_json::from_str(response.trim())
                    .map_err(|e| NotifyError::Protocol(e.to_string()))?;
                if ack.ack == "accepted" {
                    Ok(Ack::Accepted)
                } else {
                    Ok(Ack::Timeout)
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(Ack::Timeout)
            }
            // The peer reset or closed while we waited for the ack: treat it as
            // unresponsive (the app is suspended anyway).
            Err(e) if is_peer_gone(&e) => Ok(Ack::Timeout),
            Err(e) => Err(NotifyError::Socket(e)),
        }
    }
}

/// True when the peer vanished (closed/reset/aborted the connection).
fn is_peer_gone(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::NotConnected
    )
}

/// Behavior of the SDK test app when the daemon sends `WillSuspend`.
pub trait SuspendHandler: Send + Sync {
    fn on_will_suspend(&self, id: &AppId);
}

/// Minimal SDK-side server used by the test app and by integration tests.
pub struct LifecycleServer {
    listener: UnixListener,
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl LifecycleServer {
    /// Bind the SDK socket for `id` under `dir`.
    pub fn bind(dir: impl AsRef<Path>, id: &AppId) -> std::io::Result<Self> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir)?;
        let path = dir.join(socket_name(id));
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// Handle at most one connection. Returns true when a connection was
    /// served. Used by tests and by `--once` mode of the test app.
    pub fn serve_once(&self, app: &AppId, handler: &dyn SuspendHandler) -> bool {
        let (stream, _) = match self.listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return false,
            Err(_) => return false,
        };
        let _ = stream.set_nonblocking(false);
        let mut writer = match stream.try_clone() {
            Ok(clone) => clone,
            Err(_) => return false,
        };
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Ok(request) = serde_json::from_str::<WireRequest>(&line) else {
                continue;
            };
            let Some(message) = LifecycleMessage::from_wire(&request.kind) else {
                continue;
            };
            if message == LifecycleMessage::WillSuspend {
                handler.on_will_suspend(app);
                let ack = WireAck {
                    ack: "accepted".to_string(),
                    app: app.as_str().to_string(),
                };
                if let Ok(mut text) = serde_json::to_string(&ack) {
                    text.push('\n');
                    let _ = writer.write_all(text.as_bytes());
                    let _ = writer.flush();
                }
            }
            if request.kind == LifecycleMessage::WillSuspend.wire_name() {
                break;
            }
        }
        true
    }

    /// Serve connections on a background thread until the stop flag is set.
    pub fn spawn(self, app: AppId, handler: Arc<dyn SuspendHandler>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            while !self.stop.load(Ordering::Relaxed) {
                if !self.serve_once(&app, handler.as_ref()) {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for LifecycleServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn temp_dir(tag: &str) -> PathBuf {
        // Short path: AF_UNIX sun_path is limited to 104 bytes on macOS.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = PathBuf::from("/tmp").join(format!(
            "aet-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn id() -> AppId {
        AppId::new("org.example.Mail").unwrap()
    }

    struct Counter(AtomicUsize);

    impl SuspendHandler for Counter {
        fn on_will_suspend(&self, _id: &AppId) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn socket_name_is_stable_and_unique() {
        let a = AppId::new("org.example.a_b").unwrap();
        let b = AppId::new("org.example.a-b").unwrap();
        assert_eq!(socket_name(&id()), socket_name(&id()));
        assert!(socket_name(&id()).ends_with(".sock"));
        assert_ne!(socket_name(&a), socket_name(&b));
    }

    #[test]
    fn will_suspend_round_trip_is_acknowledged() {
        let dir = temp_dir("ack");
        let server = LifecycleServer::bind(&dir, &id()).unwrap();
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        let stop = server.stop_handle();
        let handle = server.spawn(id(), counter.clone());
        // Give the thread a moment to start; serve_once handles WouldBlock.
        std::thread::sleep(Duration::from_millis(20));

        let notifier = SocketNotifier::at(&dir);
        let ack = notifier
            .notify(&id(), LifecycleMessage::WillSuspend)
            .expect("notify");
        assert_eq!(ack, Ack::Accepted);
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);

        stop.store(true, Ordering::SeqCst);
        let _ = handle.join();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_client_is_reported() {
        let dir = temp_dir("missing");
        let notifier = SocketNotifier::at(&dir);
        assert!(matches!(
            notifier.notify(&id(), LifecycleMessage::DidResume),
            Err(NotifyError::NoClient)
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unresponsive_client_times_out() {
        let dir = temp_dir("timeout");
        let path = dir.join(socket_name(&id()));
        let listener = UnixListener::bind(&path).unwrap();
        // Accept and keep the stream open without answering until the notifier
        // has hit its read timeout. Releasing explicitly avoids the old race
        // where the peer closed at the same instant as the 500 ms timeout and
        // the read returned ECONNRESET instead of a timeout.
        let (release, released) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ = released.recv_timeout(Duration::from_secs(5));
            drop(stream);
        });
        let notifier = SocketNotifier::at(&dir);
        let started = std::time::Instant::now();
        let ack = notifier
            .notify(&id(), LifecycleMessage::MemoryWarning)
            .expect("an unresponsive client must be reported as a timeout, not an error");
        assert_eq!(ack, Ack::Timeout);
        assert!(started.elapsed() >= Duration::from_millis(400));
        let _ = release.send(());
        let _ = server.join();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn peer_reset_is_reported_as_timeout() {
        // Regression for the CI flake: the peer may close or reset while the
        // notifier is writing or waiting for the ack. Both must surface as
        // Ack::Timeout, never as an unexpected Socket error.
        let dir = temp_dir("reset");
        let path = dir.join(socket_name(&id()));
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            drop(stream);
        });
        let notifier = SocketNotifier::at(&dir);
        let ack = notifier
            .notify(&id(), LifecycleMessage::MemoryWarning)
            .expect("reset must be tolerated");
        assert_eq!(ack, Ack::Timeout);
        let _ = server.join();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn informational_messages_do_not_wait_for_ack() {
        let dir = temp_dir("info");
        let path = dir.join(socket_name(&id()));
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = String::new();
            let mut reader = BufReader::new(&mut stream);
            let _ = reader.read_line(&mut buf);
            assert!(buf.contains("did-resume"));
        });
        let notifier = SocketNotifier::at(&dir);
        assert_eq!(
            notifier.notify(&id(), LifecycleMessage::DidResume).unwrap(),
            Ack::Accepted
        );
        let _ = server.join();
        let _ = fs::remove_dir_all(dir);
    }
}
