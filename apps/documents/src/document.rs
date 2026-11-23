//! Document model for the Documents app (P4-T13, DA-AC-05).
//!
//! A document is a Markdown/text file plus the pagination, zoom and search
//! state the viewer needs. Pagination is by "line budget": each block costs a
//! number of rendered lines and a page holds [`DEFAULT_LINES_PER_PAGE`] of
//! them, so pages stay roughly screen-sized without measuring fonts.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::markdown::{self, Block};

pub const DEFAULT_LINES_PER_PAGE: usize = 48;
pub const MIN_FONT_SIZE: f32 = 0.75;
pub const MAX_FONT_SIZE: f32 = 1.9;
pub const FONT_STEP: f32 = 0.15;
const PREVIEW_CHARS: usize = 92;

/// One search match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Zero-based page that contains the match.
    pub page: usize,
    /// Zero-based block index.
    pub block: usize,
    /// Trimmed text around the match.
    pub preview: String,
}

/// An opened document (or an in-memory one, used by the screenshot renderer).
#[derive(Debug, Clone)]
pub struct Document {
    path: PathBuf,
    title: String,
    text: String,
    blocks: Vec<Block>,
    pages: Vec<Range<usize>>,
    page: usize,
    lines_per_page: usize,
    font_size: f32,
}

impl Document {
    /// Open a text/Markdown file from disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Document, String> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Document::build(path.to_path_buf(), title, text))
    }

    /// In-memory document, e.g. the sample note used in screenshots.
    pub fn from_text(title: impl Into<String>, text: impl Into<String>) -> Document {
        let title = title.into();
        Document::build(PathBuf::from(&title), title, text.into())
    }

    fn build(path: PathBuf, title: String, text: String) -> Document {
        let blocks = markdown::parse(&text);
        let pages = paginate(&blocks, DEFAULT_LINES_PER_PAGE);
        Document {
            path,
            title,
            text,
            blocks,
            pages,
            page: 0,
            lines_per_page: DEFAULT_LINES_PER_PAGE,
            font_size: 1.0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn lines_per_page(&self) -> usize {
        self.lines_per_page
    }

    /// Change the page budget (the UI keeps the default).
    pub fn set_lines_per_page(&mut self, lines: usize) {
        self.lines_per_page = lines.max(4);
        self.pages = paginate(&self.blocks, self.lines_per_page);
        self.page = self.page.min(self.pages.len() - 1);
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn page_index(&self) -> usize {
        self.page
    }

    /// One-based page number for the toolbar (`page 3 of 9`).
    pub fn page_number(&self) -> usize {
        self.page + 1
    }

    pub fn page_blocks(&self) -> &[Block] {
        let range = &self.pages[self.page];
        &self.blocks[range.clone()]
    }

    /// Block ranges of every page (used for page thumbnails).
    pub fn pages(&self) -> &[Range<usize>] {
        &self.pages
    }

    /// Jump to a page; returns whether the page changed.
    pub fn set_page(&mut self, page: usize) -> bool {
        let target = page.min(self.pages.len() - 1);
        if target == self.page {
            return false;
        }
        self.page = target;
        true
    }

    pub fn next_page(&mut self) -> bool {
        self.set_page(self.page + 1)
    }

    pub fn prev_page(&mut self) -> bool {
        self.set_page(self.page.saturating_sub(1))
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn zoom_percent(&self) -> i32 {
        (self.font_size * 100.0).round() as i32
    }

    /// One zoom step up, clamped at [`MAX_FONT_SIZE`].
    pub fn zoom_in(&mut self) -> f32 {
        self.font_size = (self.font_size + FONT_STEP).min(MAX_FONT_SIZE);
        self.font_size
    }

    /// One zoom step down, clamped at [`MIN_FONT_SIZE`].
    pub fn zoom_out(&mut self) -> f32 {
        self.font_size = (self.font_size - FONT_STEP).max(MIN_FONT_SIZE);
        self.font_size
    }

    pub fn reset_zoom(&mut self) {
        self.font_size = 1.0;
    }

    /// Case-insensitive search across blocks, with the page each match is on.
    pub fn find(&self, query: &str) -> Vec<SearchHit> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            let text = block.text();
            if text.to_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    page: self.page_of_block(index),
                    block: index,
                    preview: preview(&text, &needle),
                });
            }
        }
        hits
    }

    pub fn match_count(&self, query: &str) -> usize {
        self.find(query).len()
    }

    fn page_of_block(&self, block: usize) -> usize {
        self.pages
            .iter()
            .position(|range| range.contains(&block))
            .unwrap_or(0)
    }
}

/// Path as shown in the UI: `~/...` under the home directory, otherwise the
/// last two components with a leading ellipsis.
pub fn short_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            let rest = rest.display().to_string();
            return if rest.is_empty() {
                "~".to_string()
            } else {
                format!("~/{rest}")
            };
        }
    }
    let components: Vec<_> = path.components().collect();
    if components.len() > 2 {
        let tail: PathBuf = components[components.len() - 2..].iter().collect();
        format!("…/{}", tail.display())
    } else {
        path.display().to_string()
    }
}

/// Short, single-line excerpt around the first match.
fn preview(text: &str, needle: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = collapsed.to_lowercase();
    if collapsed.chars().count() <= PREVIEW_CHARS {
        return collapsed;
    }
    let start = lowered.find(needle).unwrap_or(0);
    // Keep the window on a character boundary.
    let mut offset = start.saturating_sub(24);
    while offset > 0 && !collapsed.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut end = (offset + PREVIEW_CHARS).min(collapsed.len());
    while end < collapsed.len() && !collapsed.is_char_boundary(end) {
        end += 1;
    }
    let excerpt = collapsed[offset..end].trim().to_string();
    let lead = if offset > 0 { "…" } else { "" };
    let tail = if end < collapsed.len() { "…" } else { "" };
    format!("{lead}{excerpt}{tail}")
}

/// Estimated rendered lines per block, used by the page budget.
pub fn estimated_lines(block: &Block) -> usize {
    const CHARS_PER_LINE: usize = 78;
    match block {
        Block::Heading { .. } => 3,
        Block::Paragraph { .. } => block.text().len() / CHARS_PER_LINE + 2,
        Block::Bullet { .. } | Block::Numbered { .. } => {
            block.text().len() / CHARS_PER_LINE + 2
        }
        Block::Code { lines, .. } => lines.len().max(1) + 2,
        Block::Quote { .. } => block.text().len() / CHARS_PER_LINE + 3,
        Block::Divider => 1,
    }
}

/// Split block indices into pages of roughly `budget` rendered lines. Always
/// returns at least one page so the viewer never divides by zero.
pub fn paginate(blocks: &[Block], budget: usize) -> Vec<Range<usize>> {
    let budget = budget.max(1);
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, block) in blocks.iter().enumerate() {
        let cost = estimated_lines(block);
        if used > 0 && used + cost > budget {
            pages.push(start..index);
            start = index;
            used = 0;
        }
        used += cost;
    }
    pages.push(start..blocks.len());
    pages
}

/// Recently opened documents, one path per line (no extra dependency needed).
#[derive(Debug, Clone)]
pub struct RecentFiles {
    path: PathBuf,
    entries: Vec<PathBuf>,
    limit: usize,
}

impl RecentFiles {
    pub fn new(path: impl Into<PathBuf>) -> RecentFiles {
        RecentFiles {
            path: path.into(),
            entries: Vec::new(),
            limit: 8,
        }
    }

    /// `$XDG_CONFIG_HOME/aether/documents/recent.txt` (or `~/.config/...`).
    pub fn at_config_dir() -> RecentFiles {
        let root = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.is_empty() => PathBuf::from(value),
            _ => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                Path::new(&home).join(".config")
            }
        };
        RecentFiles::new(
            root.join("aether")
                .join("documents")
                .join("recent.txt"),
        )
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn entries(&self) -> &[PathBuf] {
        &self.entries
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Read the list; a missing file is an empty list, not an error.
    pub fn load(&mut self) -> Result<(), String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                self.entries = text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(PathBuf::from)
                    .take(self.limit)
                    .collect();
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.entries.clear();
                Ok(())
            }
            Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let text = self
            .entries
            .iter()
            .map(|entry| entry.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&self.path, text)
            .map_err(|error| format!("cannot write {}: {error}", self.path.display()))
    }

    /// Move a path to the front, de-duplicated and capped at the limit.
    pub fn push(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.entries.retain(|entry| entry != &path);
        self.entries.insert(0, path);
        self.entries.truncate(self.limit);
    }

    /// File name (or full path) as shown in the sidebar.
    pub fn display_name(entry: &Path) -> String {
        entry
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| entry.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_document() -> Document {
        let mut text = String::from("# Quarterly notes\n\n");
        for index in 1..=12 {
            text.push_str(&format!("## Section {index}\n\n"));
            for _ in 0..4 {
                text.push_str("A paragraph of body copy that is long enough to wrap over two rendered lines in the viewer.\n\n");
            }
        }
        Document::from_text("notes.md", text)
    }

    fn temp_recent(tag: &str) -> RecentFiles {
        let base = std::env::temp_dir().join(format!(
            "aether-documents-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        RecentFiles::new(base.join("documents").join("recent.txt"))
    }

    #[test]
    fn pagination_respects_the_line_budget_and_keeps_every_block() {
        let document = long_document();
        assert!(document.page_count() >= 4, "the sample is a multi-page document");
        let mut total = 0;
        for page in 0..document.page_count() {
            let range = document.pages[page].clone();
            let lines: usize = document.blocks[range.clone()]
                .iter()
                .map(estimated_lines)
                .sum();
            assert!(
                lines <= document.lines_per_page(),
                "page {page} holds {lines} lines, budget {}",
                document.lines_per_page()
            );
            total += range.len();
        }
        assert_eq!(total, document.blocks().len(), "every block lands on a page");

        // A bigger budget needs fewer pages, and the same blocks survive.
        let mut roomy = document.clone();
        roomy.set_lines_per_page(400);
        assert!(roomy.page_count() < document.page_count());
        assert_eq!(roomy.page_blocks().len(), roomy.blocks().len());
    }

    #[test]
    fn page_navigation_clamps_at_both_ends() {
        let mut document = long_document();
        assert_eq!(document.page_index(), 0);
        assert!(!document.prev_page(), "already on the first page");
        assert!(document.next_page());
        assert_eq!(document.page_number(), 2);
        assert_eq!(document.page_index(), 1);
        assert!(document.set_page(document.page_count() - 1));
        assert!(!document.next_page(), "already on the last page");
        assert_eq!(document.page_number(), document.page_count());
        assert!(!document.set_page(9999), "out-of-range pages do not change state");
        assert_eq!(document.page_index(), document.page_count() - 1);
        assert!(document.set_page(0));
        assert_eq!(document.page_number(), 1);

        // Each page shows a different slice of the document.
        let first = document.page_blocks().to_vec();
        document.set_page(1);
        let second = document.page_blocks().to_vec();
        assert_ne!(first, second);
    }

    #[test]
    fn zoom_steps_up_and_down_within_bounds() {
        let mut document = Document::from_text("note.md", "# Title\n\nBody");
        assert_eq!(document.zoom_percent(), 100);
        document.zoom_in();
        assert_eq!(document.zoom_percent(), 115);
        document.zoom_out();
        document.zoom_out();
        assert_eq!(document.zoom_percent(), 85);
        for _ in 0..40 {
            document.zoom_out();
        }
        assert!((document.font_size() - MIN_FONT_SIZE).abs() < f32::EPSILON);
        for _ in 0..40 {
            document.zoom_in();
        }
        assert!((document.font_size() - MAX_FONT_SIZE).abs() < f32::EPSILON);
        document.reset_zoom();
        assert_eq!(document.zoom_percent(), 100);
    }

    #[test]
    fn search_counts_matches_and_reports_their_pages() {
        let document = long_document();
        let hits = document.find("section 5");
        assert_eq!(hits.len(), 1, "one heading mentions section 5");
        assert_eq!(hits[0].preview, "Section 5");
        assert!(hits[0].page < document.page_count());

        let body = document.find("body copy");
        assert_eq!(body.len(), 48, "twelve sections of four paragraphs");
        assert!(body.iter().any(|hit| hit.page > 0), "hits span several pages");

        assert!(document.find("   ").is_empty());
        assert!(document.find("no-such-word").is_empty());
        assert_eq!(document.match_count("body copy"), 48);
        assert!(Document::from_text("t", "x".repeat(300)).find("x")[0]
            .preview
            .ends_with('…'));
    }

    #[test]
    fn recent_files_dedupe_cap_and_round_trip() {
        let mut recent = temp_recent("recent");
        recent.load().unwrap();
        assert!(recent.entries().is_empty());

        for index in 0..10 {
            recent.push(format!("/tmp/note-{index}.md"));
        }
        assert_eq!(recent.entries().len(), 8, "capped at the limit");
        assert_eq!(
            recent.entries()[0],
            PathBuf::from("/tmp/note-9.md"),
            "newest first"
        );

        recent.push("/tmp/note-3.md");
        assert_eq!(recent.entries()[0], PathBuf::from("/tmp/note-3.md"));
        assert_eq!(
            recent.entries().iter().filter(|entry| entry.ends_with("note-3.md")).count(),
            1,
            "duplicates are removed"
        );

        recent.save().unwrap();
        let mut reloaded = RecentFiles::new(recent.path().to_path_buf());
        reloaded.load().unwrap();
        assert_eq!(reloaded.entries(), recent.entries());

        let _ = fs::remove_dir_all(recent.path().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn opening_a_missing_file_reports_the_path() {
        let error = Document::open("/definitely/not/here/doc.md").unwrap_err();
        assert!(error.contains("cannot open"), "got: {error}");
        assert!(error.contains("doc.md"));
    }
}
