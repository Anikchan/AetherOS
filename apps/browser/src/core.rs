//! Browser core: navigation, tab lifecycle, and profile data without any UI.
//!
//! `crate::lib` (the controller) is a thin Slint glue layer on top of this; all
//! rules that matter are tested here, including the private-window guarantee
//! that nothing is written to history or session.

use crate::store::{BrowserStore, DownloadState};
use crate::tabs::TabSet;
use crate::url::{display_url, parse_input, resolve, Navigation};

pub const HOME: &str = "about:home";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Bookmarks,
    History,
    Downloads,
}

impl SidebarSection {
    pub const ALL: [SidebarSection; 3] = [
        SidebarSection::Bookmarks,
        SidebarSection::History,
        SidebarSection::Downloads,
    ];

    pub fn index(self) -> i32 {
        match self {
            SidebarSection::Bookmarks => 0,
            SidebarSection::History => 1,
            SidebarSection::Downloads => 2,
        }
    }

    pub fn from_index(index: i32) -> SidebarSection {
        SidebarSection::ALL
            .get(index.max(0) as usize)
            .copied()
            .unwrap_or(SidebarSection::Bookmarks)
    }

    pub fn name(self) -> &'static str {
        match self {
            SidebarSection::Bookmarks => "bookmarks",
            SidebarSection::History => "history",
            SidebarSection::Downloads => "downloads",
        }
    }

    pub fn from_name(name: &str) -> SidebarSection {
        match name {
            "history" => SidebarSection::History,
            "downloads" => SidebarSection::Downloads,
            _ => SidebarSection::Bookmarks,
        }
    }
}

/// What the UI should do after a core action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    pub url: String,
    pub label: String,
    pub private: bool,
}

pub struct BrowserCore {
    pub tabs: TabSet,
    pub store: BrowserStore,
    pub sidebar: SidebarSection,
    pub status: String,
    /// Clock used for history timestamps; injected so tests are deterministic.
    pub now: u64,
}

impl BrowserCore {
    pub fn new(store: BrowserStore) -> BrowserCore {
        let mut tabs = TabSet::new(HOME);
        // Restore the previous session into non-private tabs.
        let session: Vec<String> = store.session().to_vec();
        if !session.is_empty() {
            tabs.tabs.clear();
            for url in session {
                tabs.open(false, &url);
            }
            tabs.active = 0;
        }
        BrowserCore {
            tabs,
            store,
            sidebar: SidebarSection::Bookmarks,
            status: "Ready".to_string(),
            now: 0,
        }
    }

    pub fn active(&self) -> &crate::tabs::Tab {
        self.tabs.active_tab()
    }

    pub fn address(&self) -> String {
        display_url(&self.active().url)
    }

    /// Navigate the active tab. Returns the URL the engine should load.
    pub fn navigate(&mut self, input: &str) -> Option<LoadRequest> {
        let navigation = parse_input(input)?;
        let url = resolve(&navigation);
        let label = match &navigation {
            Navigation::Search(query) => format!("Search: {query}"),
            Navigation::Url(_) => String::new(),
        };
        let private = self.active().private;
        self.tabs.active_tab_mut().commit(&url, &label);
        if !private {
            self.store.record_visit(&url, &label, self.now);
            self.persist_session();
        }
        self.status = if let Navigation::Search(query) = navigation {
            format!("Searching for “{query}”")
        } else {
            format!("Loaded {}", display_url(&url))
        };
        Some(LoadRequest {
            url,
            label,
            private,
        })
    }

    pub fn back(&mut self) -> Option<LoadRequest> {
        let url = self.tabs.active_tab_mut().go_back()?;
        Some(self.requestion(url))
    }

    pub fn forward(&mut self) -> Option<LoadRequest> {
        let url = self.tabs.active_tab_mut().go_forward()?;
        Some(self.requestion(url))
    }

    fn requestion(&mut self, url: String) -> LoadRequest {
        let private = self.active().private;
        LoadRequest {
            label: String::new(),
            url,
            private,
        }
    }

    pub fn open_tab(&mut self, private: bool) -> usize {
        let index = self.tabs.open(private, HOME);
        if !private {
            self.persist_session();
        }
        self.status = if private {
            "Private tab — history and cookies are not saved".to_string()
        } else {
            "New tab".to_string()
        };
        index
    }

    /// Close a tab. The last tab resets to home instead of closing the window.
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.close(index).is_none() {
            let tab = self.tabs.active_tab_mut();
            tab.url = HOME.to_string();
            tab.title = "New tab".to_string();
            tab.back.clear();
            tab.forward.clear();
        }
        self.persist_session();
    }

    pub fn toggle_bookmark(&mut self) -> bool {
        // Private tabs never touch the profile (P4-T18): a bookmark would leak
        // the private URL into bookmarks.json.
        if self.active().private {
            self.status = "Private tab — bookmarks are not saved".to_string();
            return false;
        }
        let (url, title) = {
            let tab = self.active();
            (tab.url.clone(), tab.title.clone())
        };
        if let Some(existing) = self
            .store
            .bookmarks()
            .iter()
            .find(|bookmark| bookmark.url == url)
        {
            let id = existing.id;
            self.store.remove_bookmark(id);
            let _ = self.store.save();
            return false;
        }
        self.store.add_bookmark(&url, &title, self.now);
        let _ = self.store.save();
        true
    }

    pub fn is_bookmarked(&self) -> bool {
        self.store.is_bookmarked(&self.active().url)
    }

    pub fn open_sidebar(&mut self, section: &str) {
        self.sidebar = SidebarSection::from_name(section);
    }

    /// Record a download. Private tabs keep the download out of the profile,
    /// so `None` means "not tracked" rather than "failed".
    pub fn start_download(&mut self, url: &str, file_name: &str) -> Option<u64> {
        if self.active().private {
            self.status = format!("Private tab — {file_name} is not added to downloads");
            return None;
        }
        let download = self.store.start_download(url, file_name);
        let _ = self.store.save();
        self.status = format!("Downloading {file_name}");
        Some(download.id)
    }

    pub fn finish_download(&mut self, id: u64, bytes: u64, ok: bool) {
        self.store.finish_download(
            id,
            bytes,
            if ok {
                DownloadState::Done
            } else {
                DownloadState::Failed
            },
        );
        let _ = self.store.save();
    }

    pub fn clear_history(&mut self) {
        self.store.clear_history();
        let _ = self.store.save();
        self.status = "History cleared".to_string();
    }

    pub fn persist_session(&mut self) {
        let urls: Vec<String> = self
            .tabs
            .tabs
            .iter()
            .filter(|tab| !tab.private && tab.url != HOME)
            .map(|tab| tab.url.clone())
            .collect();
        self.store.save_session(&urls);
        let _ = self.store.save();
    }

    pub fn save(&self) {
        let _ = self.store.save();
    }

    /// Number of rows the sidebar shows for the current section.
    pub fn sidebar_len(&self) -> usize {
        match self.sidebar {
            SidebarSection::Bookmarks => self.store.bookmarks().len(),
            SidebarSection::History => self.store.history().len(),
            SidebarSection::Downloads => self.store.downloads().len(),
        }
    }
}

/// Relative time label for history rows ("just now", "3 h ago", "2 d ago").
pub fn relative_time(now: u64, then: u64) -> String {
    if then == 0 {
        return "—".to_string();
    }
    let seconds = now.saturating_sub(then);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        86_400..=604_799 => format!("{} d ago", seconds / 86_400),
        _ => format!("{} mo ago", seconds / 2_592_000),
    }
}

/// Human-readable byte size for the downloads panel.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::BrowserStore;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "aether-browser-core-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn core(tag: &str) -> BrowserCore {
        let root = temp_root(tag);
        let _ = std::fs::remove_dir_all(&root);
        let mut core = BrowserCore::new(BrowserStore::open(&root));
        core.now = 1_700_000_000;
        core
    }

    #[test]
    fn navigating_a_url_records_history_and_session() {
        let mut core = core("navigate");
        let request = core.navigate("https://aether.example/docs").unwrap();
        assert_eq!(request.url, "https://aether.example/docs");
        assert_eq!(core.address(), "aether.example/docs");
        assert_eq!(core.store.history().len(), 1);
        assert_eq!(core.store.session(), ["https://aether.example/docs"]);
        let _ = std::fs::remove_dir_all(core.store.root().to_path_buf());
    }

    #[test]
    fn private_tabs_write_nothing() {
        let mut core = core("private");
        core.open_tab(true);
        core.navigate("https://secret.example/");
        assert!(core.store.history().is_empty(), "no history for private tabs");
        assert!(
            core.store.session().is_empty(),
            "private URLs never enter the session"
        );
        let _ = std::fs::remove_dir_all(core.store.root().to_path_buf());
    }

    #[test]
    fn private_tabs_never_bookmark_or_record_downloads() {
        let mut core = core("private-leak");
        core.open_tab(true);
        core.navigate("https://secret.example/private-page");

        assert!(!core.toggle_bookmark(), "private bookmark is refused");
        assert!(core.store.bookmarks().is_empty());
        assert!(!core.is_bookmarked());

        assert_eq!(
            core.start_download("https://secret.example/private-page/file.zip", "file.zip"),
            None,
            "private downloads are not recorded"
        );
        assert!(core.store.downloads().is_empty());

        // Nothing reached the persisted profile either.
        core.save();
        let reloaded = BrowserStore::open(core.store.root());
        assert!(reloaded.bookmarks().is_empty());
        assert!(reloaded.downloads().is_empty());
        assert!(reloaded.history().is_empty());
        assert!(reloaded.session().is_empty());
    }

    #[test]
    fn public_tabs_still_bookmark_and_download() {
        let mut core = core("public-ok");
        core.navigate("https://aether.example/docs");
        assert!(core.toggle_bookmark());
        let id = core
            .start_download("https://aether.example/os.iso", "os.iso")
            .expect("public download is tracked");
        core.finish_download(id, 1024, true);
        let reloaded = BrowserStore::open(core.store.root());
        assert_eq!(reloaded.bookmarks().len(), 1);
        assert_eq!(reloaded.downloads().len(), 1);
    }

    #[test]
    fn search_queries_use_the_engine_endpoint() {
        let mut core = core("search");
        let request = core.navigate("slint layout").unwrap();
        assert!(request.url.starts_with("https://duckduckgo.com/html/?q="));
        assert_eq!(core.store.history()[0].title, "Search: slint layout");
        let _ = std::fs::remove_dir_all(core.store.root().to_path_buf());
    }

    #[test]
    fn back_and_forward_return_load_requests() {
        let mut core = core("history-nav");
        core.navigate("https://a.example/");
        core.navigate("https://b.example/");
        assert_eq!(core.back().unwrap().url, "https://a.example/");
        assert_eq!(core.forward().unwrap().url, "https://b.example/");
        assert!(core.forward().is_none());
        let _ = std::fs::remove_dir_all(core.store.root().to_path_buf());
    }

    #[test]
    fn bookmarking_toggles_and_persists() {
        let mut core = core("bookmark");
        core.navigate("https://aether.example/");
        assert!(core.toggle_bookmark());
        assert!(core.is_bookmarked());
        assert_eq!(core.store.bookmarks().len(), 1);
        assert!(!core.toggle_bookmark());
        assert!(!core.is_bookmarked());
        let _ = std::fs::remove_dir_all(core.store.root().to_path_buf());
    }

    #[test]
    fn closing_the_last_tab_resets_to_home() {
        let mut core = core("close");
        core.navigate("https://a.example/");
        core.close_tab(0);
        assert_eq!(core.tabs.tabs.len(), 1);
        assert_eq!(core.active().url, HOME);
        let _ = std::fs::remove_dir_all(core.store.root().to_path_buf());
    }

    #[test]
    fn session_restore_reopens_previous_urls() {
        let root = temp_root("restore");
        {
            let mut store = BrowserStore::open(&root);
            store.save_session(&["https://a.example/".into(), "https://b.example/".into()]);
            store.save().unwrap();
        }
        let core = BrowserCore::new(BrowserStore::open(&root));
        assert_eq!(core.tabs.tabs.len(), 2);
        assert_eq!(core.tabs.tabs[0].url, "https://a.example/");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sidebar_sections_round_trip() {
        for section in SidebarSection::ALL {
            assert_eq!(SidebarSection::from_index(section.index()), section);
            assert_eq!(SidebarSection::from_name(section.name()), section);
        }
        assert_eq!(SidebarSection::from_index(99), SidebarSection::Bookmarks);
    }

    #[test]
    fn relative_time_and_bytes_formatting() {
        assert_eq!(relative_time(1000, 1000), "just now");
        assert_eq!(relative_time(3700, 1000), "45 min ago");
        assert_eq!(relative_time(10_000, 1000), "2 h ago");
        assert_eq!(relative_time(200_000, 1000), "2 d ago");
        assert_eq!(relative_time(10, 0), "—");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
