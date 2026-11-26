//! Interactive shell host (G-NH2 ↔ P4, S3).
//!
//! [`crate::nohang`] is the policy; this module is the loop that actually runs
//! the shell. It
//!
//! 1. follows the compositor journal as it grows ([`Journal`], so tests inject a
//!    [`MemoryJournal`] and the device reads a [`FileJournal`]),
//! 2. feeds `AETHER_COMPOSITOR_APP_STATE` decisions to
//!    [`ShellController::observe_health`] and `AETHER_COMPOSITOR_SHELL_WATCHDOG`
//!    markers to [`ShellController::observe_watchdog`],
//! 3. ticks the dialog clock so a waived app is re-offered after the wait grace
//!    ([`ShellController::tick_dialog`]),
//! 4. presents the Slint surfaces and, when asked, captures a frame as evidence,
//! 5. publishes every marker the controller produced (Wait / Force Quit,
//!    recovery, `AETHER_SHELL_NOHANG_TERMINATE`, safe mode) to the shell's own
//!    journal, and
//! 6. owns the `--start-begin` / `--start-confirm` ledger handshake: the start is
//!    counted before the surfaces exist and confirmed once the first frame is
//!    presented, so a crash inside the host counts as a failed start.
//!
//! The loop never blocks the render path: journal reads are incremental
//! (`FileJournal` keeps a byte offset) and nothing here waits on a client.
//!
//! ```text
//! aether-shell --interactive --journal /var/log/aether-compositor.log
//! aether-shell --interactive --journal fixture.log --frames 4 --frames-out /tmp/shots
//! ```

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;

use crate::app::ShellController;
use crate::nohang::{self, DialogAction, DialogEvent, StartOutcome, WatchdogReport};

/// Journal the session launcher points the shell at.
pub const DEFAULT_JOURNAL: &str = "/var/log/aether-compositor.log";
/// Journal poll / dialog tick interval.
pub const DEFAULT_STEP: Duration = Duration::from_millis(250);
/// Dialog clock interval (the wait grace is 15 s, so 1 s is fine).
pub const DEFAULT_DIALOG_TICK: Duration = Duration::from_secs(1);
/// Size evidence frames are captured at (logical pixels).
pub const CAPTURE_SIZE: (u32, u32) = (1440, 900);

/// A source of compositor journal text. `poll` returns only text that arrived
/// since the previous call, and only whole lines.
pub trait Journal {
    fn poll(&mut self) -> Result<String, String>;
}

/// Follows a journal file by byte offset. A truncated or rotated file restarts
/// from the beginning; a missing file is simply "nothing new yet".
#[derive(Debug, Clone)]
pub struct FileJournal {
    path: PathBuf,
    offset: u64,
    pending: String,
}

impl FileJournal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            pending: String::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Journal for FileJournal {
    fn poll(&mut self) -> Result<String, String> {
        match std::fs::metadata(&self.path) {
            Ok(metadata) => {
                if metadata.len() < self.offset {
                    // Rotated: read the new file from the start.
                    self.offset = 0;
                    self.pending.clear();
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => {
                return Err(format!("cannot stat {}: {error}", self.path.display()));
            }
        }
        let data = match std::fs::read(&self.path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(error) => {
                return Err(format!("cannot read {}: {error}", self.path.display()));
            }
        };
        let start = (self.offset as usize).min(data.len());
        self.offset = data.len() as u64;
        self.pending
            .push_str(&String::from_utf8_lossy(&data[start..]));
        // Hand out complete lines only; a half-written marker stays buffered.
        Ok(match self.pending.rfind('\n') {
            Some(end) => self.pending.drain(..=end).collect(),
            None => String::new(),
        })
    }
}

/// Journal the tests (and fixtures) feed directly. Clones share one buffer, so
/// a test keeps a writing handle while the host owns the reading end.
#[derive(Debug, Default, Clone)]
pub struct MemoryJournal {
    buffer: std::rc::Rc<std::cell::RefCell<String>>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue journal text; partial lines are allowed and joined with the next
    /// push, like a real file.
    pub fn push(&self, text: &str) {
        self.buffer.borrow_mut().push_str(text);
    }

    /// A second handle on the same buffer.
    pub fn handle(&self) -> MemoryJournal {
        self.clone()
    }
}

impl Journal for MemoryJournal {
    fn poll(&mut self) -> Result<String, String> {
        Ok(std::mem::take(&mut *self.buffer.borrow_mut()))
    }
}

/// Loop timing and evidence capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    pub step: Duration,
    pub dialog_tick: Duration,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            step: DEFAULT_STEP,
            dialog_tick: DEFAULT_DIALOG_TICK,
        }
    }
}

/// What one [`ShellHost::run_for`] produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostRun {
    pub frames: u64,
    pub polls: u64,
    /// Dialogs raised, including Wait re-offers.
    pub dialogs: u32,
    /// Wait / Force Quit actions taken from the dialog.
    pub actions: u32,
    /// Clients whose `AETHER_SHELL_NOHANG_TERMINATE` marker was published.
    pub terminations: Vec<u64>,
    /// The session reached "surfaces presented" and the start was confirmed.
    pub ready: bool,
}

impl HostRun {
    pub fn summary_line(&self) -> String {
        format!(
            "AETHER_SHELL_HOST_DONE: frames={} polls={} dialogs={} actions={} terminations={} ready={}",
            self.frames,
            self.polls,
            self.dialogs,
            self.actions,
            self.terminations.len(),
            self.ready
        )
    }
}

/// The running shell: journal in, Slint surfaces out.
pub struct ShellHost {
    controller: Rc<ShellController>,
    journal: Box<dyn Journal>,
    config: HostConfig,
    /// `begin_start` ran (the ledger counts this process).
    started: bool,
    ready: bool,
    /// Everything the controller logged before this index is already published.
    published: usize,
    last_tick: Option<Instant>,
    watchdog: WatchdogReport,
    outbox: Vec<String>,
    frames: u64,
    polls: u64,
    dialogs: u32,
    actions: u32,
    terminations: Vec<u64>,
    capture_error_reported: bool,
    /// Harness affordance: press this dialog button once when a dialog is open.
    auto_action: Option<DialogAction>,
}

impl ShellHost {
    pub fn new(
        controller: Rc<ShellController>,
        journal: Box<dyn Journal>,
        config: HostConfig,
    ) -> Self {
        let published = controller.log.borrow().len();
        Self {
            controller,
            journal,
            config,
            started: false,
            ready: false,
            published,
            last_tick: None,
            watchdog: WatchdogReport::default(),
            outbox: Vec::new(),
            frames: 0,
            polls: 0,
            dialogs: 0,
            actions: 0,
            terminations: Vec::new(),
            capture_error_reported: false,
            auto_action: None,
        }
    }

    pub fn controller(&self) -> &Rc<ShellController> {
        &self.controller
    }

    pub fn config(&self) -> &HostConfig {
        &self.config
    }

    /// Safer profile for a launch loop: a faster journal poll.
    pub fn with_step(mut self, step: Duration) -> Self {
        self.config.step = step;
        self
    }

    /// The ledger start: count this launch before any surface is shown and arm
    /// safe mode if the previous compositor run stalled three times. Emits
    /// `AETHER_SHELL_START` (and `AETHER_SHELL_SAFE_MODE` when it applies).
    pub fn start(&mut self, now: Instant) -> StartOutcome {
        if self.started {
            return StartOutcome {
                attempt: self.controller.startup().attempts,
                safe_mode: self.controller.safe_mode(),
                previous_failed: !self.controller.startup().confirmed,
            };
        }
        // Watchdog markers from the run that just died arm safe mode *before*
        // the start is counted (`--start-begin <journal>` does the same).
        if self.watchdog.timeouts > 0 {
            self.controller.observe_watchdog(&self.watchdog);
        }
        let outcome = self.controller.begin_session_start();
        self.started = true;
        self.last_tick = Some(now);
        let (start_line, safe_mode_line) = {
            let ledger = self.controller.startup();
            (
                ledger.start_line(&outcome),
                if ledger.safe_mode {
                    ledger.safe_mode_line()
                } else {
                    String::new()
                },
            )
        };
        self.publish_line(start_line.trim_end());
        if !safe_mode_line.is_empty() {
            self.publish_line(safe_mode_line.trim_end());
        }
        outcome
    }

    /// Press a dialog button. This is the exact call the surfaces' Wait /
    /// Force Quit callbacks make (`ShellController::dialog_action`), so a
    /// scripted press and a click are indistinguishable in the journal.
    pub fn act(&mut self, action: DialogAction, now: Instant) -> Option<u64> {
        let acted = self.controller.dialog_action(action, now);
        self.collect_controller_log();
        acted
    }

    /// Press `action` once, at the first pump where a dialog is on screen. The
    /// acceptance harness uses it to prove the terminate marker without a
    /// pointer (`aether-shell --interactive … --act force-quit`).
    pub fn set_auto_action(&mut self, action: Option<DialogAction>) {
        self.auto_action = action;
    }

    /// Read the journal, apply every marker and tick the dialog clock. Returns
    /// the marker lines that were published (also available through
    /// [`Self::drain_published`]).
    pub fn pump_once(&mut self, now: Instant) -> Result<Vec<String>, String> {
        let text = self.journal.poll()?;
        self.polls += 1;
        for line in text.lines() {
            if let Some(event) = nohang::parse_health_line(line) {
                if let DialogEvent::Shown(_) = self.controller.observe_health(event, now) {
                    self.dialogs += 1;
                }
            } else if self.watchdog.observe_line(line) {
                let armed = self.controller.observe_watchdog(&self.watchdog);
                self.publish_line(format!(
                    "AETHER_SHELL_HOST_WATCHDOG: timeouts={} restart_required={} armed_safe_mode={}",
                    self.watchdog.timeouts,
                    self.watchdog.needs_restart(),
                    armed
                ));
            }
        }
        if self
            .last_tick
            .map(|last| now.duration_since(last) >= self.config.dialog_tick)
            .unwrap_or(true)
        {
            if self.controller.tick_dialog(now).is_some() {
                self.dialogs += 1;
            }
            self.last_tick = Some(now);
        }
        // The harness's button press (a click reaches the same call).
        if let Some(action) = self.auto_action {
            let open = self.controller.nohang().is_open();
            if open {
                self.auto_action = None;
                let _ = self.act(action, now);
            }
        }
        self.collect_controller_log();
        Ok(self.drain_published())
    }

    /// Show the surfaces, pump animations and request one redraw. The first
    /// presentation confirms the ledger start (`--start-confirm`).
    pub fn present(&mut self, capture: Option<&Path>) -> Result<(), String> {
        // Clone the handle so the mutable host state is free while the surfaces
        // are borrowed for this frame.
        let controller = Rc::clone(&self.controller);
        let ui = controller.ui();
        ui.show()
            .map_err(|error| format!("cannot show the shell surfaces: {error}"))?;
        slint::platform::update_timers_and_animations();
        ui.window().request_redraw();
        self.frames += 1;
        if !self.ready {
            self.ready = true;
            controller.session_ready();
            let attempts = controller.startup().attempts;
            self.publish_line(format!("AETHER_SHELL_START_CONFIRMED: attempts={attempts}"));
            self.publish_line(format!(
                "AETHER_SHELL_HOST_READY: frames={} polls={}",
                self.frames, self.polls
            ));
        }
        if let Some(dir) = capture {
            let path = dir.join(format!("shell-host-{:04}.png", self.frames));
            if let Err(error) =
                aether_ui::render::render_png(ui, CAPTURE_SIZE.0, CAPTURE_SIZE.1, &path)
            {
                // A real display backend cannot be captured; that is not fatal.
                if !self.capture_error_reported {
                    self.capture_error_reported = true;
                    self.publish_line(format!("AETHER_SHELL_HOST_CAPTURE_SKIPPED: {error}"));
                }
            }
        }
        self.collect_controller_log();
        Ok(())
    }

    /// The host loop. `frames == 0` runs until the process is stopped (the
    /// service manager's job on the image). `publish` receives each batch of
    /// marker lines as it is produced.
    pub fn run_for<F>(
        &mut self,
        frames: u64,
        capture: Option<&Path>,
        mut publish: F,
    ) -> Result<HostRun, String>
    where
        F: FnMut(&[String]),
    {
        if !self.started {
            self.start(Instant::now());
            publish(&self.drain_published());
        }
        let mut presented = 0;
        loop {
            if frames != 0 && presented >= frames {
                break;
            }
            let batch = self.pump_once(Instant::now())?;
            if !batch.is_empty() {
                publish(&batch);
            }
            self.present(capture)?;
            let batch = self.drain_published();
            if !batch.is_empty() {
                publish(&batch);
            }
            presented += 1;
            if frames == 0 || presented < frames {
                std::thread::sleep(self.config.step);
            }
        }
        Ok(self.run())
    }

    pub fn run(&self) -> HostRun {
        HostRun {
            frames: self.frames,
            polls: self.polls,
            dialogs: self.dialogs,
            actions: self.actions,
            terminations: self.terminations.clone(),
            ready: self.ready,
        }
    }

    /// Clients whose Force Quit the host must end (the compositor's xdg
    /// unresponsive error, or the service manager consuming
    /// `AETHER_SHELL_NOHANG_TERMINATE`). Draining clears the list.
    pub fn take_terminations(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.terminations)
    }

    /// Take the markers produced since the last drain.
    pub fn drain_published(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    /// Every controller log line not published yet: markers, terminate intents
    /// and the safe-mode reset all reach the shell's journal from here.
    fn collect_controller_log(&mut self) {
        let lines: Vec<String> = {
            let log = self.controller.log.borrow();
            log.iter()
                .skip(self.published)
                .cloned()
                .collect::<Vec<String>>()
        };
        self.published += lines.len();
        for line in lines {
            if let Some(client) = line
                .strip_prefix("AETHER_SHELL_NOHANG_TERMINATE: client=")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok())
            {
                if !self.terminations.contains(&client) {
                    self.terminations.push(client);
                }
            }
            if line.starts_with("AETHER_SHELL_NOHANG_ACTION:") {
                self.actions += 1;
            }
            self.publish_line(line);
        }
    }

    fn publish_line(&mut self, line: impl Into<String>) {
        let line = line.into();
        if !line.is_empty() {
            self.outbox.push(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aether-shell-host-{tag}-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn the_file_journal_hands_out_only_whole_lines() {
        let path = temp_path("file");
        std::fs::write(
            &path,
            "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100\nhalf",
        )
        .unwrap();
        let mut journal = FileJournal::new(&path);

        let first = journal.poll().unwrap();
        assert_eq!(
            first,
            "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100\n"
        );
        // The partial line waits for its newline.
        assert_eq!(journal.poll().unwrap(), "");

        // Append the rest plus another marker: only the new text is returned.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str(" line\nAETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout timeouts=1\n");
        std::fs::write(&path, text).unwrap();
        let second = journal.poll().unwrap();
        assert!(second.starts_with("half line\n"), "{second}");
        assert!(second.contains("SHELL_WATCHDOG: state=timeout"), "{second}");

        // A missing file is empty, not an error; a rotated one restarts.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(journal.poll().unwrap(), "");
        std::fs::write(&path, "fresh\n").unwrap();
        assert_eq!(journal.poll().unwrap(), "fresh\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_truncated_journal_is_read_from_the_start() {
        let path = temp_path("rotate");
        std::fs::write(&path, "aaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbbbbbbbb\n").unwrap();
        let mut journal = FileJournal::new(&path);
        assert!(journal.poll().unwrap().contains("aaaaaaaa"));
        // Rotation: the new file is shorter than the old offset.
        std::fs::write(&path, "new\n").unwrap();
        assert_eq!(journal.poll().unwrap(), "new\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_memory_journal_drains_its_buffer() {
        let mut journal = MemoryJournal::new();
        journal.push("one\n");
        journal.push("two\n");
        assert_eq!(journal.poll().unwrap(), "one\ntwo\n");
        assert_eq!(journal.poll().unwrap(), "");
    }

    #[test]
    fn health_decisions_are_parsed_line_by_line() {
        assert_eq!(
            nohang::parse_health_line(
                "AETHER_COMPOSITOR_APP_STATE: client=7 state=not_responding after_ms=5100"
            ),
            Some(nohang::HangEvent::NotResponding {
                client: 7,
                after_ms: 5100
            })
        );
        assert_eq!(
            nohang::parse_health_line(
                "AETHER_COMPOSITOR_APP_STATE: client=7 state=responsive recovered=true"
            ),
            Some(nohang::HangEvent::Recovered { client: 7 })
        );
        assert_eq!(
            nohang::parse_health_line("AETHER_COMPOSITOR_APP_STATE: client=7 state=ping_pending"),
            None
        );
    }

    #[test]
    fn a_watchdog_report_grows_marker_by_marker() {
        let mut report = WatchdogReport::default();
        assert!(!report.observe_line("AETHER_COMPOSITOR_PING: client=7 serial=1"));
        assert!(report.observe_line(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6200 timeout_ms=5000 timeouts=2"
        ));
        assert!(report.needs_restart());
        assert_eq!(report.timeouts, 2);
        assert_eq!(report.last_idle_ms, Some(6200));
        assert!(report
            .observe_line("AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true timeouts=2"));
        assert!(!report.needs_restart());
        assert!(report.recovered);
        assert_eq!(report, WatchdogReport::scan(
            "AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=6200 timeout_ms=5000 timeouts=2\n\
             AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true timeouts=2\n"
        ));
    }

    #[test]
    fn the_host_run_summary_is_harness_readable() {
        let run = HostRun {
            frames: 4,
            polls: 4,
            dialogs: 1,
            actions: 1,
            terminations: vec![7],
            ready: true,
        };
        assert_eq!(
            run.summary_line(),
            "AETHER_SHELL_HOST_DONE: frames=4 polls=4 dialogs=1 actions=1 terminations=1 ready=true"
        );
    }
}
