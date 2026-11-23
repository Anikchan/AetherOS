//! Browser data: bookmarks, history, downloads and session restore (P4-T18b).
//!
//! One JSON document per user profile, written atomically. Private tabs are
//! never passed to these functions, which is what makes private browsing
//! actually private: the controller simply skips recording for them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub folder: String,
    pub added_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryItem {
    pub url: String,
    pub title: String,
    pub visited_at: u64,
    pub visits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub id: u64,
    pub url: String,
    pub file_name: String,
    pub bytes: u64,
    pub state: DownloadState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadState {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl DownloadState {
    pub fn label(self) -> &'static str {
        match self {
            DownloadState::Running => "Downloading",
            DownloadState::Done => "Done",
            DownloadState::Failed => "Failed",
            DownloadState::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoreData {
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryItem>,
    pub downloads: Vec<Download>,
    pub session: Vec<String>,
    #[serde(default)]
    next_id: u64,
}

const HISTORY_CAP: usize = 500;

/// User-profile store rooted at `~/.config/aether/browser`.
#[derive(Debug, Clone)]
pub struct BrowserStore {
    root: PathBuf,
    data: StoreData,
}

impl BrowserStore {
    pub fn open(root: impl Into<PathBuf>) -> BrowserStore {
        let root = root.into();
        let data = std::fs::read_to_string(root.join("profile.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let mut store = BrowserStore { root, data };
        if store.data.next_id == 0 {
            store.data.next_id = 1;
        }
        store
    }

    pub fn at_default_location() -> BrowserStore {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(&home).join(".config"));
        BrowserStore::open(base.join("aether").join("browser"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data(&self) -> &StoreData {
        &self.data
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.root)
            .map_err(|error| format!("cannot create {}: {error}", self.root.display()))?;
        let text = serde_json::to_string_pretty(&self.data)
            .map_err(|error| format!("serialise profile: {error}"))?;
        let path = self.root.join("profile.json");
        let temp = self.root.join("profile.json.tmp");
        std::fs::write(&temp, text).map_err(|error| format!("write {}: {error}", temp.display()))?;
        std::fs::rename(&temp, &path).map_err(|error| format!("replace {}: {error}", path.display()))?;
        Ok(())
    }

    fn next_id(&mut self) -> u64 {
        let id = self.data.next_id;
        self.data.next_id += 1;
        id
    }

    // ------------------------------------------------------------ bookmarks ---

    pub fn add_bookmark(&mut self, url: &str, title: &str, now: u64) -> Bookmark {
        if let Some(existing) = self.data.bookmarks.iter_mut().find(|b| b.url == url) {
            existing.title = title.to_string();
            existing.added_at = now;
            return existing.clone();
        }
        let bookmark = Bookmark {
            id: self.next_id(),
            url: url.to_string(),
            title: title.to_string(),
            folder: "Bookmarks".to_string(),
            added_at: now,
        };
        self.data.bookmarks.insert(0, bookmark.clone());
        bookmark
    }

    pub fn remove_bookmark(&mut self, id: u64) -> bool {
        let before = self.data.bookmarks.len();
        self.data.bookmarks.retain(|b| b.id != id);
        self.data.bookmarks.len() != before
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.data.bookmarks.iter().any(|b| b.url == url)
    }

    pub fn bookmarks(&self) -> &[Bookmark] {
        &self.data.bookmarks
    }

    // -------------------------------------------------------------- history ---

    /// Record a visit. Visits on the same day de-duplicate into one row.
    pub fn record_visit(&mut self, url: &str, title: &str, now: u64) {
        let day = now / 86_400;
        if let Some(index) = self
            .data
            .history
            .iter()
            .position(|item| item.url == url && item.visited_at / 86_400 == day)
        {
            let mut item = self.data.history.remove(index);
            item.visits += 1;
            item.visited_at = now;
            if !title.trim().is_empty() {
                item.title = title.to_string();
            }
            self.data.history.insert(0, item);
        } else {
            self.data.history.insert(
                0,
                HistoryItem {
                    url: url.to_string(),
                    title: if title.trim().is_empty() {
                        url.to_string()
                    } else {
                        title.to_string()
                    },
                    visited_at: now,
                    visits: 1,
                },
            );
        }
        self.data.history.truncate(HISTORY_CAP);
    }

    pub fn history(&self) -> &[HistoryItem] {
        &self.data.history
    }

    pub fn search_history(&self, query: &str, limit: usize) -> Vec<&HistoryItem> {
        let query = query.to_lowercase();
        self.data
            .history
            .iter()
            .filter(|item| {
                query.is_empty()
                    || item.url.to_lowercase().contains(&query)
                    || item.title.to_lowercase().contains(&query)
            })
            .take(limit)
            .collect()
    }

    pub fn clear_history(&mut self) {
        self.data.history.clear();
    }

    /// Most visited hosts, for the "frequent" section.
    pub fn top_hosts(&self, limit: usize) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for item in &self.data.history {
            let host = item
                .url
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or(&item.url)
                .to_string();
            *totals.entry(host).or_insert(0) += item.visits;
        }
        let mut rows: Vec<(String, u32)> = totals.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rows.truncate(limit);
        rows
    }

    // ------------------------------------------------------------ downloads ---

    pub fn start_download(&mut self, url: &str, file_name: &str) -> Download {
        let download = Download {
            id: self.next_id(),
            url: url.to_string(),
            file_name: file_name.to_string(),
            bytes: 0,
            state: DownloadState::Running,
        };
        self.data.downloads.insert(0, download.clone());
        download
    }

    pub fn finish_download(&mut self, id: u64, bytes: u64, state: DownloadState) -> bool {
        if let Some(download) = self.data.downloads.iter_mut().find(|d| d.id == id) {
            download.bytes = bytes;
            download.state = state;
            true
        } else {
            false
        }
    }

    pub fn downloads(&self) -> &[Download] {
        &self.data.downloads
    }

    /// Replace the download list (used when the user removes entries).
    pub fn replace_downloads(&mut self, downloads: Vec<Download>) {
        self.data.downloads = downloads;
    }

    // -------------------------------------------------------------- session ---

    /// Persist the URLs of non-private tabs for session restore.
    pub fn save_session(&mut self, urls: &[String]) {
        self.data.session = urls.to_vec();
    }

    pub fn session(&self) -> &[String] {
        &self.data.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aether-browser-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn bookmarks_are_deduplicated_and_persisted() {
        let root = temp_root("bookmarks");
        let mut store = BrowserStore::open(&root);
        store.add_bookmark("https://a.example/", "A", 1);
        store.add_bookmark("https://a.example/", "A new title", 2);
        assert_eq!(store.bookmarks().len(), 1);
        assert_eq!(store.bookmarks()[0].title, "A new title");
        assert!(store.is_bookmarked("https://a.example/"));
        store.save().unwrap();
        let mut reloaded = BrowserStore::open(&root);
        assert_eq!(reloaded.bookmarks().len(), 1);
        let id = reloaded.bookmarks()[0].id;
        assert!(reloaded.remove_bookmark(id));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn removing_a_bookmark_reports_whether_it_existed() {
        let root = temp_root("remove");
        let mut store = BrowserStore::open(&root);
        let bookmark = store.add_bookmark("https://a.example/", "A", 1);
        assert!(store.remove_bookmark(bookmark.id));
        assert!(!store.remove_bookmark(bookmark.id));
    }

    #[test]
    fn same_day_visits_merge_into_one_history_row() {
        let root = temp_root("history");
        let mut store = BrowserStore::open(&root);
        let day = 1_700_000_000;
        store.record_visit("https://a.example/", "A", day);
        store.record_visit("https://a.example/", "A", day + 60);
        store.record_visit("https://b.example/", "B", day + 120);
        assert_eq!(store.history().len(), 2);
        assert_eq!(store.history()[0].visits, 1);
        let a = store
            .history()
            .iter()
            .find(|item| item.url == "https://a.example/")
            .unwrap();
        assert_eq!(a.visits, 2);
    }

    #[test]
    fn history_search_matches_url_and_title() {
        let root = temp_root("search");
        let mut store = BrowserStore::open(&root);
        store.record_visit("https://rust-lang.org/", "Rust", 1);
        store.record_visit("https://slint.dev/", "Slint", 2);
        assert_eq!(store.search_history("slint", 10).len(), 1);
        assert_eq!(store.search_history("rust-lang", 10)[0].title, "Rust");
        assert_eq!(store.search_history("", 10).len(), 2);
        assert!(store.search_history("nothing", 10).is_empty());
    }

    #[test]
    fn history_is_capped() {
        let root = temp_root("cap");
        let mut store = BrowserStore::open(&root);
        for index in 0..(HISTORY_CAP + 20) {
            store.record_visit(&format!("https://h{index}.example/"), "", index as u64);
        }
        assert_eq!(store.history().len(), HISTORY_CAP);
    }

    #[test]
    fn downloads_transition_and_persist() {
        let root = temp_root("downloads");
        let mut store = BrowserStore::open(&root);
        let download = store.start_download("https://a.example/f.iso", "f.iso");
        assert_eq!(download.state, DownloadState::Running);
        assert!(store.finish_download(download.id, 1024, DownloadState::Done));
        store.save().unwrap();
        let reloaded = BrowserStore::open(&root);
        assert_eq!(reloaded.downloads().len(), 1);
        assert_eq!(reloaded.downloads()[0].bytes, 1024);
        assert_eq!(reloaded.downloads()[0].state.label(), "Done");
        let mut other = reloaded.clone();
        assert!(!other.finish_download(999, 0, DownloadState::Failed));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn session_round_trips_through_disk() {
        let root = temp_root("session");
        let mut store = BrowserStore::open(&root);
        store.save_session(&["https://a.example/".into(), "https://b.example/".into()]);
        store.save().unwrap();
        let reloaded = BrowserStore::open(&root);
        assert_eq!(reloaded.session().len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn top_hosts_ranks_by_visits() {
        let root = temp_root("hosts");
        let mut store = BrowserStore::open(&root);
        for index in 0..3 {
            store.record_visit("https://a.example/x", "A", 1 + index);
        }
        store.record_visit("https://b.example/", "B", 10);
        let hosts = store.top_hosts(2);
        assert_eq!(hosts[0].0, "a.example");
        assert_eq!(hosts[0].1, 3);
        assert_eq!(hosts[1].0, "b.example");
    }
}
