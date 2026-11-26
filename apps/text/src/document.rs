//! AetherPad document model (P4-T10, DA-AC-01/02).
//!
//! The editor window is a thin view over this module: every edit, undo step,
//! find/replace result, count, save and recent-file entry is produced here and
//! unit tested without a display. Operations are expressed in **char** indices
//! (not bytes) so unicode text edits and counts stay correct; the module maps
//! them to byte offsets internally.
//!
//! Size guards: a document may not exceed [`MAX_DOCUMENT_BYTES`], and the undo
//! history is bounded both by an entry count and a byte budget.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

/// Hard cap for one document (16 MiB of UTF-8), enforced on every edit. The
/// cap covers DA-AC-02's 10 MB file with headroom while keeping a stray binary
/// file from being loaded as text.
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;

/// Default number of undo snapshots kept.
pub const DEFAULT_HISTORY_LIMIT: usize = 200;

/// Default byte budget for undo snapshots; older snapshots are dropped first.
pub const DEFAULT_HISTORY_BYTES: usize = 8 * 1024 * 1024;

/// Number of recent files remembered.
pub const DEFAULT_RECENT_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    /// The edit would grow the document past the size guard.
    TooLarge { limit: usize },
    /// A char index was outside `0..=char_count`.
    InvalidIndex { index: usize },
    /// A range was reversed or outside the document.
    InvalidRange { start: usize, end: usize },
    /// Reading or writing a file failed.
    Io(String),
    /// The file is not valid UTF-8.
    InvalidUtf8(String),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentError::TooLarge { limit } => {
                write!(f, "document is larger than the {limit} byte limit")
            }
            DocumentError::InvalidIndex { index } => write!(f, "invalid char index {index}"),
            DocumentError::InvalidRange { start, end } => {
                write!(f, "invalid char range {start}..{end}")
            }
            DocumentError::Io(message) => write!(f, "{message}"),
            DocumentError::InvalidUtf8(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Text file encodings AetherPad reads and writes (DA-AC-01).
///
/// Detection order: UTF-8 BOM, UTF-16 BOM, a UTF-16 NUL-byte heuristic, valid
/// UTF-8, and finally Windows-1252 (the common "ANSI" fallback). Saving writes
/// back in the encoding the file was opened with, so a round trip does not
/// change the file's bytes for any character the encoding can represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252",
        }
    }

    /// Pick the encoding for a byte sequence.
    pub fn detect(bytes: &[u8]) -> Encoding {
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return Encoding::Utf8Bom;
        }
        if bytes.starts_with(&[0xFF, 0xFE]) {
            return Encoding::Utf16Le;
        }
        if bytes.starts_with(&[0xFE, 0xFF]) {
            return Encoding::Utf16Be;
        }
        // UTF-16 without a BOM: ASCII text produces a NUL in every other byte.
        if bytes.len() >= 4 && bytes.len() % 2 == 0 {
            let even_nuls = bytes.iter().step_by(2).filter(|b| **b == 0).count();
            let odd_nuls = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
            let pairs = bytes.len() / 2;
            if odd_nuls * 4 >= pairs * 3 && even_nuls == 0 {
                return Encoding::Utf16Le;
            }
            if even_nuls * 4 >= pairs * 3 && odd_nuls == 0 {
                return Encoding::Utf16Be;
            }
        }
        if std::str::from_utf8(bytes).is_ok() {
            return Encoding::Utf8;
        }
        Encoding::Windows1252
    }

    /// Decode bytes with this encoding (the BOM is removed when present).
    pub fn decode(self, bytes: &[u8]) -> Result<String, DocumentError> {
        match self {
            Encoding::Utf8 => std::str::from_utf8(bytes)
                .map(ToString::to_string)
                .map_err(|error| DocumentError::InvalidUtf8(error.to_string())),
            Encoding::Utf8Bom => {
                let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
                std::str::from_utf8(rest)
                    .map(ToString::to_string)
                    .map_err(|error| DocumentError::InvalidUtf8(error.to_string()))
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let little = self == Encoding::Utf16Le;
                let mut rest = bytes;
                if little {
                    rest = rest.strip_prefix(&[0xFF, 0xFE]).unwrap_or(rest);
                } else {
                    rest = rest.strip_prefix(&[0xFE, 0xFF]).unwrap_or(rest);
                }
                if rest.len() % 2 != 0 {
                    return Err(DocumentError::InvalidUtf8(
                        "truncated UTF-16 sequence".to_string(),
                    ));
                }
                let units: Vec<u16> = rest
                    .chunks_exact(2)
                    .map(|pair| {
                        if little {
                            u16::from_le_bytes([pair[0], pair[1]])
                        } else {
                            u16::from_be_bytes([pair[0], pair[1]])
                        }
                    })
                    .collect();
                String::from_utf16(&units).map_err(|error| {
                    DocumentError::InvalidUtf8(format!("invalid UTF-16: {error}"))
                })
            }
            Encoding::Windows1252 => {
                let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
                Ok(text.into_owned())
            }
        }
    }

    /// Encode text for writing. Windows-1252 falls back to UTF-8 when a
    /// character is not representable, so saving never loses data.
    pub fn encode(self, text: &str) -> (Vec<u8>, Encoding) {
        match self {
            Encoding::Utf8 => (text.as_bytes().to_vec(), Encoding::Utf8),
            Encoding::Utf8Bom => {
                let mut bytes = vec![0xEF, 0xBB, 0xBF];
                bytes.extend_from_slice(text.as_bytes());
                (bytes, Encoding::Utf8Bom)
            }
            Encoding::Utf16Le => {
                let mut bytes = vec![0xFF, 0xFE];
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
                (bytes, Encoding::Utf16Le)
            }
            Encoding::Utf16Be => {
                let mut bytes = vec![0xFE, 0xFF];
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_be_bytes());
                }
                (bytes, Encoding::Utf16Be)
            }
            Encoding::Windows1252 => {
                let (bytes, _, had_errors) = encoding_rs::WINDOWS_1252.encode(text);
                if had_errors {
                    (text.as_bytes().to_vec(), Encoding::Utf8)
                } else {
                    (bytes.into_owned(), Encoding::Windows1252)
                }
            }
        }
    }
}

/// Count non-overlapping occurrences of `needle` in `hay` in one pass.
fn count_in(hay: &[char], needle: &[char]) -> usize {
    if needle.is_empty() || hay.len() < needle.len() {
        return 0;
    }
    let mut index = 0;
    let mut count = 0;
    while index + needle.len() <= hay.len() {
        if hay[index..index + needle.len()] == *needle {
            count += 1;
            index += needle.len();
        } else {
            index += 1;
        }
    }
    count
}

/// A text buffer with bounded undo/redo history and a saved-state marker.
#[derive(Debug, Clone)]
pub struct Document {
    text: String,
    path: Option<PathBuf>,
    /// Text as last written to disk (or at creation): drives the dirty flag.
    saved: String,
    undo: Vec<String>,
    redo: Vec<String>,
    history_limit: usize,
    history_bytes: usize,
    undo_bytes: usize,
    /// Encoding used the last time this document was read or written.
    encoding: Encoding,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Empty, untitled document with the default history budget.
    pub fn new() -> Document {
        Document {
            text: String::new(),
            path: None,
            saved: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            history_limit: DEFAULT_HISTORY_LIMIT,
            history_bytes: DEFAULT_HISTORY_BYTES,
            undo_bytes: 0,
            encoding: Encoding::Utf8,
        }
    }

    /// Untitled document seeded with `text`; `saved` is left empty so the demo
    /// document shows the unsaved dot.
    pub fn from_text(text: impl Into<String>) -> Document {
        let text = text.into();
        let mut doc = Document::new();
        doc.saved = text.clone();
        doc.text = text;
        doc
    }

    /// Document that already has a file path but unsaved edits on top.
    pub fn with_path(text: impl Into<String>, path: impl Into<PathBuf>) -> Document {
        let mut doc = Document::from_text(text);
        doc.path = Some(path.into());
        doc.saved.clear();
        doc
    }

    /// Same as [`Document::new`] but with a custom history budget (tests).
    pub fn with_history_limit(mut self, limit: usize) -> Document {
        self.history_limit = limit.max(1);
        self.trim_history();
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = Some(path.into());
    }

    /// Short display name for the window and the sidebar.
    pub fn name(&self) -> String {
        match &self.path {
            Some(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            None => "Untitled".to_string(),
        }
    }

    /// Path with `$HOME` shortened to `~` for the status bar.
    pub fn display_path(&self) -> String {
        match &self.path {
            None => "Not saved yet".to_string(),
            Some(path) => short_path(path),
        }
    }

    /// Replace the whole buffer as one undoable step.
    pub fn set_text(&mut self, text: impl Into<String>) -> Result<(), DocumentError> {
        let text = text.into();
        if text.len() > MAX_DOCUMENT_BYTES {
            return Err(DocumentError::TooLarge {
                limit: MAX_DOCUMENT_BYTES,
            });
        }
        if text == self.text {
            return Ok(());
        }
        self.commit(text);
        Ok(())
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    pub fn byte_len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Words split on unicode whitespace.
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    /// 0 for an empty document, otherwise newlines + 1.
    pub fn line_count(&self) -> usize {
        if self.text.is_empty() {
            0
        } else {
            self.text.matches('\n').count() + 1
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }

    /// Mark the current content as saved without touching the disk.
    pub fn mark_saved(&mut self) {
        self.saved = self.text.clone();
    }

    // --- edits -----------------------------------------------------------

    /// Insert `insert` at `char_index`, returning the index after the insert.
    pub fn insert(&mut self, char_index: usize, insert: &str) -> Result<usize, DocumentError> {
        if insert.is_empty() {
            return self.check_index(char_index).map(|_| char_index);
        }
        let byte = self.byte_index(char_index)?;
        self.check_size(self.text.len() + insert.len())?;
        let mut next = String::with_capacity(self.text.len() + insert.len());
        next.push_str(&self.text[..byte]);
        next.push_str(insert);
        next.push_str(&self.text[byte..]);
        self.commit(next);
        Ok(char_index + insert.chars().count())
    }

    /// Delete the chars in `start..end`; returns how many chars were removed.
    pub fn delete_range(&mut self, start: usize, end: usize) -> Result<usize, DocumentError> {
        if start > end || end > self.char_count() {
            return Err(DocumentError::InvalidRange { start, end });
        }
        if start == end {
            return Ok(0);
        }
        let from = self.byte_index(start)?;
        let to = self.byte_index(end)?;
        let mut next = String::with_capacity(self.text.len() - (to - from));
        next.push_str(&self.text[..from]);
        next.push_str(&self.text[to..]);
        self.commit(next);
        Ok(end - start)
    }

    /// Replace `start..end` with `insert` in one undoable step.
    pub fn replace_range(
        &mut self,
        start: usize,
        end: usize,
        insert: &str,
    ) -> Result<usize, DocumentError> {
        if start > end || end > self.char_count() {
            return Err(DocumentError::InvalidRange { start, end });
        }
        let from = self.byte_index(start)?;
        let to = self.byte_index(end)?;
        self.check_size(self.text.len() - (to - from) + insert.len())?;
        let mut next = String::with_capacity(self.text.len() - (to - from) + insert.len());
        next.push_str(&self.text[..from]);
        next.push_str(insert);
        next.push_str(&self.text[to..]);
        self.commit(next);
        Ok(start + insert.chars().count())
    }

    // --- history ---------------------------------------------------------

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Restore the text before the last edit.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.undo_bytes = self.undo_bytes.saturating_sub(previous.len());
        let current = std::mem::replace(&mut self.text, previous);
        self.redo.push(current);
        true
    }

    /// Re-apply the edit that the last undo removed.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.text, next);
        self.push_undo(current);
        // A redo must not wipe the remaining redo stack.
        true
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.undo_bytes = 0;
    }

    // --- search and replace ---------------------------------------------

    /// Find `needle` at or after `from_char`. Returns char indices
    /// `(start, end)`. Case folding is unicode aware.
    pub fn find(
        &self,
        needle: &str,
        from_char: usize,
        case_sensitive: bool,
    ) -> Option<(usize, usize)> {
        if needle.is_empty() {
            return None;
        }
        let hay: Vec<char> = self.text.chars().collect();
        let from_char = from_char.min(hay.len());
        if case_sensitive {
            let needle_chars: Vec<char> = needle.chars().collect();
            if needle_chars.len() > hay.len() {
                return None;
            }
            (from_char..=hay.len() - needle_chars.len())
                .find(|&start| hay[start..start + needle_chars.len()] == needle_chars[..])
                .map(|start| (start, start + needle_chars.len()))
        } else {
            let (folded, map) = fold_with_map(&hay);
            let needle_chars: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
            if needle_chars.len() > folded.len() {
                return None;
            }
            // From char -> folded position (folded is at least as long).
            let folded_from = map.partition_point(|&index| index < from_char);
            (folded_from..=folded.len() - needle_chars.len())
                .find(|&start| folded[start..start + needle_chars.len()] == needle_chars[..])
                .map(|start| {
                    let end = map[start + needle_chars.len() - 1] + 1;
                    (map[start], end)
                })
        }
    }

    /// Number of matches for the current query.
    /// Count non-overlapping matches in one pass (DA-AC-02: a 10 MB file must
    /// be searchable without freezing, so this must not call [`Self::find`] in
    /// a loop — that would re-collect the text for every match).
    pub fn count_matches(&self, needle: &str, case_sensitive: bool) -> usize {
        if needle.is_empty() {
            return 0;
        }
        let hay: Vec<char> = self.text.chars().collect();
        if case_sensitive {
            let needle_chars: Vec<char> = needle.chars().collect();
            count_in(&hay, &needle_chars)
        } else {
            let (folded, _) = fold_with_map(&hay);
            let needle_chars: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
            count_in(&folded, &needle_chars)
        }
    }

    /// Replace the first match at or after `from_char`; returns the char index
    /// after the replacement.
    pub fn replace_next(
        &mut self,
        needle: &str,
        replacement: &str,
        from_char: usize,
        case_sensitive: bool,
    ) -> Option<usize> {
        let (start, end) = self.find(needle, from_char, case_sensitive)?;
        self.replace_range(start, end, replacement).ok()
    }

    /// Replace every match; returns how many replacements happened.
    pub fn replace_all(&mut self, needle: &str, replacement: &str, case_sensitive: bool) -> usize {
        if needle.is_empty() {
            return 0;
        }
        let hay: Vec<char> = self.text.chars().collect();
        let mut result: Vec<char> = Vec::with_capacity(hay.len());
        let mut cursor = 0;
        let mut count = 0;
        while let Some((start, end)) = self.find_chars(&hay, needle, cursor, case_sensitive) {
            result.extend_from_slice(&hay[cursor..start]);
            result.extend(replacement.chars());
            cursor = end.max(start + 1);
            count += 1;
        }
        if count == 0 {
            return 0;
        }
        result.extend_from_slice(&hay[cursor..]);
        let next: String = result.into_iter().collect();
        if self.set_text(next).is_err() {
            return 0;
        }
        count
    }

    fn find_chars(
        &self,
        hay: &[char],
        needle: &str,
        from_char: usize,
        case_sensitive: bool,
    ) -> Option<(usize, usize)> {
        if case_sensitive {
            let needle_chars: Vec<char> = needle.chars().collect();
            if needle_chars.is_empty() || needle_chars.len() > hay.len() {
                return None;
            }
            (from_char..=hay.len() - needle_chars.len())
                .find(|&start| hay[start..start + needle_chars.len()] == needle_chars[..])
                .map(|start| (start, start + needle_chars.len()))
        } else {
            let (folded, map) = fold_with_map(hay);
            let needle_chars: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
            if needle_chars.is_empty() || needle_chars.len() > folded.len() {
                return None;
            }
            let folded_from = map.partition_point(|&index| index < from_char);
            (folded_from..=folded.len() - needle_chars.len())
                .find(|&start| folded[start..start + needle_chars.len()] == needle_chars[..])
                .map(|start| {
                    let end = map[start + needle_chars.len() - 1] + 1;
                    (map[start], end)
                })
        }
    }

    // --- persistence ------------------------------------------------------

    /// Read a text file, detecting UTF-8, UTF-16 (LE/BE) or Windows-1252
    /// (DA-AC-01). The detected encoding is remembered for saving.
    pub fn load(path: impl AsRef<Path>) -> Result<Document, DocumentError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|error| {
            DocumentError::Io(format!("cannot read {}: {error}", path.display()))
        })?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(DocumentError::TooLarge {
                limit: MAX_DOCUMENT_BYTES,
            });
        }
        let encoding = Encoding::detect(&bytes);
        let text = encoding.decode(&bytes).map_err(|error| match error {
            DocumentError::InvalidUtf8(message) => DocumentError::InvalidUtf8(format!(
                "{} cannot be decoded as {}: {message}",
                path.display(),
                encoding.label()
            )),
            other => other,
        })?;
        let mut doc = Document {
            text: text.clone(),
            path: Some(path.to_path_buf()),
            saved: text,
            encoding,
            ..Document::new()
        };
        doc.clear_history();
        Ok(doc)
    }

    /// Encoding this document will be written with.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Write the current buffer to the document path.
    pub fn save(&mut self) -> Result<PathBuf, DocumentError> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| DocumentError::Io("document has no file path yet".to_string()))?;
        self.save_as(path)
    }

    /// Write the current buffer to `path` and make it the document path.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<PathBuf, DocumentError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|error| {
                    DocumentError::Io(format!("cannot create {}: {error}", parent.display()))
                })?;
            }
        }
        let (bytes, encoding) = self.encoding.encode(&self.text);
        // Atomic save: write a sibling temp file, then rename over the target,
        // so a crash mid-write cannot truncate the document.
        let temp = path.with_extension(format!(
            "{}tmp",
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| format!("{ext}."))
                .unwrap_or_default()
        ));
        fs::write(&temp, &bytes).map_err(|error| {
            DocumentError::Io(format!("cannot write {}: {error}", temp.display()))
        })?;
        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(DocumentError::Io(format!(
                "cannot replace {}: {error}",
                path.display()
            )));
        }
        self.encoding = encoding;
        self.path = Some(path.clone());
        self.saved = self.text.clone();
        Ok(path)
    }

    // --- internals --------------------------------------------------------

    fn check_index(&self, char_index: usize) -> Result<usize, DocumentError> {
        if char_index > self.char_count() {
            return Err(DocumentError::InvalidIndex { index: char_index });
        }
        self.byte_index(char_index)
    }

    fn byte_index(&self, char_index: usize) -> Result<usize, DocumentError> {
        if char_index == 0 {
            return Ok(0);
        }
        if let Some((byte, _)) = self.text.char_indices().nth(char_index) {
            return Ok(byte);
        }
        if char_index == self.char_count() {
            return Ok(self.text.len());
        }
        Err(DocumentError::InvalidIndex { index: char_index })
    }

    fn check_size(&self, size: usize) -> Result<(), DocumentError> {
        if size > MAX_DOCUMENT_BYTES {
            return Err(DocumentError::TooLarge {
                limit: MAX_DOCUMENT_BYTES,
            });
        }
        Ok(())
    }

    /// One undoable step: snapshot the old text, swap in the new one.
    fn commit(&mut self, next: String) {
        let previous = std::mem::replace(&mut self.text, next);
        self.push_undo(previous);
        self.redo.clear();
    }

    fn push_undo(&mut self, snapshot: String) {
        self.undo_bytes += snapshot.len();
        self.undo.push(snapshot);
        self.trim_history();
    }

    fn trim_history(&mut self) {
        while self.undo.len() > self.history_limit {
            let removed = self.undo.remove(0);
            self.undo_bytes = self.undo_bytes.saturating_sub(removed.len());
        }
        while self.undo.len() > 1 && self.undo_bytes > self.history_bytes {
            let removed = self.undo.remove(0);
            self.undo_bytes = self.undo_bytes.saturating_sub(removed.len());
        }
    }
}

/// Lowercase every char while remembering which original char index produced
/// each folded char, so unicode case folding keeps indices correct.
fn fold_with_map(chars: &[char]) -> (Vec<char>, Vec<usize>) {
    let mut folded = Vec::with_capacity(chars.len());
    let mut map = Vec::with_capacity(chars.len());
    for (index, ch) in chars.iter().enumerate() {
        for lower in ch.to_lowercase() {
            folded.push(lower);
            map.push(index);
        }
    }
    (folded, map)
}

/// Shorten the user's home directory to `~`.
pub fn short_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home = Path::new(&home);
        if let Ok(rest) = path.strip_prefix(home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// AetherOS config root: `AETHER_CONFIG_HOME`, then `XDG_CONFIG_HOME/aether`,
/// then `~/.config/aether`.
pub fn config_root() -> PathBuf {
    if let Ok(dir) = std::env::var("AETHER_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("aether");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("aether")
}

/// Autosave target used when a document has no path yet.
pub fn autosave_path() -> PathBuf {
    config_root().join("text").join("autosave.txt")
}

/// Bounded most-recently-used file list, persisted one path per line.
#[derive(Debug, Clone)]
pub struct RecentFiles {
    store: PathBuf,
    limit: usize,
    entries: VecDeque<PathBuf>,
}

impl RecentFiles {
    pub fn new(store: impl Into<PathBuf>, limit: usize) -> RecentFiles {
        RecentFiles {
            store: store.into(),
            limit: limit.max(1),
            entries: VecDeque::new(),
        }
    }

    /// List kept under the user config root.
    pub fn at_default_location(limit: usize) -> RecentFiles {
        RecentFiles::new(config_root().join("text").join("recent.txt"), limit)
    }

    pub fn store_path(&self) -> &Path {
        &self.store
    }

    pub fn entries(&self) -> Vec<PathBuf> {
        self.entries.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert a path at the front, de-duplicating and capping the list.
    /// Returns true when the path was not already present.
    pub fn add(&mut self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref().to_path_buf();
        let existed = self.entries.iter().any(|entry| entry == &path);
        self.entries.retain(|entry| entry != &path);
        self.entries.push_front(path);
        while self.entries.len() > self.limit {
            self.entries.pop_back();
        }
        !existed
    }

    pub fn remove(&mut self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let before = self.entries.len();
        self.entries.retain(|entry| entry != path);
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Load the path list; a missing file is not an error.
    pub fn load(&mut self) -> Result<(), DocumentError> {
        let text = match fs::read_to_string(&self.store) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(DocumentError::Io(format!(
                    "cannot read {}: {error}",
                    self.store.display()
                )))
            }
        };
        self.entries.clear();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.entries.push_back(PathBuf::from(line));
            if self.entries.len() == self.limit {
                break;
            }
        }
        Ok(())
    }

    /// Write the path list back, one entry per line.
    pub fn persist(&self) -> Result<(), DocumentError> {
        if let Some(parent) = self.store.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                DocumentError::Io(format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        let body: String = self
            .entries
            .iter()
            .map(|path| format!("{}\n", path.display()))
            .collect();
        fs::write(&self.store, body).map_err(|error| {
            DocumentError::Io(format!("cannot write {}: {error}", self.store.display()))
        })
    }

    /// `(file name, short path)` pairs for the sidebar.
    pub fn labels(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                (name, short_path(path))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-text-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn edits_are_char_indexed_and_byte_guarded() {
        let mut doc = Document::new();
        doc.insert(0, "héllo").unwrap();
        assert_eq!(doc.text(), "héllo");
        assert_eq!(doc.char_count(), 5);
        assert_eq!(doc.byte_len(), 6, "é takes two UTF-8 bytes");
        // Insert inside a multi-byte word.
        doc.insert(1, "i").unwrap();
        assert_eq!(doc.text(), "hiéllo");
        doc.delete_range(1, 2).unwrap();
        assert_eq!(doc.text(), "héllo");
        assert!(doc.insert(99, "x").is_err());
        assert!(doc.replace_range(4, 2, "x").is_err());
        // Oversize guard: a single insert larger than the document cap.
        let giant = "x".repeat(MAX_DOCUMENT_BYTES + 1);
        assert_eq!(
            doc.insert(0, &giant).unwrap_err(),
            DocumentError::TooLarge {
                limit: MAX_DOCUMENT_BYTES
            }
        );
        assert_eq!(doc.text(), "héllo");
    }

    #[test]
    fn undo_and_redo_walk_the_history_sequence() {
        let mut doc = Document::new();
        doc.insert(0, "one").unwrap();
        doc.insert(3, " two").unwrap();
        doc.insert(7, " three").unwrap();
        assert!(doc.can_undo());
        assert!(doc.undo());
        assert_eq!(doc.text(), "one two");
        assert!(doc.undo());
        assert_eq!(doc.text(), "one");
        assert!(doc.undo());
        assert_eq!(doc.text(), "");
        assert!(!doc.can_undo());
        assert!(!doc.undo());
        assert!(doc.can_redo());
        assert!(doc.redo());
        assert_eq!(doc.text(), "one");
        assert!(doc.redo());
        assert!(doc.redo());
        assert_eq!(doc.text(), "one two three");
        assert!(!doc.can_redo());
        // A fresh edit invalidates the redo path.
        assert!(doc.undo());
        doc.insert(0, ">").unwrap();
        assert!(!doc.can_redo());
        assert_eq!(doc.text(), ">one two");
    }

    #[test]
    fn history_is_bounded_by_the_limit() {
        let mut doc = Document::new().with_history_limit(3);
        for step in 0..10 {
            doc.set_text(format!("step {step}")).unwrap();
        }
        assert!(doc.undo_len() <= 3, "history stayed bounded");
        // Undo only walks the retained snapshots.
        let mut undos = 0;
        while doc.undo() {
            undos += 1;
        }
        assert_eq!(undos, 3);
        assert_eq!(doc.text(), "step 6");
    }

    #[test]
    fn counts_are_unicode_aware() {
        let doc = Document::from_text("héllo wörld\n日本語 テスト\n\nlast");
        assert_eq!(doc.char_count(), 25);
        assert_eq!(doc.word_count(), 5);
        assert_eq!(doc.line_count(), 4);
        assert_eq!(Document::new().line_count(), 0);
        assert_eq!(Document::from_text("one\n").line_count(), 2);
        assert_eq!(Document::from_text("   ").word_count(), 0);
    }

    #[test]
    fn find_is_case_aware_and_unicode_safe() {
        let doc = Document::from_text("Rust and rust and RÜST");
        assert_eq!(doc.find("rust", 0, true), Some((9, 13)));
        assert_eq!(doc.find("rust", 13, true), None);
        assert_eq!(doc.find("rust", 0, false), Some((0, 4)));
        assert_eq!(doc.find("rüst", 0, false), Some((18, 22)));
        // "RÜST" folds to "rüst", which is not "rust".
        assert_eq!(doc.count_matches("rust", false), 2);
        assert_eq!(doc.count_matches("rust", true), 1);
        assert_eq!(doc.count_matches("", false), 0);
        assert_eq!(doc.find("missing", 0, false), None);
    }

    #[test]
    fn replace_next_and_replace_all() {
        let mut doc = Document::from_text("cat Cat catalog");
        assert_eq!(doc.replace_next("cat", "dog", 0, true), Some(3));
        assert_eq!(doc.text(), "dog Cat catalog");
        assert_eq!(doc.count_matches("cat", false), 2);
        assert_eq!(doc.replace_all("cat", "dog", false), 2);
        assert_eq!(doc.text(), "dog dog dogalog");
        assert_eq!(doc.replace_all("missing", "x", false), 0);
        assert_eq!(doc.replace_all("", "x", false), 0);
        // The whole replace-all is one undo step.
        assert!(doc.undo());
        assert_eq!(doc.text(), "dog Cat catalog");
    }

    fn temp_file(tag: &str, extension: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-text-encoding-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("sample.{extension}"))
    }

    #[test]
    fn detects_supported_encodings() {
        assert_eq!(Encoding::detect(b"plain ascii"), Encoding::Utf8);
        assert_eq!(Encoding::detect(&[0xEF, 0xBB, 0xBF, b'h']), Encoding::Utf8Bom);
        assert_eq!(Encoding::detect(&[0xFF, 0xFE, b'h', 0]), Encoding::Utf16Le);
        assert_eq!(Encoding::detect(&[0xFE, 0xFF, 0, b'h']), Encoding::Utf16Be);
        // UTF-16LE without a BOM: ASCII letters with a NUL in every high byte.
        assert_eq!(Encoding::detect(b"h\0e\0l\0l\0o\0"), Encoding::Utf16Le);
        // 0xE9 is "é" in Windows-1252 and invalid on its own in UTF-8.
        assert_eq!(Encoding::detect(&[b'c', b'a', b'f', 0xE9]), Encoding::Windows1252);
    }

    #[test]
    fn utf16_le_round_trip_preserves_characters() {
        // DA-AC-01: open a UTF-16 file without losing characters.
        let path = temp_file("utf16le", "txt");
        let original = "Café — naïve \u{1F600} ok\n";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in original.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();

        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.text(), original);
        assert_eq!(doc.encoding(), Encoding::Utf16Le);

        doc.insert(0, "X").unwrap();
        doc.save().unwrap();
        let reloaded = Document::load(&path).unwrap();
        assert_eq!(reloaded.text(), format!("X{original}"));
        assert_eq!(reloaded.encoding(), Encoding::Utf16Le, "encoding is preserved");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn utf16_be_round_trip_preserves_characters() {
        let path = temp_file("utf16be", "txt");
        let original = "Grüße €5";
        let mut bytes = vec![0xFE, 0xFF];
        for unit in original.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        fs::write(&path, &bytes).unwrap();
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.text(), original);
        let end = doc.char_count();
        doc.insert(end, "!").unwrap();
        doc.save().unwrap();
        assert_eq!(Document::load(&path).unwrap().text(), "Grüße €5!");
        assert_eq!(Document::load(&path).unwrap().encoding(), Encoding::Utf16Be);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn windows1252_round_trip_keeps_the_same_bytes() {
        // DA-AC-01: the classic Windows "ANSI" file.
        let path = temp_file("cp1252", "txt");
        let bytes = b"caf\xE9 \x80 5\r\n".to_vec(); // "café € 5"
        fs::write(&path, &bytes).unwrap();
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.text(), "café € 5\r\n");
        assert_eq!(doc.encoding(), Encoding::Windows1252);
        doc.replace_all("5", "6", true);
        doc.save().unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"caf\xE9 \x80 6\r\n".to_vec(),
            "saving keeps Windows-1252 bytes"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unrepresentable_characters_fall_back_to_utf8_instead_of_being_lost() {
        let path = temp_file("fallback", "txt");
        fs::write(&path, b"caf\xE9").unwrap();
        let mut doc = Document::load(&path).unwrap();
        let end = doc.char_count();
        doc.insert(end, " 日本語").unwrap();
        doc.save().unwrap();
        let reloaded = Document::load(&path).unwrap();
        assert_eq!(reloaded.text(), "café 日本語");
        assert_eq!(
            reloaded.encoding(),
            Encoding::Utf8,
            "a non-representable character promotes the file to UTF-8"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn utf8_bom_is_remembered_and_written_back() {
        let path = temp_file("bom", "txt");
        fs::write(&path, [0xEF, 0xBB, 0xBF, b'h', b'i']).unwrap();
        let doc = Document::load(&path).unwrap();
        assert_eq!(doc.text(), "hi", "BOM is not part of the text");
        assert_eq!(doc.encoding(), Encoding::Utf8Bom);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn save_and_load_round_trip_in_a_temp_dir() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("notes/ünïcode.txt");
        let mut doc = Document::from_text("Línea uno\nlínea dos ✓\n");
        assert!(doc.is_dirty() == false, "from_text starts clean");
        doc.set_text("Línea uno\nlínea dos ✓\n").unwrap();
        doc.insert(0, "> ").unwrap();
        assert!(doc.is_dirty());
        let saved = doc.save_as(&path).unwrap();
        assert_eq!(saved, path);
        assert!(!doc.is_dirty());
        assert_eq!(doc.name(), "ünïcode.txt");

        let loaded = Document::load(&path).unwrap();
        assert_eq!(loaded.text(), "> Línea uno\nlínea dos ✓\n");
        assert!(!loaded.is_dirty());
        assert_eq!(loaded.char_count(), 24);
        assert_eq!(loaded.path(), Some(path.as_path()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_rejects_invalid_utf8_and_oversized_files() {
        let dir = temp_dir("guards");
        let bad = dir.join("bad.txt");
        fs::write(&bad, [0xFF, 0xFE, 0x00]).unwrap();
        assert!(matches!(
            Document::load(&bad),
            Err(DocumentError::InvalidUtf8(_))
        ));
        let big = dir.join("big.txt");
        fs::write(&big, vec![b'a'; MAX_DOCUMENT_BYTES + 1]).unwrap();
        assert!(matches!(
            Document::load(&big),
            Err(DocumentError::TooLarge { .. })
        ));
        // A UTF-8 BOM is stripped on load.
        let bom = dir.join("bom.txt");
        fs::write(&bom, "\u{feff}hello").unwrap();
        assert_eq!(Document::load(&bom).unwrap().text(), "hello");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dirty_flag_tracks_the_saved_snapshot() {
        let dir = temp_dir("dirty");
        let path = dir.join("doc.txt");
        let mut doc = Document::from_text("draft");
        assert!(!doc.is_dirty());
        doc.insert(0, "# ").unwrap();
        assert!(doc.is_dirty());
        doc.save_as(&path).unwrap();
        assert!(!doc.is_dirty());
        doc.undo();
        assert_eq!(doc.text(), "draft");
        assert!(
            doc.is_dirty(),
            "undoing past the save marks the document dirty"
        );
        doc.mark_saved();
        assert!(!doc.is_dirty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_files_are_bounded_deduplicated_and_persisted() {
        let dir = temp_dir("recent");
        let store = RecentFiles::new(dir.join("recent.txt"), 3);
        let mut recent = store;
        assert!(recent.add("/tmp/a.md"));
        assert!(recent.add("/tmp/b.md"));
        assert!(recent.add("/tmp/c.md"));
        assert!(!recent.add("/tmp/a.md"), "existing entry is not new");
        assert_eq!(
            recent.entries(),
            vec![
                PathBuf::from("/tmp/a.md"),
                PathBuf::from("/tmp/c.md"),
                PathBuf::from("/tmp/b.md"),
            ]
        );
        assert!(recent.add("/tmp/d.md"));
        assert_eq!(recent.len(), 3, "the list is capped");
        assert_eq!(recent.entries()[0], PathBuf::from("/tmp/d.md"));
        recent.persist().unwrap();

        let mut reloaded = RecentFiles::new(dir.join("recent.txt"), 3);
        reloaded.load().unwrap();
        assert_eq!(reloaded.entries(), recent.entries());
        assert_eq!(reloaded.labels()[0].0, "d.md");
        assert!(reloaded.remove("/tmp/d.md"));
        assert_eq!(reloaded.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn documents_without_a_path_refuse_to_save_and_recent_loading_is_optional() {
        let mut doc = Document::from_text("draft");
        match doc.save() {
            Err(DocumentError::Io(message)) => {
                assert!(message.contains("no file path"), "{message}");
            }
            other => panic!("expected an Io error, got {other:?}"),
        }

        // A missing recent-files list is the first run, not an error.
        let dir = temp_dir("recent-missing");
        let mut recent = RecentFiles::new(dir.join("recent.txt"), 4);
        assert_eq!(recent.load(), Ok(()));
        assert!(recent.is_empty());
        // Nothing to persist yet stores an empty file, not an error.
        recent.persist().unwrap();
        assert_eq!(fs::read_to_string(recent.store_path()).unwrap(), "");
        assert!(autosave_path().ends_with("text/autosave.txt"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn short_path_only_rewrites_the_home_prefix() {
        let home = std::env::var("HOME").expect("HOME is set");
        let inside = Path::new(&home).join("Documents/notes.md");
        assert_eq!(short_path(&inside), "~/Documents/notes.md");
        let outside = Path::new("/aether/not-home/file.txt");
        assert_eq!(
            short_path(outside),
            "/aether/not-home/file.txt",
            "paths outside HOME are unchanged"
        );
        assert_eq!(Encoding::Windows1252.label(), "Windows-1252");
        assert_eq!(Encoding::Utf16Be.label(), "UTF-16 BE");
    }

    #[test]
    fn find_and_replace_ten_megabyte_file_within_a_second() {
        // DA-AC-02: find/replace across ~10 MB with no UI freeze over 1 s.
        let line = "lorem ipsum dolor sit amet 0123456789 aether\n"; // 44 bytes
        let repeats = (10 * 1024 * 1024) / line.len() + 1;
        let text = line.repeat(repeats);
        assert!(text.len() > 10 * 1024 * 1024);
        let mut doc = Document::from_text(text);
        assert_eq!(doc.byte_len(), repeats * line.len());

        // The 1 s budget is a release-build guarantee; a debug build runs the
        // same code 10-20x slower, so only correctness is asserted there.
        let budget = if cfg!(debug_assertions) {
            std::time::Duration::from_secs(60)
        } else {
            std::time::Duration::from_secs(1)
        };
        let start = std::time::Instant::now();
        let matches = doc.count_matches("ipsum", true);
        let counted_in = start.elapsed();
        assert_eq!(matches, repeats);
        println!("DA-AC-02 count_matches 10 MB: {counted_in:?} ({matches} matches)");
        assert!(counted_in < budget, "count took {counted_in:?}");

        let start = std::time::Instant::now();
        let replaced = doc.replace_all("ipsum", "IPSUM", true);
        let replaced_in = start.elapsed();
        assert_eq!(replaced, repeats);
        println!("DA-AC-02 replace_all 10 MB: {replaced_in:?} ({replaced} replacements)");
        assert!(replaced_in < budget, "replace all took {replaced_in:?}");
        assert_eq!(doc.count_matches("IPSUM", true), repeats);
        assert_eq!(doc.count_matches("ipsum", true), 0);
    }

    #[test]
    fn ten_megabyte_file_loads_and_saves_within_a_second() {        let path = temp_file("large", "txt");
        let line = "0123456789abcdefghijklmnopqrstuvwxyz\n"; // 37 bytes
        let body = line.repeat((10 * 1024 * 1024) / line.len() + 1);
        fs::write(&path, body.as_bytes()).unwrap();

        let start = std::time::Instant::now();
        let mut doc = Document::load(&path).unwrap();
        let loaded_in = start.elapsed();
        let budget = if cfg!(debug_assertions) {
            std::time::Duration::from_secs(60)
        } else {
            std::time::Duration::from_secs(1)
        };
        println!("DA-AC-02 load 10 MB: {loaded_in:?}");
        assert!(loaded_in < budget, "load took {loaded_in:?}");
        assert_eq!(doc.text().len(), body.len());

        let start = std::time::Instant::now();
        doc.save().unwrap();
        let saved_in = start.elapsed();
        println!("DA-AC-02 save 10 MB: {saved_in:?}");
        assert!(saved_in < budget, "save took {saved_in:?}");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

}
