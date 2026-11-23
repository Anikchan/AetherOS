//! VT100/ANSI screen model (P4-T15, DA-AC-07).
//!
//! A small, testable parser for the subset a shell needs: printable UTF-8
//! text, CR/LF/BS/TAB, CSI cursor movement (`A B C D H f`), erase (`J K`),
//! SGR colours and attributes (`0 1 2 22 30-37 39 90-97`) and safely ignored
//! OSC title sequences. The grid is fixed size and lines that scroll off the
//! top are kept in a bounded scrollback.
//!
//! The window renders [`Vt::grid`] cell by cell, so alignment does not depend
//! on the host having a monospace font installed.

use std::collections::VecDeque;

pub const DEFAULT_COLS: usize = 124;
pub const DEFAULT_ROWS: usize = 32;
/// Lines kept above the visible grid.
pub const MAX_SCROLLBACK: usize = 1000;

/// Terminal colour, either the default foreground or an ANSI palette index
/// (0-7 normal, 8-15 bright).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnsiColor {
    #[default]
    Default,
    Indexed(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: AnsiColor,
    pub bold: bool,
    pub dim: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg: AnsiColor::Default,
            bold: false,
            dim: false,
        }
    }
}

impl Cell {
    pub fn blank() -> Cell {
        Cell::default()
    }

    pub fn is_blank(&self) -> bool {
        self.ch == ' ' && self.fg == AnsiColor::Default && !self.bold && !self.dim
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    fg: AnsiColor,
    bold: bool,
    dim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    Ground,
    Escape,
    Csi,
    /// OSC / string sequences: consumed until BEL or ST.
    String,
    /// ESC seen inside a string sequence; `\` terminates it.
    StringEscape,
}

/// Fixed-size ANSI screen with scrollback.
#[derive(Debug, Clone)]
pub struct Vt {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<Cell>>,
    scrollback: VecDeque<Vec<Cell>>,
    cursor_col: usize,
    cursor_row: usize,
    saved_cursor: (usize, usize),
    style: Style,
    state: ParseState,
    params: Vec<u16>,
    param: Option<u16>,
    /// A printable at the last column defers the wrap until the next glyph.
    wrap_pending: bool,
    /// Incomplete UTF-8 sequence carried between reads.
    pending: Vec<u8>,
}

impl Default for Vt {
    fn default() -> Self {
        Vt::new(DEFAULT_COLS, DEFAULT_ROWS)
    }
}

impl Vt {
    pub fn new(cols: usize, rows: usize) -> Vt {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Vt {
            cols,
            rows,
            grid: vec![vec![Cell::blank(); cols]; rows],
            scrollback: VecDeque::new(),
            cursor_col: 0,
            cursor_row: 0,
            saved_cursor: (0, 0),
            style: Style::default(),
            state: ParseState::Ground,
            params: Vec::new(),
            param: None,
            wrap_pending: false,
            pending: Vec::new(),
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn grid(&self) -> &[Vec<Cell>] {
        &self.grid
    }

    pub fn scrollback(&self) -> &VecDeque<Vec<Cell>> {
        &self.scrollback
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_col, self.cursor_row)
    }

    pub fn cell(&self, col: usize, row: usize) -> Option<Cell> {
        self.grid.get(row)?.get(col).copied()
    }

    /// Row text with trailing blanks removed (for tests and copy).
    pub fn line_text(&self, row: usize) -> String {
        self.grid
            .get(row)
            .map(|cells| {
                cells
                    .iter()
                    .map(|cell| cell.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .unwrap_or_default()
    }

    /// Whole visible screen, newline separated.
    pub fn screen_text(&self) -> String {
        (0..self.rows)
            .map(|row| self.line_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn clear(&mut self) {
        self.grid = vec![vec![Cell::blank(); self.cols]; self.rows];
        self.scrollback.clear();
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.saved_cursor = (0, 0);
        self.style = Style::default();
        self.state = ParseState::Ground;
        self.params.clear();
        self.param = None;
        self.wrap_pending = false;
        self.pending.clear();
    }

    /// Feed raw bytes from the PTY. UTF-8 sequences split across reads are
    /// buffered until complete.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        loop {
            if self.pending.is_empty() {
                return;
            }
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    let owned = text.to_string();
                    self.pending.clear();
                    for ch in owned.chars() {
                        self.handle_char(ch);
                    }
                    return;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        let owned = std::str::from_utf8(&self.pending[..valid])
                            .expect("valid_up_to is a char boundary")
                            .to_string();
                        self.pending.drain(..valid);
                        for ch in owned.chars() {
                            self.handle_char(ch);
                        }
                        continue;
                    }
                    match error.error_len() {
                        Some(len) => {
                            self.pending.drain(..len);
                            self.handle_char('\u{FFFD}');
                        }
                        // Incomplete sequence: wait for the next read.
                        None => return,
                    }
                }
            }
        }
    }

    pub fn feed_str(&mut self, text: &str) {
        self.feed(text.as_bytes());
    }

    /// Resize the grid, keeping the top-left content and clamping the cursor.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut grid = vec![vec![Cell::blank(); cols]; rows];
        for (row, line) in self.grid.iter().take(rows).enumerate() {
            for (col, cell) in line.iter().take(cols).enumerate() {
                grid[row][col] = *cell;
            }
        }
        self.cols = cols;
        self.rows = rows;
        self.grid = grid;
        self.cursor_col = self.cursor_col.min(cols - 1);
        self.cursor_row = self.cursor_row.min(rows - 1);
        self.wrap_pending = false;
    }

    // --- character dispatch ------------------------------------------------

    fn handle_char(&mut self, ch: char) {
        match self.state {
            ParseState::Ground => self.ground(ch),
            ParseState::Escape => match ch {
                '[' => {
                    self.state = ParseState::Csi;
                    self.params.clear();
                    self.param = None;
                }
                // OSC and the other string sequences are ignored safely.
                ']' | 'P' | '^' | '_' => self.state = ParseState::String,
                '7' => {
                    self.saved_cursor = (self.cursor_col, self.cursor_row);
                    self.state = ParseState::Ground;
                }
                '8' => {
                    self.cursor_col = self.saved_cursor.0.min(self.cols - 1);
                    self.cursor_row = self.saved_cursor.1.min(self.rows - 1);
                    self.state = ParseState::Ground;
                }
                'c' => {
                    self.clear();
                    self.state = ParseState::Ground;
                }
                _ => self.state = ParseState::Ground,
            },
            ParseState::Csi => self.csi(ch),
            ParseState::String => match ch {
                '\x07' => self.state = ParseState::Ground,
                '\x1b' => self.state = ParseState::StringEscape,
                _ => {}
            },
            ParseState::StringEscape => {
                // ST is ESC \\; any other ESC aborts the string sequence.
                self.state = ParseState::Ground;
            }
        }
    }

    fn ground(&mut self, ch: char) {
        match ch {
            '\x1b' => {
                self.state = ParseState::Escape;
            }
            '\r' => {
                self.cursor_col = 0;
                self.wrap_pending = false;
            }
            '\n' => {
                self.linefeed();
                self.wrap_pending = false;
            }
            '\x08' => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                self.wrap_pending = false;
            }
            '\t' => {
                let next = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = next.min(self.cols - 1);
                self.wrap_pending = false;
            }
            '\x07' => {}
            ch if ch.is_control() => {}
            ch => self.print(ch),
        }
    }

    fn print(&mut self, ch: char) {
        if self.wrap_pending {
            self.cursor_col = 0;
            self.linefeed();
            self.wrap_pending = false;
        }
        let cell = Cell {
            ch,
            fg: self.style.fg,
            bold: self.style.bold,
            dim: self.style.dim,
        };
        self.grid[self.cursor_row][self.cursor_col] = cell;
        if self.cursor_col + 1 >= self.cols {
            self.wrap_pending = true;
        } else {
            self.cursor_col += 1;
        }
    }

    fn linefeed(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_row += 1;
        }
    }

    fn scroll_up(&mut self) {
        let line = self.grid.remove(0);
        self.scrollback.push_back(line);
        while self.scrollback.len() > MAX_SCROLLBACK {
            self.scrollback.pop_front();
        }
        self.grid.push(vec![Cell::blank(); self.cols]);
    }

    // --- CSI ---------------------------------------------------------------

    fn csi(&mut self, ch: char) {
        match ch {
            '0'..='9' => {
                let digit = ch as u16 - '0' as u16;
                let value = self
                    .param
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit);
                self.param = Some(value.min(9999));
            }
            ';' | ':' => {
                self.params.push(self.param.take().unwrap_or(0));
            }
            '?' | '>' | '!' | '<' | '=' => {
                // Private modes (cursor visibility, alternate screen): ignored.
            }
            c if (' '..='/').contains(&c) => {
                // Intermediate bytes: ignored.
            }
            c if ('@'..='~').contains(&c) => {
                if let Some(value) = self.param.take() {
                    self.params.push(value);
                }
                let params = std::mem::take(&mut self.params);
                self.dispatch(c, &params);
                self.state = ParseState::Ground;
            }
            _ => {
                // Malformed sequence: return to ground without acting.
                self.state = ParseState::Ground;
                self.params.clear();
                self.param = None;
            }
        }
    }

    fn dispatch(&mut self, final_byte: char, params: &[u16]) {
        let first = params.first().copied().unwrap_or(0) as usize;
        let n = if first == 0 { 1 } else { first };
        match final_byte {
            'A' => self.cursor_row = self.cursor_row.saturating_sub(n),
            'B' => self.cursor_row = (self.cursor_row + n).min(self.rows - 1),
            'C' => {
                self.cursor_col = (self.cursor_col + n).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'D' => {
                self.cursor_col = self.cursor_col.saturating_sub(n);
                self.wrap_pending = false;
            }
            'E' => {
                self.cursor_row = (self.cursor_row + n).min(self.rows - 1);
                self.cursor_col = 0;
            }
            'F' => {
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
            }
            'G' => self.cursor_col = (n - 1).min(self.cols - 1),
            'd' => self.cursor_row = (n - 1).min(self.rows - 1),
            'H' | 'f' => {
                let row = params.first().copied().unwrap_or(1) as usize;
                let col = params.get(1).copied().unwrap_or(1) as usize;
                self.cursor_row = row.max(1).saturating_sub(1).min(self.rows - 1);
                self.cursor_col = col.max(1).saturating_sub(1).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'J' => self.erase_display(first),
            'K' => self.erase_line(first),
            'm' => self.sgr(params),
            's' => self.saved_cursor = (self.cursor_col, self.cursor_row),
            'u' => {
                self.cursor_col = self.saved_cursor.0.min(self.cols - 1);
                self.cursor_row = self.saved_cursor.1.min(self.rows - 1);
            }
            'X' => {
                let end = (self.cursor_col + n).min(self.cols);
                for col in self.cursor_col..end {
                    self.grid[self.cursor_row][col] = Cell::blank();
                }
            }
            'P' => {
                let row = &mut self.grid[self.cursor_row];
                let count = n.min(self.cols - self.cursor_col);
                row.drain(self.cursor_col..self.cursor_col + count);
                row.extend(std::iter::repeat(Cell::blank()).take(count));
            }
            '@' => {
                let row = &mut self.grid[self.cursor_row];
                let count = n.min(self.cols - self.cursor_col);
                for _ in 0..count {
                    row.insert(self.cursor_col, Cell::blank());
                }
                row.truncate(self.cols);
            }
            // Scroll region and modes are accepted but not applied: the grid
            // has no per-region scrolling yet (P4-T15 follow-up).
            _ => {}
        }
    }

    fn erase_display(&mut self, mode: usize) {
        match mode {
            0 => {
                self.erase_line(0);
                for row in self.cursor_row + 1..self.rows {
                    self.grid[row] = vec![Cell::blank(); self.cols];
                }
            }
            1 => {
                self.erase_line(1);
                for row in 0..self.cursor_row {
                    self.grid[row] = vec![Cell::blank(); self.cols];
                }
            }
            _ => {
                for row in 0..self.rows {
                    self.grid[row] = vec![Cell::blank(); self.cols];
                }
            }
        }
    }

    fn erase_line(&mut self, mode: usize) {
        let row = &mut self.grid[self.cursor_row];
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    row[col] = Cell::blank();
                }
            }
            1 => {
                for col in 0..=self.cursor_col.min(self.cols - 1) {
                    row[col] = Cell::blank();
                }
            }
            _ => {
                for col in 0..self.cols {
                    row[col] = Cell::blank();
                }
            }
        }
    }

    fn sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.style = Style::default();
            return;
        }
        for &param in params {
            match param {
                0 => self.style = Style::default(),
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                30..=37 => self.style.fg = AnsiColor::Indexed((param - 30) as u8),
                39 => self.style.fg = AnsiColor::Default,
                90..=97 => self.style.fg = AnsiColor::Indexed((param - 90 + 8) as u8),
                // Background colours and the rest are accepted but unused.
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(vt: &Vt, row: usize) -> String {
        vt.line_text(row)
    }

    #[test]
    fn prints_text_and_wraps_at_the_grid_width() {
        let mut vt = Vt::new(10, 3);
        vt.feed_str("abcdefghij");
        assert_eq!(row(&vt, 0), "abcdefghij");
        assert_eq!(vt.cursor(), (9, 0), "the wrap is deferred");
        vt.feed_str("K");
        assert_eq!(row(&vt, 1), "K");
        assert_eq!(vt.cursor(), (1, 1));
        // Exactly filling a line then CRLF does not create a blank line.
        let mut vt = Vt::new(4, 2);
        vt.feed_str("abcd\r\nxy");
        assert_eq!(row(&vt, 0), "abcd");
        assert_eq!(row(&vt, 1), "xy");
    }

    #[test]
    fn handles_cr_lf_bs_and_tab() {
        let mut vt = Vt::new(20, 5);
        vt.feed_str("one\r\ntwo\rthree");
        assert_eq!(row(&vt, 0), "one");
        assert_eq!(row(&vt, 1), "three", "CR returns to column 0");
        vt.feed_str("\nx\ty");
        // LF keeps the column (5), the tab advances to the next stop (8).
        assert_eq!(row(&vt, 2), "     x  y", "tab stops every 8 columns");
        vt.feed_str("\x08Z");
        assert_eq!(row(&vt, 2), "     x  Z");
        // Backspace at column 0 is a no-op.
        vt.feed_str("\r\x08\x08!");
        assert_eq!(row(&vt, 2), "!    x  Z");
    }

    #[test]
    fn csi_cursor_movement_and_positioning() {
        let mut vt = Vt::new(10, 5);
        vt.feed_str("\x1b[3;4H");
        assert_eq!(vt.cursor(), (3, 2), "H uses 1-based row;col");
        vt.feed_str("\x1b[2A");
        assert_eq!(vt.cursor(), (3, 0));
        vt.feed_str("\x1b[2B\x1b[1C\x1b[2D");
        assert_eq!(vt.cursor(), (2, 2));
        vt.feed_str("\x1b[5G\x1b[4d");
        assert_eq!(vt.cursor(), (4, 3));
        // Movement clamps to the grid instead of panicking.
        vt.feed_str("\x1b[99A\x1b[99D");
        assert_eq!(vt.cursor(), (0, 0));
        vt.feed_str("\x1b[99B\x1b[99C");
        assert_eq!(vt.cursor(), (9, 4));
        vt.feed_str("\x1b[2;3fX");
        assert_eq!(vt.cursor(), (3, 1));
        assert_eq!(row(&vt, 1), "  X");
    }

    #[test]
    fn erase_line_and_display_modes() {
        let mut vt = Vt::new(8, 3);
        vt.feed_str("abcdefgh\r\nijkl\r\nmnop");
        assert_eq!(row(&vt, 0), "abcdefgh");

        // K: erase from the cursor to the end of the line.
        vt.feed_str("\x1b[1;4H\x1b[K");
        assert_eq!(row(&vt, 0), "abc");
        // K1: erase from the start of the line to the cursor.
        vt.feed_str("\x1b[1;2H\x1b[1K");
        assert_eq!(row(&vt, 0), "  c");
        // K2: the whole line.
        vt.feed_str("\x1b[2K");
        assert_eq!(row(&vt, 0), "");

        vt.feed_str("\x1b[2;1H\x1b[J");
        assert_eq!(row(&vt, 1), "");
        assert_eq!(row(&vt, 2), "", "J0 clears everything after the cursor");

        // J1 erases everything before the cursor (rows above and the current
        // row up to and including the cursor cell).
        let mut vt = Vt::new(8, 3);
        vt.feed_str("AAA\r\nBBB\r\nCCC");
        vt.feed_str("\x1b[2;2H\x1b[1J");
        assert_eq!(row(&vt, 0), "");
        assert_eq!(row(&vt, 1), "  B");
        assert_eq!(row(&vt, 2), "CCC");
        vt.feed_str("\x1b[2J");
        assert!(vt.screen_text().trim().is_empty());
    }

    #[test]
    fn sgr_sets_and_resets_attributes() {
        let mut vt = Vt::new(20, 2);
        vt.feed_str("\x1b[31mR\x1b[0mD\x1b[1mB\x1b[22mN\x1b[2mF\x1b[0m\x1b[92mG\x1b[39mE");
        let cells: Vec<Cell> = vt.grid()[0].iter().take(6).copied().collect();
        assert_eq!(cells[0].fg, AnsiColor::Indexed(1), "31 -> red");
        assert_eq!(cells[1].fg, AnsiColor::Default, "0 resets");
        assert!(cells[2].bold && !cells[2].dim);
        assert!(!cells[3].bold, "22 clears bold");
        assert!(cells[4].dim);
        assert_eq!(cells[5].fg, AnsiColor::Indexed(10), "92 -> bright green");
        // 39 restores the default foreground.
        vt.feed_str("\x1b[39mX");
        assert_eq!(vt.cell(6, 0).unwrap().fg, AnsiColor::Default);
        // Only the printed cells carry style.
        assert!(vt.cell(19, 0).unwrap().is_blank());
    }

    #[test]
    fn scrollback_is_bounded_and_keeps_grown_lines() {
        let mut vt = Vt::new(20, 3);
        vt.feed_str("one\r\ntwo\r\nthree\r\nfour");
        assert_eq!(vt.scrollback_len(), 1);
        assert_eq!(
            vt.scrollback()
                .back()
                .unwrap()
                .iter()
                .map(|c| c.ch)
                .collect::<String>(),
            "one                 "
        );
        assert_eq!(row(&vt, 0), "two");
        assert_eq!(row(&vt, 2), "four");

        for step in 0..(MAX_SCROLLBACK + 10) {
            vt.feed_str(&format!("\r\nline {step}"));
        }
        assert_eq!(vt.scrollback_len(), MAX_SCROLLBACK);
        vt.clear();
        assert_eq!(vt.scrollback_len(), 0);
    }

    #[test]
    fn osc_and_unknown_sequences_are_ignored_safely() {
        let mut vt = Vt::new(20, 2);
        // Set the window title, then print.
        vt.feed_str("\x1b]0;my title\x07ok");
        assert_eq!(row(&vt, 0), "ok");
        // ST-terminated OSC.
        vt.feed_str("\x1b]2;another\x1b\\!");
        assert_eq!(row(&vt, 0), "ok!");
        // Unknown CSI finals and malformed sequences are ignored.
        vt.feed_str("\x1b[5;9z\x1b[?25l\x1b[38;5;200mP");
        assert_eq!(row(&vt, 0), "ok!P");
        // A lone ESC at the end of a read does not corrupt the next glyph.
        vt.feed_str("\x1b");
        vt.feed_str("[31mX");
        assert_eq!(vt.cell(4, 0).unwrap().ch, 'X');
        assert_eq!(vt.cell(4, 0).unwrap().fg, AnsiColor::Indexed(1));
    }

    #[test]
    fn utf8_split_across_feeds_is_decoded() {
        let mut vt = Vt::new(10, 2);
        let bytes = "héllo ✓".as_bytes();
        for chunk in bytes.chunks(3) {
            vt.feed(chunk);
        }
        assert_eq!(row(&vt, 0), "héllo ✓");
        // Invalid bytes become the replacement character and do not panic.
        vt.feed_str("\r\n");
        vt.feed(&[0xff, b'x']);
        assert!(row(&vt, 1).starts_with('\u{FFFD}'));
        assert!(row(&vt, 1).ends_with('x'));
    }

    #[test]
    fn resize_keeps_content_and_clamps_the_cursor() {
        let mut vt = Vt::new(10, 3);
        vt.feed_str("abcdef\r\nghijkl");
        vt.feed_str("\x1b[3;10H");
        vt.resize(6, 2);
        assert_eq!(vt.cols(), 6);
        assert_eq!(vt.rows(), 2);
        assert_eq!(row(&vt, 0), "abcdef");
        assert_eq!(row(&vt, 1), "ghijkl");
        assert_eq!(vt.cursor(), (5, 1));
        vt.resize(12, 4);
        assert_eq!(row(&vt, 1), "ghijkl");
        assert!(row(&vt, 3).is_empty(), "new rows are blank");
        // Printing after a resize stays in bounds.
        vt.feed_str("\x1b[4;12HZ");
        assert_eq!(vt.cell(11, 3).unwrap().ch, 'Z');
    }
}
