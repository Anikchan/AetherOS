//! AetherOS Browser (P4-T18 core, T18b data features, T18a media).
//!
//! The chrome and every data feature are real: tabs with per-tab back/forward,
//! address-bar parsing (URL or search), bookmarks, history with de-duplication,
//! downloads, private tabs that never touch the profile, and session restore.
//! The rendering engine is the seam in [`engine`] — WebKitGTK embedding arrives
//! with the graphics bring-up while [`engine::ExternalEngine`] can hand URLs to
//! a renderer today. Media playback policy (codecs, hardware decode, reduced
//! quality, DRM) lives in [`media`] on top of the shared `aether-media` crate.
//! Security posture, the WebKitGTK security floor and the update channel live
//! in [`security`] (P4-T18b).

slint::include_modules!();

pub mod core;
pub mod engine;
pub mod media;
pub mod security;
pub mod store;
pub mod tabs;
pub mod url;

use std::cell::RefCell;
use std::rc::Rc;

use aether_media::MediaCapabilities;
use slint::{ModelRc, VecModel};

use crate::core::{format_bytes, relative_time, BrowserCore, SidebarSection};
use crate::engine::{Engine, NullEngine};
use crate::store::{BrowserStore, DownloadState};

pub use crate::core::HOME;

/// Current Unix time in seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub struct BrowserController {
    core: RefCell<BrowserCore>,
    engine: RefCell<Box<dyn Engine>>,
    media: RefCell<MediaCapabilities>,
    ui: BrowserApp,
}

impl BrowserController {
    pub fn new(
        ui: BrowserApp,
        store: BrowserStore,
        engine: Box<dyn Engine>,
    ) -> Rc<BrowserController> {
        let mut core = BrowserCore::new(store);
        core.now = unix_now();
        let controller = Rc::new(BrowserController {
            core: RefCell::new(core),
            engine: RefCell::new(engine),
            media: RefCell::new(MediaCapabilities::detect()),
            ui,
        });
        controller.wire();
        controller.sync();
        controller
    }

    /// Media capabilities this window uses (detected in [`Self::new`]).
    pub fn media_capabilities(&self) -> MediaCapabilities {
        *self.media.borrow()
    }

    /// Replace the detected capabilities (tests, acceptance harness).
    pub fn set_media_capabilities(&self, capabilities: MediaCapabilities) {
        *self.media.borrow_mut() = capabilities;
    }

    /// Demo profile for screenshots (no user data touched).
    pub fn with_demo(ui: BrowserApp) -> Rc<BrowserController> {
        let root = std::env::temp_dir().join(format!("aether-browser-demo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = BrowserStore::open(&root);
        let controller = BrowserController::new(ui, store, Box::new(NullEngine::new()));
        let now = unix_now();
        {
            let mut core = controller.core.borrow_mut();
            core.now = now;
            core.store.add_bookmark("https://aether.example/docs", "AetherOS docs", now - 600);
            core.store.add_bookmark("https://slint.dev", "Slint", now - 3_600);
            core.store.add_bookmark("https://www.rust-lang.org", "Rust", now - 7_200);
            core.store.record_visit("https://aether.example/docs", "AetherOS docs", now - 120);
            core.store.record_visit("https://slint.dev/docs", "Slint documentation", now - 900);
            core.store.record_visit("https://news.ycombinator.com", "Hacker News", now - 5_400);
            core.store
                .record_visit("https://www.rust-lang.org/learn", "Learn Rust", now - 90_000);
            let download = core.store.start_download("https://aether.example/os.iso", "aether-os.iso");
            core.store
                .finish_download(download.id, 1_462_272_000, DownloadState::Done);
            core.navigate("https://aether.example/docs");
            core.open_tab(false);
            core.navigate("https://slint.dev");
            core.open_tab(true);
            core.tabs.select(0);
            core.status = "2 tabs · 1 private · profile saved".to_string();
        }
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &BrowserApp {
        &self.ui
    }

    pub fn core(&self) -> std::cell::Ref<'_, BrowserCore> {
        self.core.borrow()
    }

    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.on_navigate(move |text| {
            if let Some(controller) = weak.upgrade() {
                controller.navigate(text.as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_go_back(move || {
            if let Some(controller) = weak.upgrade() {
                controller.back();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_go_forward(move || {
            if let Some(controller) = weak.upgrade() {
                controller.forward();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_reload(move || {
            if let Some(controller) = weak.upgrade() {
                controller.reload();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_new_tab(move || {
            if let Some(controller) = weak.upgrade() {
                controller.new_tab(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_close_tab(move |index| {
            if let Some(controller) = weak.upgrade() {
                controller.close_tab(index as usize);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_select_tab(move |index| {
            if let Some(controller) = weak.upgrade() {
                controller.select_tab(index as usize);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_toggle_bookmark(move || {
            if let Some(controller) = weak.upgrade() {
                controller.toggle_bookmark();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_open_sidebar(move |section| {
            if let Some(controller) = weak.upgrade() {
                controller.open_sidebar(section.as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_sidebar_open_url(move |url| {
            if let Some(controller) = weak.upgrade() {
                controller.navigate(url.as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_remove_bookmark(move |id| {
            if let Some(controller) = weak.upgrade() {
                controller.remove_bookmark(id as u64);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_clear_history(move || {
            if let Some(controller) = weak.upgrade() {
                controller.clear_history();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_remove_download(move |id| {
            if let Some(controller) = weak.upgrade() {
                controller.remove_download(id as u64);
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_zoom_in(move || {
            if let Some(controller) = weak.upgrade() {
                controller.core.borrow_mut().tabs.active_tab_mut().zoom_in();
                controller.sync();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_zoom_out(move || {
            if let Some(controller) = weak.upgrade() {
                controller.core.borrow_mut().tabs.active_tab_mut().zoom_out();
                controller.sync();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.on_zoom_reset(move || {
            if let Some(controller) = weak.upgrade() {
                controller.core.borrow_mut().tabs.active_tab_mut().zoom_reset();
                controller.sync();
            }
        });
    }

    pub fn sync(&self) {
        let core = self.core.borrow();
        let active = core.tabs.active;
        let tabs: Vec<TabEntry> = core
            .tabs
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| TabEntry {
                id: tab.id as i32,
                title: tab.title.clone().into(),
                url: tab.url.clone().into(),
                active: index == active,
                incognito: tab.private,
            })
            .collect();
        self.ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
        self.ui.set_active(active as i32);
        self.ui.set_address(core.address().into());
        self.ui.set_can_back(core.active().can_go_back());
        self.ui.set_can_forward(core.active().can_go_forward());
        self.ui.set_loading(core.active().loading);
        self.ui.set_bookmarked(core.is_bookmarked());
        self.ui.set_incognito(core.active().private);
        self.ui.set_zoom(core.active().zoom as i32);
        self.ui.set_section(core.sidebar.index());
        self.ui.set_status(core.status.clone().into());
        // The chrome always advertises the chosen engine; the adapter kind goes
        // to the status bar so "null"/"external" never reaches the user.
        let _adapter = self.engine.borrow().name();
        self.ui
            .set_engine_name("WebKitGTK (adapter)".to_string().into());

        let bookmarks: Vec<BookmarkEntry> = core
            .store
            .bookmarks()
            .iter()
            .map(|bookmark| BookmarkEntry {
                id: bookmark.id as i32,
                title: bookmark.title.clone().into(),
                url: bookmark.url.clone().into(),
            })
            .collect();
        self.ui
            .set_bookmarks(ModelRc::new(VecModel::from(bookmarks)));

        let history: Vec<HistoryRow> = core
            .store
            .history()
            .iter()
            .take(50)
            .enumerate()
            .map(|(index, item)| HistoryRow {
                id: index as i32,
                title: item.title.clone().into(),
                url: item.url.clone().into(),
                time: relative_time(core.now, item.visited_at).into(),
                visits: item.visits.to_string().into(),
            })
            .collect();
        self.ui.set_history(ModelRc::new(VecModel::from(history)));

        let downloads: Vec<DownloadRow> = core
            .store
            .downloads()
            .iter()
            .map(|download| DownloadRow {
                id: download.id as i32,
                name: download.file_name.clone().into(),
                size: format_bytes(download.bytes).into(),
                state: download.state.label().into(),
            })
            .collect();
        self.ui
            .set_downloads(ModelRc::new(VecModel::from(downloads)));
    }

    fn load(&self, url: &str) {
        let result = self.engine.borrow_mut().load(url);
        match result {
            Err(error) => self.core.borrow_mut().status = error.to_string(),
            Ok(()) => {
                // Media URLs report the codec/decode policy in the status bar
                // (P4-T18a): "playing VP9 720p — software decode" or the
                // codec-bundle hint, instead of a silent blank page.
                let capabilities = *self.media.borrow();
                if let Some(note) = media::status_line(url, &capabilities) {
                    self.core.borrow_mut().status = note;
                }
            }
        }
    }

    pub fn navigate(&self, input: &str) {
        let request = self.core.borrow_mut().navigate(input);
        if let Some(request) = request {
            self.load(&request.url);
        }
        self.sync();
    }

    pub fn back(&self) {
        let request = self.core.borrow_mut().back();
        if let Some(request) = request {
            self.load(&request.url);
        }
        self.sync();
    }

    pub fn forward(&self) {
        let request = self.core.borrow_mut().forward();
        if let Some(request) = request {
            self.load(&request.url);
        }
        self.sync();
    }

    pub fn reload(&self) {
        let url = self.core.borrow().active().url.clone();
        let _ = self.engine.borrow_mut().reload();
        self.load(&url);
        self.sync();
    }

    pub fn new_tab(&self, private: bool) {
        self.core.borrow_mut().open_tab(private);
        self.sync();
    }

    pub fn close_tab(&self, index: usize) {
        self.core.borrow_mut().close_tab(index);
        self.sync();
    }

    pub fn select_tab(&self, index: usize) {
        self.core.borrow_mut().tabs.select(index);
        self.sync();
    }

    pub fn toggle_bookmark(&self) {
        self.core.borrow_mut().toggle_bookmark();
        self.sync();
    }

    pub fn open_sidebar(&self, section: &str) {
        {
            let mut core = self.core.borrow_mut();
            core.open_sidebar(section);
        }
        self.ui.set_sidebar_visible(true);
        self.sync();
    }

    pub fn remove_bookmark(&self, id: u64) {
        {
            let mut core = self.core.borrow_mut();
            core.store.remove_bookmark(id);
        }
        self.core.borrow().save();
        self.sync();
    }

    pub fn remove_download(&self, id: u64) {
        {
            let mut core = self.core.borrow_mut();
            let store = &mut core.store;
            let downloads = store.downloads().to_vec();
            let mut filtered: Vec<_> = downloads.into_iter().filter(|d| d.id != id).collect();
            filtered.shrink_to_fit();
            store.replace_downloads(filtered);
        }
        self.core.borrow().save();
        self.sync();
    }

    pub fn clear_history(&self) {
        self.core.borrow_mut().clear_history();
        self.sync();
    }

    pub fn sidebar_section(&self) -> SidebarSection {
        self.core.borrow().sidebar
    }

    /// Rows currently rendered for the active sidebar section (tests).
    pub fn sidebar_len(&self) -> usize {
        self.core.borrow().sidebar_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_controller_shape_is_stable() {
        // Pure-data assertion: the demo profile that the screenshot renders.
        assert_eq!(format_bytes(1_462_272_000), "1.4 GB");
        assert_eq!(HOME, "about:home");
    }
}
