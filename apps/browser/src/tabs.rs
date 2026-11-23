//! Tab model for the AetherOS browser (P4-T18).
//!
//! A tab owns its own back/forward stack. Private tabs never write history or
//! bookmarks (private window requirement) and are marked so the UI can show it.

use crate::url::display_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub loading: bool,
    pub private: bool,
    pub back: Vec<HistoryEntry>,
    pub forward: Vec<HistoryEntry>,
    pub zoom: u8,
}

impl Tab {
    pub fn new(id: u64, private: bool, home: &str) -> Tab {
        Tab {
            id,
            title: if private { "Private tab" } else { "New tab" }.to_string(),
            url: home.to_string(),
            loading: false,
            private,
            back: Vec::new(),
            forward: Vec::new(),
            zoom: 100,
        }
    }

    /// Record a committed navigation. Same-URL reloads do not grow the stack.
    pub fn commit(&mut self, url: &str, title: &str) {
        if self.url == url {
            self.title = title.to_string();
            self.loading = false;
            return;
        }
        if !self.url.is_empty() {
            self.back.push(HistoryEntry {
                url: self.url.clone(),
                title: self.title.clone(),
            });
        }
        self.forward.clear();
        self.url = url.to_string();
        self.title = if title.trim().is_empty() {
            display_url(url)
        } else {
            title.to_string()
        };
        self.loading = false;
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Move back; returns the URL to load.
    pub fn go_back(&mut self) -> Option<String> {
        let previous = self.back.pop()?;
        self.forward.push(HistoryEntry {
            url: self.url.clone(),
            title: self.title.clone(),
        });
        self.url = previous.url.clone();
        self.title = previous.title;
        Some(previous.url)
    }

    pub fn go_forward(&mut self) -> Option<String> {
        let next = self.forward.pop()?;
        self.back.push(HistoryEntry {
            url: self.url.clone(),
            title: self.title.clone(),
        });
        self.url = next.url.clone();
        self.title = next.title;
        Some(next.url)
    }

    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + 10).min(200);
    }

    pub fn zoom_out(&mut self) {
        self.zoom = self.zoom.saturating_sub(10).max(50);
    }

    pub fn zoom_reset(&mut self) {
        self.zoom = 100;
    }
}

/// The tab strip. Keeps at least one tab alive.
#[derive(Debug, Clone)]
pub struct TabSet {
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: u64,
}

impl TabSet {
    pub fn new(home: &str) -> TabSet {
        TabSet {
            tabs: vec![Tab::new(1, false, home)],
            active: 0,
            next_id: 2,
        }
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn open(&mut self, private: bool, home: &str) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id, private, home));
        self.active = self.tabs.len() - 1;
        self.active
    }

    /// Close a tab. Returns the id that was closed, or `None` when closing the
    /// last tab would leave the browser empty (the caller resets to home).
    pub fn close(&mut self, index: usize) -> Option<u64> {
        if index >= self.tabs.len() {
            return None;
        }
        if self.tabs.len() == 1 {
            return None;
        }
        let removed = self.tabs.remove(index);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if index < self.active {
            self.active -= 1;
        }
        Some(removed.id)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if self.active == from {
            self.active = to;
        } else if from < self.active && to >= self.active {
            self.active -= 1;
        } else if from > self.active && to <= self.active {
            self.active += 1;
        }
    }

    pub fn private_count(&self) -> usize {
        self.tabs.iter().filter(|tab| tab.private).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "about:home";

    #[test]
    fn new_tab_set_has_one_home_tab() {
        let set = TabSet::new(HOME);
        assert_eq!(set.tabs.len(), 1);
        assert_eq!(set.active_tab().url, HOME);
        assert!(!set.active_tab().can_go_back());
    }

    #[test]
    fn navigation_builds_back_and_forward_stacks() {
        let mut set = TabSet::new(HOME);
        let tab = set.active_tab_mut();
        tab.commit("https://a.example/", "A");
        tab.commit("https://b.example/", "B");
        assert!(tab.can_go_back());
        assert_eq!(tab.go_back().unwrap(), "https://a.example/");
        assert!(tab.can_go_forward());
        assert_eq!(tab.go_forward().unwrap(), "https://b.example/");
        assert!(!tab.can_go_forward());
    }

    #[test]
    fn new_navigation_clears_the_forward_stack() {
        let mut tab = Tab::new(1, false, HOME);
        tab.commit("https://a.example/", "A");
        tab.commit("https://b.example/", "B");
        tab.go_back();
        tab.commit("https://c.example/", "C");
        assert!(!tab.can_go_forward());
        assert_eq!(tab.url, "https://c.example/");
    }

    #[test]
    fn same_url_reload_does_not_grow_history() {
        let mut tab = Tab::new(1, false, HOME);
        tab.commit("https://a.example/", "A");
        tab.commit("https://a.example/", "A reloaded");
        assert_eq!(tab.back.len(), 1);
        assert_eq!(tab.title, "A reloaded");
    }

    #[test]
    fn closing_the_last_tab_is_refused() {
        let mut set = TabSet::new(HOME);
        assert_eq!(set.close(0), None);
        assert_eq!(set.tabs.len(), 1);
    }

    #[test]
    fn closing_tabs_keeps_the_active_index_valid() {
        let mut set = TabSet::new(HOME);
        set.open(false, HOME);
        set.open(false, HOME);
        assert_eq!(set.active, 2);
        let closed = set.close(1).unwrap();
        assert!(closed > 0);
        assert_eq!(set.tabs.len(), 2);
        assert!(set.active < set.tabs.len());
    }

    #[test]
    fn reordering_follows_the_active_tab() {
        let mut set = TabSet::new(HOME);
        set.open(false, HOME);
        set.open(false, HOME);
        set.select(0);
        let first_id = set.tabs[0].id;
        set.move_tab(0, 2);
        assert_eq!(set.active, 2);
        assert_eq!(set.active_tab().id, first_id);
    }

    #[test]
    fn private_tabs_are_counted_and_labelled() {
        let mut set = TabSet::new(HOME);
        set.open(true, HOME);
        assert_eq!(set.private_count(), 1);
        assert_eq!(set.active_tab().title, "Private tab");
    }

    #[test]
    fn zoom_is_clamped() {
        let mut tab = Tab::new(1, false, HOME);
        for _ in 0..20 {
            tab.zoom_in();
        }
        assert_eq!(tab.zoom, 200);
        for _ in 0..40 {
            tab.zoom_out();
        }
        assert_eq!(tab.zoom, 50);
        tab.zoom_reset();
        assert_eq!(tab.zoom, 100);
    }
}
