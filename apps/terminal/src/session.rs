//! PTY session (P4-T15, DA-AC-07).
//!
//! Spawns the user's shell in a pseudo terminal with `forkpty(3)`, exposes
//! resize and read helpers, and hands raw bytes to [`crate::vt::Vt`]. The
//! process side is unix only; on other platforms [`PtySession::spawn`] returns
//! [`SessionError::Unsupported`] so the rest of the crate (and its tests) still
//! builds and passes.
//!
//! The interactive host that pumps [`PtySession::read_loop`] into the Slint
//! event loop arrives with the compositor's layer-shell work (P3-T02); this
//! module is the reusable half of that wiring.

use std::path::{Path, PathBuf};

/// Program plus arguments to run inside the PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    program: PathBuf,
    args: Vec<String>,
}

impl Command {
    pub fn new(program: impl Into<PathBuf>) -> Command {
        Command {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Command {
        self.args.push(arg.into());
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Program first, then every argument: exactly what `execvp` receives.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(self.program.to_string_lossy().to_string());
        argv.extend(self.args.iter().cloned());
        argv
    }

    /// The shell from `$SHELL`, or `/bin/sh` when unset.
    pub fn shell_from(shell: Option<&str>) -> Command {
        let shell = shell
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("/bin/sh");
        Command::new(shell)
    }

    pub fn default_shell() -> Command {
        let shell = std::env::var("SHELL").ok();
        Command::shell_from(shell.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// PTY support needs unix `forkpty`; other platforms get this error.
    Unsupported(String),
    /// The child could not be started.
    Spawn(String),
    /// Reading or writing the master side failed.
    Io(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Unsupported(message) => write!(f, "{message}"),
            SessionError::Spawn(message) => write!(f, "cannot start shell: {message}"),
            SessionError::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SessionError {}

#[cfg(unix)]
mod unix {
    use super::{Command, SessionError};
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};

    /// One running shell attached to a pseudo terminal.
    #[derive(Debug)]
    pub struct PtySession {
        master: c_int,
        child: libc::pid_t,
        cols: u16,
        rows: u16,
        closed: bool,
    }

    impl PtySession {
        /// Start `command` with a terminal `cols` x `rows` cells big.
        pub fn spawn(command: &Command, cols: u16, rows: u16) -> Result<PtySession, SessionError> {
            let argv: Vec<CString> = command
                .argv()
                .iter()
                .map(|argument| {
                    CString::new(argument.as_str())
                        .map_err(|_| SessionError::Spawn("arguments must not contain NUL".into()))
                })
                .collect::<Result<_, _>>()?;
            let mut pointers: Vec<*const c_char> =
                argv.iter().map(|argument| argument.as_ptr()).collect();
            pointers.push(std::ptr::null());

            let mut winsize = libc::winsize {
                ws_row: rows.max(1),
                ws_col: cols.max(1),
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let mut master: c_int = -1;
            // SAFETY: forkpty is called with valid pointers and the child path
            // only performs async-signal-safe execvp/_exit calls.
            let child = unsafe {
                libc::forkpty(
                    &mut master,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut winsize,
                )
            };
            if child < 0 {
                return Err(SessionError::Spawn(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            if child == 0 {
                unsafe {
                    libc::execvp(pointers[0], pointers.as_ptr());
                    libc::_exit(127);
                }
            }
            Ok(PtySession {
                master,
                child,
                cols,
                rows,
                closed: false,
            })
        }

        pub fn pid(&self) -> i32 {
            self.child
        }

        pub fn cols(&self) -> u16 {
            self.cols
        }

        pub fn rows(&self) -> u16 {
            self.rows
        }

        /// Write raw bytes (a command line plus `\n`, keys, paste).
        pub fn write(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
            if self.closed {
                return Err(SessionError::Io("session is closed".into()));
            }
            let mut written = 0;
            while written < bytes.len() {
                let count = unsafe {
                    libc::write(
                        self.master,
                        bytes[written..].as_ptr() as *const c_void,
                        bytes.len() - written,
                    )
                };
                if count < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(SessionError::Io(format!("pty write failed: {error}")));
                }
                written += count as usize;
            }
            Ok(())
        }

        /// Wait up to `timeout_ms` for output and return what is available.
        /// An empty vector means "nothing yet" or "the child exited".
        pub fn read_available(&mut self, timeout_ms: i32) -> Result<Vec<u8>, SessionError> {
            if self.closed {
                return Ok(Vec::new());
            }
            let mut pollfd = libc::pollfd {
                fd: self.master,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    return Ok(Vec::new());
                }
                return Err(SessionError::Io(format!("pty poll failed: {error}")));
            }
            if ready == 0 {
                return Ok(Vec::new());
            }
            let mut buffer = [0u8; 8192];
            let count = unsafe {
                libc::read(
                    self.master,
                    buffer.as_mut_ptr() as *mut c_void,
                    buffer.len(),
                )
            };
            if count <= 0 {
                // EIO is how a closed PTY reports that the child exited.
                return Ok(Vec::new());
            }
            Ok(buffer[..count as usize].to_vec())
        }

        /// Read until the child closes the terminal or `idle_ms` passes with
        /// no output, handing each chunk to `sink`. Returns the byte count.
        pub fn read_loop(
            &mut self,
            idle_ms: i32,
            sink: &mut dyn FnMut(&[u8]),
        ) -> Result<usize, SessionError> {
            let mut total = 0;
            loop {
                let chunk = self.read_available(idle_ms)?;
                if chunk.is_empty() {
                    return Ok(total);
                }
                total += chunk.len();
                sink(&chunk);
            }
        }

        /// Update the terminal size; the shell receives `SIGWINCH`.
        pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), SessionError> {
            if self.closed {
                return Err(SessionError::Io("session is closed".into()));
            }
            self.cols = cols.max(1);
            self.rows = rows.max(1);
            let winsize = libc::winsize {
                ws_row: self.rows,
                ws_col: self.cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let result = unsafe {
                libc::ioctl(
                    self.master,
                    libc::TIOCSWINSZ as libc::c_ulong,
                    &winsize as *const libc::winsize as *mut c_void,
                )
            };
            if result < 0 {
                return Err(SessionError::Io(format!(
                    "resize failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            unsafe {
                libc::kill(self.child, libc::SIGWINCH);
            }
            Ok(())
        }

        /// Reap the child if it already exited. Marks the session closed so
        /// `Drop` never signals a pid that the OS may have recycled.
        pub fn wait(&mut self) -> Option<i32> {
            if self.closed {
                return None;
            }
            let mut status: c_int = 0;
            let result = unsafe { libc::waitpid(self.child, &mut status, libc::WNOHANG) };
            if result == self.child {
                self.closed = true;
                let _ = unsafe { libc::close(self.master) };
                Some(status)
            } else {
                None
            }
        }
    }

    impl Drop for PtySession {
        fn drop(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            unsafe {
                libc::kill(self.child, libc::SIGHUP);
                libc::kill(self.child, libc::SIGKILL);
                // Reap without blocking forever if the child ignores signals.
                let mut status: c_int = 0;
                for _ in 0..50 {
                    if libc::waitpid(self.child, &mut status, libc::WNOHANG) != 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                libc::close(self.master);
            }
        }
    }
}

#[cfg(unix)]
pub use unix::PtySession;

#[cfg(not(unix))]
pub use other::PtySession;

#[cfg(not(unix))]
mod other {
    use super::{Command, SessionError};

    /// Non-unix placeholder that fails every operation clearly.
    #[derive(Debug)]
    pub struct PtySession {
        cols: u16,
        rows: u16,
    }

    impl PtySession {
        pub fn spawn(_command: &Command, cols: u16, rows: u16) -> Result<PtySession, SessionError> {
            Err(SessionError::Unsupported(format!(
                "PTY sessions need unix forkpty; {cols}x{rows} terminal unavailable on this platform"
            )))
        }

        fn unsupported() -> SessionError {
            SessionError::Unsupported("PTY sessions need unix forkpty".into())
        }

        pub fn pid(&self) -> i32 {
            0
        }

        pub fn cols(&self) -> u16 {
            self.cols
        }

        pub fn rows(&self) -> u16 {
            self.rows
        }

        pub fn write(&mut self, _bytes: &[u8]) -> Result<(), SessionError> {
            Err(Self::unsupported())
        }

        pub fn read_available(&mut self, _timeout_ms: i32) -> Result<Vec<u8>, SessionError> {
            Err(Self::unsupported())
        }

        pub fn read_loop(
            &mut self,
            _idle_ms: i32,
            _sink: &mut dyn FnMut(&[u8]),
        ) -> Result<usize, SessionError> {
            Err(Self::unsupported())
        }

        pub fn resize(&mut self, _cols: u16, _rows: u16) -> Result<(), SessionError> {
            Err(Self::unsupported())
        }

        pub fn wait(&mut self) -> Option<i32> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_is_program_then_arguments() {
        let command = Command::new("/bin/sh").arg("-c").arg("echo hi");
        assert_eq!(command.argv(), vec!["/bin/sh", "-c", "echo hi"]);
        assert_eq!(command.program(), Path::new("/bin/sh"));
        let bare = Command::new("aether-terminal");
        assert_eq!(bare.argv(), vec!["aether-terminal"]);
    }

    #[test]
    fn shell_from_prefers_the_environment_and_falls_back() {
        assert_eq!(
            Command::shell_from(Some("/usr/bin/fish")).argv(),
            vec!["/usr/bin/fish"]
        );
        assert_eq!(Command::shell_from(Some("   ")).argv(), vec!["/bin/sh"]);
        assert_eq!(Command::shell_from(None).argv(), vec!["/bin/sh"]);
    }

    #[cfg(unix)]
    #[test]
    fn unix_pty_runs_a_command_and_returns_its_output() {
        let mut session =
            match PtySession::spawn(&Command::new("/bin/echo").arg("aether-pty"), 80, 24) {
                Ok(session) => session,
                // A locked-down CI sandbox without /dev/ptmx should not fail the
                // suite; the parser tests cover the rest.
                Err(error) => {
                    eprintln!("skipping PTY spawn test: {error}");
                    return;
                }
            };
        assert!(session.pid() > 0);
        assert_eq!((session.cols(), session.rows()), (80, 24));
        let mut output = Vec::new();
        session
            .read_loop(2000, &mut |chunk| output.extend_from_slice(chunk))
            .expect("read the echo output");
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("aether-pty"), "got {text:?}");
        assert!(text.ends_with('\n') || text.ends_with("\r\n"));
        // The exit may land a moment after the last byte was read.
        let mut status = None;
        for _ in 0..100 {
            if let Some(code) = session.wait() {
                status = Some(code);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(status.is_some(), "the child exits after echo");
    }

    #[cfg(unix)]
    #[test]
    fn unix_pty_resize_and_write_are_accepted() {
        let mut session = match PtySession::spawn(&Command::new("/bin/cat"), 80, 24) {
            Ok(session) => session,
            Err(error) => {
                eprintln!("skipping PTY resize test: {error}");
                return;
            }
        };
        session.resize(120, 40).expect("resize the pty");
        assert_eq!((session.cols(), session.rows()), (120, 40));
        session.write(b"hello\n").expect("write into the pty");
        let mut output = Vec::new();
        session
            .read_loop(500, &mut |chunk| output.extend_from_slice(chunk))
            .expect("read the echo");
        assert!(String::from_utf8_lossy(&output).contains("hello"));
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_pty_reports_a_clear_error() {
        let error = PtySession::spawn(&Command::new("/bin/sh"), 80, 24).unwrap_err();
        assert!(matches!(error, SessionError::Unsupported(_)));
        assert!(error.to_string().contains("forkpty"));
    }
}
