//! AetherOS Terminal (P4-T15, DA-AC-07).
//!
//! * [`vt`] — the VT100/ANSI screen model: printing, cursor movement, erase,
//!   SGR colours and bounded scrollback. It is unit tested on its own.
//! * [`session`] — the real PTY: `forkpty` + `execvp` on unix, a clear
//!   [`SessionError::Unsupported`] elsewhere, plus argv building and resize.
//! * [`TerminalController`] — tabs, the prompt line and the bridge from the
//!   screen model to the Slint grid. It also runs a small built-in shell
//!   (`help`, `ls`, `cd`, `echo`, `pwd`, `uname`, `whoami`, `clear`) so the
//!   window works without a display host process.
//!
//! The interactive host that pumps [`PtySession::read_loop`] on a timer lands
//! with the compositor's layer-shell work; [`TerminalController::spawn_pty`]
//! and [`TerminalController::pump`] are the halves it will call.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

pub mod session;
pub mod vt;

pub use session::{Command, ExitStatus, PtySession, SessionError};
pub use vt::{AnsiColor, Cell, Vt, DEFAULT_COLS, DEFAULT_ROWS, MAX_SCROLLBACK};

slint::include_modules!();

/// One tab: its own screen, working directory and optional live PTY.
struct Tab {
    title: String,
    vt: Vt,
    cwd: PathBuf,
    shell: String,
    pty: Option<PtySession>,
    pty_error: Option<String>,
}

struct State {
    tabs: Vec<Tab>,
    active: usize,
    prompt: String,
    note: String,
}

/// Owns the tabs and mirrors the active screen into the window.
pub struct TerminalController {
    ui: TerminalApp,
    state: RefCell<State>,
}

impl TerminalController {
    pub fn new(ui: TerminalApp) -> Rc<TerminalController> {
        let shell = Command::default_shell();
        let shell_name = shell
            .program()
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "sh".to_string());
        let mut tab = Tab::new(shell_name.clone(), shell.program().to_path_buf());
        tab.vt.feed_str(&format!(
            "\x1b[1mAetherOS Terminal\x1b[0m \x1b[2m0.1.0-p4 · {} · UTF-8\x1b[0m\r\n\r\n",
            shell_name
        ));
        let controller = Rc::new(TerminalController {
            ui,
            state: RefCell::new(State {
                tabs: vec![tab],
                active: 0,
                prompt: String::new(),
                note: "Ready".to_string(),
            }),
        });
        controller.wire();
        controller.sync();
        controller
    }

    /// Demo transcript for the screenshot renderer: two tabs, a `git status`
    /// run and a coloured test run, all fed through the real parser.
    pub fn with_demo(ui: TerminalApp) -> Rc<TerminalController> {
        let controller = TerminalController::new(ui);
        {
            let mut state = controller.state.borrow_mut();
            let cwd =
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/aether".to_string()))
                    .join("Projects/aetheros");
            state.tabs[0].cwd = cwd.clone();
            state.tabs[0].title = "zsh".to_string();
            state.tabs[0].shell = "/bin/zsh".to_string();
            state.tabs[0].vt.feed_str(DEMO_TRANSCRIPT);
            state.note = "Local shell · no PTY host attached in render mode".to_string();

            let mut second = Tab::new("build".to_string(), PathBuf::from("/bin/sh"));
            second.cwd = cwd;
            second.vt.feed_str(
                "\x1b[1mBuild log\x1b[0m \x1b[2m· aether-terminal\x1b[0m\r\n\r\n\
                 \x1b[32m✓\x1b[0m cargo test -p aether-terminal\r\n\
                 \x1b[32m✓\x1b[0m cargo test -p aether-text\r\n\
                 \x1b[32m✓\x1b[0m cargo test -p aether-calculator\r\n\
                 \x1b[33m・\x1b[0m cargo run -p aether-terminal -- --render docs\r\n",
            );
            state.tabs.push(second);
        }
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &TerminalApp {
        &self.ui
    }

    pub fn active_tab(&self) -> usize {
        self.state.borrow().active
    }

    pub fn tab_titles(&self) -> Vec<String> {
        self.state
            .borrow()
            .tabs
            .iter()
            .map(|tab| tab.title.clone())
            .collect()
    }

    /// Visible screen of the active tab, as text.
    pub fn screen_text(&self) -> String {
        let state = self.state.borrow();
        state.tabs[state.active].vt.screen_text()
    }

    /// Text for the host clipboard: the visible screen with trailing blanks
    /// trimmed from every line (DA-AC-07 copy).
    pub fn copy_screen(&self) -> String {
        trim_screen(&self.screen_text())
    }

    /// Paste text into the active tab. A terminal line break becomes a space so
    /// a pasted block cannot execute commands by accident. Returns `true` when
    /// the text went to the attached PTY, `false` when it was appended to the
    /// prompt line instead.
    pub fn paste(&self, text: &str) -> bool {
        let clean = sanitise_paste(text);
        if clean.is_empty() {
            return false;
        }
        let mut state = self.state.borrow_mut();
        let active = state.active;
        if let Some(session) = state.tabs[active].pty.as_mut() {
            if session.write(clean.as_bytes()).is_ok() {
                state.note = format!("pasted {} bytes into the shell", clean.len());
                return true;
            }
        }
        state.prompt.push_str(&clean);
        state.note = format!("pasted {} characters into the prompt", clean.len());
        drop(state);
        self.sync();
        false
    }

    pub fn scrollback_len(&self) -> usize {
        let state = self.state.borrow();
        state.tabs[state.active].vt.scrollback_len()
    }

    pub fn viewport(&self) -> (usize, usize) {
        let state = self.state.borrow();
        let vt = &state.tabs[state.active].vt;
        (vt.cols(), vt.rows())
    }

    pub fn pty_error(&self) -> Option<String> {
        let state = self.state.borrow();
        state.tabs[state.active].pty_error.clone()
    }

    /// Write raw bytes into the active screen (PTY output, paste).
    pub fn feed(&self, bytes: &[u8]) {
        {
            let mut state = self.state.borrow_mut();
            let active = state.active;
            state.tabs[active].vt.feed(bytes);
        }
        self.sync();
    }

    /// Send keystrokes (or a whole command line) to the active PTY. The
    /// interactive host forwards keyboard input here; headless tests use it to
    /// drive a real shell to its exit code.
    pub fn write_pty(&self, bytes: &[u8]) -> Result<(), SessionError> {
        let mut state = self.state.borrow_mut();
        let active = state.active;
        match state.tabs[active].pty.as_mut() {
            Some(pty) => pty.write(bytes),
            None => Err(SessionError::Io("no PTY session in this tab".into())),
        }
    }

    /// Submit the prompt line: echo it, then run the command.
    pub fn submit(&self) {
        let line = {
            let mut state = self.state.borrow_mut();
            std::mem::take(&mut state.prompt)
        };
        self.run_line(&line);
    }

    /// Run a command line through the built-in shell.
    pub fn run_line(&self, line: &str) {
        let line = line.trim().to_string();
        {
            let mut state = self.state.borrow_mut();
            let active = state.active;
            let prompt = prompt_for(&state.tabs[active].cwd);
            let vt = &mut state.tabs[active].vt;
            vt.feed_str(&format!("{prompt}{line}\r\n"));
            if line.is_empty() {
                drop(state);
                self.sync();
                return;
            }
            if line == "clear" {
                vt.clear();
                drop(state);
                self.sync();
                return;
            }
            let output = execute(&mut state.tabs[active], &line);
            state.tabs[active].vt.feed_str(&output);
        }
        self.sync();
    }

    pub fn clear(&self) {
        {
            let mut state = self.state.borrow_mut();
            let active = state.active;
            state.tabs[active].vt.clear();
        }
        self.sync();
    }

    pub fn select_tab(&self, index: usize) {
        {
            let mut state = self.state.borrow_mut();
            if index < state.tabs.len() {
                state.active = index;
            }
        }
        self.sync();
    }

    pub fn new_tab(&self) {
        {
            let mut state = self.state.borrow_mut();
            let shell = Command::default_shell();
            let base = shell
                .program()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "sh".to_string());
            let count = state
                .tabs
                .iter()
                .filter(|tab| tab.shell == shell.program().to_string_lossy())
                .count();
            let title = if count == 0 {
                base
            } else {
                format!("{base} ({})", count + 1)
            };
            let cwd = state.tabs[state.active].cwd.clone();
            let mut tab = Tab::new(title, shell.program().to_path_buf());
            tab.cwd = cwd;
            tab.vt
                .feed_str("\x1b[2mNew local shell session\x1b[0m\r\n\r\n");
            state.tabs.push(tab);
            state.active = state.tabs.len() - 1;
        }
        self.sync();
    }

    pub fn close_tab(&self, index: usize) {
        {
            let mut state = self.state.borrow_mut();
            if state.tabs.len() <= 1 || index >= state.tabs.len() {
                return;
            }
            state.tabs.remove(index);
            if state.active > index {
                state.active -= 1;
            }
            if state.active >= state.tabs.len() {
                state.active = state.tabs.len() - 1;
            }
        }
        self.sync();
    }

    /// Start the real PTY for the active tab (unix only, see [`session`]).
    pub fn spawn_pty(&self, cols: u16, rows: u16) -> Result<(), SessionError> {
        let mut state = self.state.borrow_mut();
        let active = state.active;
        let shell = state.tabs[active].shell.clone();
        let result = PtySession::spawn(&Command::new(&shell), cols, rows);
        match result {
            Ok(session) => {
                state.tabs[active].pty = Some(session);
                state.tabs[active].pty_error = None;
                state.note = format!("PTY ready · {shell}");
                drop(state);
                self.sync();
                Ok(())
            }
            Err(error) => {
                state.tabs[active].pty_error = Some(error.to_string());
                state.note = error.to_string();
                Err(error)
            }
        }
    }

    /// Read whatever the PTY has produced and draw it. The interactive host
    /// calls this on a short timer. When the shell has exited, the decoded
    /// status is drawn and shown in the status bar (DA-AC-07).
    pub fn pump(&self, timeout_ms: i32) {
        let mut drew = false;
        let mut ended = None;
        {
            let mut state = self.state.borrow_mut();
            let active = state.active;
            if let Some(mut pty) = state.tabs[active].pty.take() {
                match pty.read_available(timeout_ms) {
                    Ok(bytes) if !bytes.is_empty() => {
                        state.tabs[active].vt.feed(&bytes);
                        drew = true;
                    }
                    Ok(_) => {}
                    Err(error) => state.note = error.to_string(),
                }
                if let Some(status) = pty.wait() {
                    ended = Some(status);
                } else {
                    state.tabs[active].pty = Some(pty);
                }
            }
        }
        if let Some(status) = ended {
            let label = status.label();
            {
                let mut state = self.state.borrow_mut();
                let active = state.active;
                state.tabs[active]
                    .vt
                    .feed_str(&format!("\r\n\x1b[2m[process {label}]\x1b[0m\r\n"));
                state.note = format!("Process {label}");
            }
            self.sync();
        } else if drew {
            self.sync();
        }
    }

    /// Resize the active tab: the screen grid and, when a PTY is open, the
    /// kernel window size (the shell receives `SIGWINCH`). The interactive
    /// host calls this from its window-resize event.
    pub fn resize(&self, cols: u16, rows: u16) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut error = None;
        {
            let mut state = self.state.borrow_mut();
            let active = state.active;
            state.tabs[active].vt.resize(cols as usize, rows as usize);
            if let Some(pty) = state.tabs[active].pty.as_mut() {
                error = pty.resize(cols, rows).err();
            }
        }
        if let Some(error) = error {
            self.state.borrow_mut().note = error.to_string();
        }
        self.sync();
    }

    /// The status-bar note (resize errors and exit codes are reported here).
    pub fn note(&self) -> String {
        self.state.borrow().note.clone()
    }

    fn wire(self: &Rc<Self>) {
        {
            let weak = Rc::downgrade(self);
            self.ui.on_prompt_submitted(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.submit();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_prompt_edited(move |text| {
                if let Some(controller) = weak.upgrade() {
                    controller.state.borrow_mut().prompt = text.to_string();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_select_tab(move |index| {
                if let Some(controller) = weak.upgrade() {
                    controller.select_tab(index.max(0) as usize);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_new_tab(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.new_tab();
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_close_tab(move |index| {
                if let Some(controller) = weak.upgrade() {
                    controller.close_tab(index.max(0) as usize);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_clear_screen(move || {
                if let Some(controller) = weak.upgrade() {
                    controller.clear();
                }
            });
        }
    }

    fn sync(&self) {
        let state = self.state.borrow();
        let active = state.active;
        let tab = &state.tabs[active];
        let ui = &self.ui;

        let tabs: Vec<SharedString> = state
            .tabs
            .iter()
            .map(|tab| SharedString::from(tab.title.clone()))
            .collect();
        ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
        ui.set_active_tab(active as i32);

        let glyphs: Vec<TermGlyph> = tab
            .vt
            .grid()
            .iter()
            .enumerate()
            .flat_map(|(row, line)| {
                line.iter()
                    .enumerate()
                    .filter(|(_, cell)| !cell.is_blank())
                    .map(move |(col, cell)| TermGlyph {
                        row: row as i32,
                        col: col as i32,
                        ch: SharedString::from(cell.ch.to_string()),
                        fg: ansi_index(cell.fg),
                        bold: cell.bold,
                        dim: cell.dim,
                    })
            })
            .collect();
        ui.set_glyphs(ModelRc::new(VecModel::from(glyphs)));
        ui.set_grid_cols(tab.vt.cols() as i32);
        ui.set_grid_rows(tab.vt.rows() as i32);
        ui.set_prompt_text(SharedString::from(state.prompt.clone()));
        ui.set_prompt_label(SharedString::from(prompt_label(&tab.cwd)));

        let scrollback = tab.vt.scrollback_len();
        let pty_state = if tab.pty.is_some() {
            "PTY ready"
        } else if tab.pty_error.is_some() {
            "PTY unavailable"
        } else {
            "local shell"
        };
        ui.set_pty_ready(tab.pty.is_some());
        ui.set_status_left(SharedString::from(format!(
            "{} · {}×{} · UTF-8 · {} · scrollback {} lines",
            pty_state,
            tab.vt.cols(),
            tab.vt.rows(),
            tab.shell,
            scrollback
        )));
        ui.set_status_right(SharedString::from(format!(
            "⏎ run · ⌘K clear · ⌘T new tab · {}",
            state.note
        )));
    }
}

impl Tab {
    fn new(title: String, shell: PathBuf) -> Tab {
        let cwd = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));
        Tab {
            title,
            vt: Vt::new(DEFAULT_COLS, DEFAULT_ROWS),
            cwd,
            shell: shell.to_string_lossy().to_string(),
            pty: None,
            pty_error: None,
        }
    }
}

/// ANSI palette index for the window: -1 means "default foreground".
fn ansi_index(color: AnsiColor) -> i32 {
    match color {
        AnsiColor::Default => -1,
        AnsiColor::Indexed(index) => index as i32,
    }
}

/// `~/Projects/x` when under the home directory, else the full path.
fn short_cwd(path: &PathBuf) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            let rest = rest.display();
            return if rest.to_string().is_empty() {
                "~".to_string()
            } else {
                format!("~/{rest}")
            };
        }
    }
    path.display().to_string()
}

/// Prompt printed into the screen (with SGR colours).
fn prompt_for(cwd: &PathBuf) -> String {
    format!(
        "\x1b[32maether@aether\x1b[0m:\x1b[34m{}\x1b[0m$ ",
        short_cwd(cwd)
    )
}

/// Prompt shown next to the input row (plain text).
fn prompt_label(cwd: &PathBuf) -> String {
    format!("aether@aether:{} $", short_cwd(cwd))
}

/// The built-in shell. Real output, real ANSI, no display host needed.
fn execute(tab: &mut Tab, line: &str) -> String {
    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or("");
    let args: Vec<&str> = parts.collect();
    match command {
        "help" => "\x1b[1mmandatory commands\x1b[0m\r\n\
                   help        this list\r\n\
                   ls [dir]    list a directory\r\n\
                   cd [dir]    change the working directory\r\n\
                   pwd         print the working directory\r\n\
                   echo TEXT   print TEXT\r\n\
                   uname       system information\r\n\
                   whoami      current user\r\n\
                   clear       clear the screen and scrollback\r\n\
                   \x1b[2mThe real PTY path lives in src/session.rs (forkpty/execvp).\x1b[0m\r\n"
            .to_string(),
        "echo" => format!("{}\r\n", args.join(" ")),
        "pwd" => format!("{}\r\n", tab.cwd.display()),
        "uname" => format!(
            "AetherOS 0.1.0-p4 {} {}\r\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        "whoami" => format!(
            "{}\r\n",
            std::env::var("USER").unwrap_or_else(|_| "aether".to_string())
        ),
        "cd" => {
            let target = args.first().copied().unwrap_or("~");
            let next = if target == "~" {
                std::env::var("HOME").map(PathBuf::from).ok()
            } else {
                let candidate = tab.cwd.join(target);
                Some(candidate)
            };
            match next {
                Some(path) if path.is_dir() => {
                    tab.cwd = path.canonicalize().unwrap_or(path);
                    String::new()
                }
                _ => format!("\x1b[31mcd: no such directory: {target}\x1b[0m\r\n"),
            }
        }
        "ls" => list_directory(tab, args.first().copied()),
        other => format!("\x1b[31maether-shell: command not found: {other}\x1b[0m\r\n"),
    }
}

fn list_directory(tab: &Tab, argument: Option<&str>) -> String {
    let target = match argument {
        Some(path) => tab.cwd.join(path),
        None => tab.cwd.clone(),
    };
    let entries = match std::fs::read_dir(&target) {
        Ok(entries) => entries,
        Err(error) => {
            return format!(
                "\x1b[31mls: {}: {}\x1b[0m\r\n",
                target.display(),
                error.kind()
            )
        }
    };
    let mut names: Vec<(String, bool)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            (entry.file_name().to_string_lossy().to_string(), is_dir)
        })
        .collect();
    names.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    if names.is_empty() {
        return "\x1b[2m(empty directory)\x1b[0m\r\n".to_string();
    }
    let mut output = String::new();
    for (name, is_dir) in names {
        if is_dir {
            output.push_str(&format!("\x1b[34m{name}/\x1b[0m\r\n"));
        } else {
            output.push_str(&format!("{name}\r\n"));
        }
    }
    output
}

/// Rendered demo transcript (fed through the VT parser at startup).
const DEMO_TRANSCRIPT: &str = "\x1b[32maether@aether\x1b[0m:\x1b[34m~/Projects/aetheros\x1b[0m$ git status\r\n\
On branch main\r\n\
Your branch is up to date with 'origin/main'.\r\n\
\r\n\
Changes not staged for commit:\r\n\
  (use \"git add <file>...\" to update what will be committed)\r\n\
        \x1b[31mmodified:   shell/src/app.rs\x1b[0m\r\n\
        \x1b[31mmodified:   ui/design.slint\x1b[0m\r\n\
        \x1b[32mnew file:   apps/text/src/document.rs\x1b[0m\r\n\
\r\n\
\x1b[32maether@aether\x1b[0m:\x1b[34m~/Projects/aetheros\x1b[0m$ cargo test -p aether-terminal\r\n\
   Compiling aether-terminal v0.1.0 (/home/aether/Projects/aetheros/apps/terminal)\r\n\
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.42s\r\n\
     Running unittests src/lib.rs (target/debug/deps/aether_terminal-3f1c)\r\n\
\r\n\
running 11 tests\r\n\
test vt::tests::prints_text_and_wraps_at_the_grid_width ... \x1b[32mok\x1b[0m\r\n\
test vt::tests::csi_cursor_movement_and_positioning ... \x1b[32mok\x1b[0m\r\n\
test vt::tests::erase_line_and_display_modes ... \x1b[32mok\x1b[0m\r\n\
test vt::tests::sgr_sets_and_resets_attributes ... \x1b[32mok\x1b[0m\r\n\
test vt::tests::os_and_unknown_sequences ... \x1b[32mok\x1b[0m\r\n\
test session::tests::argv_is_program_then_arguments ... \x1b[32mok\x1b[0m\r\n\
\r\n\
\x1b[1m\x1b[32mtest result: ok.\x1b[0m 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\r\n\
\r\n\
\x1b[33mwarning\x1b[0m: unused variable: `spike`\r\n\
\x1b[2m  --> apps/terminal/src/session.rs:42:9\x1b[0m\r\n\
\r\n\
\x1b[32maether@aether\x1b[0m:\x1b[34m~/Projects/aetheros\x1b[0m$ \x1b[1m█\x1b[0m";

/// Clipboard text for the visible screen: trailing blanks removed per line and
/// trailing empty lines dropped, so a copy never pastes a wall of spaces.
pub fn trim_screen(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// Normalise pasted text for a terminal: line breaks become spaces so a pasted
/// block cannot execute a command on its own (DA-AC-07 paste guard).
pub fn sanitise_paste(text: &str) -> String {
    text.replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod clipboard_tests {
    use super::*;

    #[test]
    fn trim_screen_drops_trailing_blanks_and_empty_lines() {
        assert_eq!(trim_screen("a    \nb\n\n\n"), "a\nb");
        assert_eq!(trim_screen("  indented\n"), "  indented");
        assert_eq!(trim_screen(""), "");
    }

    #[test]
    fn paste_cannot_smuggle_a_line_break() {
        assert_eq!(sanitise_paste("ls -la\r\nrm -rf /\n"), "ls -la rm -rf /");
        assert_eq!(sanitise_paste("multi\nline"), "multi line");
        assert_eq!(sanitise_paste("   "), "");
        assert_eq!(sanitise_paste("plain"), "plain");
    }
}
