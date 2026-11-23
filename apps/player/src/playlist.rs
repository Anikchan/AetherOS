//! Playlist and queue model for the Player app (P4-T14, DA-AC-06).
//!
//! The queue keeps two views of the same tracks: `tracks` in the order the user
//! arranged them, and `order` (a permutation of track indices) as the play
//! order. Shuffle only rewrites `order`, so turning shuffle off restores the
//! user's arrangement and the currently playing track never jumps.
//!
//! Shuffle uses a seeded xorshift generator: a fixed seed and the same calls
//! always produce the same play order, which is what the reproducibility test
//! checks.

use std::time::Duration;

use crate::transport::format_time;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Duration,
    /// GStreamer URI (`file:///...`, `http://...`).
    pub uri: String,
}

impl Track {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        artist: impl Into<String>,
        album: impl Into<String>,
        seconds: u64,
    ) -> Track {
        let id = id.into();
        Track {
            uri: format!("file:///home/anik/Music/{id}.flac"),
            id,
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration: Duration::from_secs(seconds),
        }
    }

    pub fn duration_label(&self) -> String {
        format_time(self.duration)
    }

    /// `Artist · Album`
    pub fn subtitle(&self) -> String {
        if self.album.is_empty() {
            self.artist.clone()
        } else {
            format!("{} · {}", self.artist, self.album)
        }
    }
}

/// Deterministic xorshift64* generator (no dependency needed).
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut state = self.state;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.state = state;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish value in `0..n`; `0` for an empty range.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}

impl Repeat {
    pub const ALL: [Repeat; 3] = [Repeat::Off, Repeat::All, Repeat::One];

    /// Cycle Off -> All -> One -> Off (the transport button behaviour).
    pub fn cycle(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "Repeat off",
            Repeat::All => "Repeat all",
            Repeat::One => "Repeat one",
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Repeat::Off => 0,
            Repeat::All => 1,
            Repeat::One => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Playlist {
    tracks: Vec<Track>,
    order: Vec<usize>,
    cursor: usize,
    shuffle: bool,
    repeat: Repeat,
    seed: u64,
    rng: Rng,
}

impl Playlist {
    pub fn new(seed: u64) -> Playlist {
        Playlist {
            tracks: Vec::new(),
            order: Vec::new(),
            cursor: 0,
            shuffle: false,
            repeat: Repeat::Off,
            seed,
            rng: Rng::new(seed),
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Play order as queue indices.
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    pub fn cycle_repeat(&mut self) -> Repeat {
        self.repeat = self.repeat.cycle();
        self.repeat
    }

    /// Append a track; returns its queue index.
    pub fn add(&mut self, track: Track) -> usize {
        let index = self.tracks.len();
        self.tracks.push(track);
        self.order.push(index);
        if self.tracks.len() == 1 {
            self.cursor = 0;
        }
        index
    }

    /// Remove a track by queue index. The current track survives unless it is
    /// the one being removed (then the next track becomes current).
    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index >= self.tracks.len() {
            return None;
        }
        let removed = self.tracks.remove(index);
        let was_current = self.current_index() == Some(index);
        self.order.retain(|&queued| queued != index);
        for queued in self.order.iter_mut() {
            if *queued > index {
                *queued -= 1;
            }
        }
        if !self.shuffle {
            self.order = (0..self.tracks.len()).collect();
        }
        if was_current || self.cursor >= self.order.len() {
            self.cursor = self.cursor.min(self.order.len().saturating_sub(1));
        }
        Some(removed)
    }

    /// Reorder within the user's arrangement. Returns `false` for bad indices.
    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        let len = self.tracks.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        let current_id = self.current().map(|track| track.id.clone());
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        let current = current_id.and_then(|id| self.tracks.iter().position(|track| track.id == id));
        if self.shuffle {
            // A manual reorder invalidates the shuffled sequence; rebuild it
            // with the current track still playing.
            self.order = self.shuffled_order(current);
            self.cursor = 0;
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.cursor = current.unwrap_or(0);
        }
        true
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.order.clear();
        self.cursor = 0;
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index()
            .and_then(|index| self.tracks.get(index))
    }

    /// Queue index of the playing track.
    pub fn current_index(&self) -> Option<usize> {
        self.order.get(self.cursor).copied()
    }

    /// Position in the play order (1-based for "3 of 8" labels).
    pub fn position(&self) -> Option<usize> {
        if self.order.is_empty() {
            None
        } else {
            Some(self.cursor + 1)
        }
    }

    /// Jump to a queue index.
    pub fn select(&mut self, index: usize) -> Option<usize> {
        let position = self.order.iter().position(|&queued| queued == index)?;
        self.cursor = position;
        Some(index)
    }

    /// Next track honouring the repeat mode. `None` at the end with repeat off
    /// (the caller stops playback).
    pub fn next_track(&mut self) -> Option<usize> {
        if self.order.is_empty() {
            return None;
        }
        match self.repeat {
            Repeat::One => self.current_index(),
            Repeat::All => {
                self.cursor = (self.cursor + 1) % self.order.len();
                self.current_index()
            }
            Repeat::Off => {
                if self.cursor + 1 < self.order.len() {
                    self.cursor += 1;
                    self.current_index()
                } else {
                    None
                }
            }
        }
    }

    /// Previous track; wraps to the end only with repeat all.
    pub fn previous_track(&mut self) -> Option<usize> {
        if self.order.is_empty() {
            return None;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            self.current_index()
        } else if self.repeat == Repeat::All {
            self.cursor = self.order.len() - 1;
            self.current_index()
        } else {
            None
        }
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if shuffle == self.shuffle {
            return;
        }
        let current = self.current_index();
        self.shuffle = shuffle;
        if shuffle {
            self.order = self.shuffled_order(current);
            self.cursor = 0;
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.cursor = current.unwrap_or(0);
        }
    }

    pub fn toggle_shuffle(&mut self) -> bool {
        self.set_shuffle(!self.shuffle);
        self.shuffle
    }

    /// Current track first (so it keeps playing), the rest shuffled.
    fn shuffled_order(&mut self, current: Option<usize>) -> Vec<usize> {
        let mut rest: Vec<usize> = (0..self.tracks.len())
            .filter(|index| Some(*index) != current)
            .collect();
        for index in (1..rest.len()).rev() {
            let swap = self.rng.below(index + 1);
            rest.swap(index, swap);
        }
        let mut order = Vec::with_capacity(self.tracks.len());
        if let Some(current) = current {
            order.push(current);
        }
        order.extend(rest);
        order
    }

    pub fn total_duration(&self) -> Duration {
        self.tracks
            .iter()
            .fold(Duration::ZERO, |total, track| total + track.duration)
    }

    pub fn total_label(&self) -> String {
        format_time(self.total_duration())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_tracks() -> Vec<Track> {
        vec![
            Track::new("glass", "Glass Horizon", "Aether Ensemble", "Quiet Machines", 232),
            Track::new("slow", "Slow Orbit", "Aether Ensemble", "Quiet Machines", 198),
            Track::new("neon", "Neon Rain", "Mira Voss", "Night Signals", 254),
            Track::new("deep", "Deep Field", "Mira Voss", "Night Signals", 301),
            Track::new("drift", "Drift", "Kite & Co", "Paper Skies", 187),
            Track::new("dawn", "Dawn Chorus", "Kite & Co", "Paper Skies", 276),
        ]
    }

    fn playlist() -> Playlist {
        let mut playlist = Playlist::new(7);
        for track in demo_tracks() {
            playlist.add(track);
        }
        playlist
    }

    #[test]
    fn queue_add_remove_move_and_clear() {
        let mut playlist = playlist();
        assert_eq!(playlist.len(), 6);
        assert_eq!(playlist.current().unwrap().title, "Glass Horizon");
        assert_eq!(playlist.position(), Some(1));
        assert_eq!(playlist.total_duration(), Duration::from_secs(232 + 198 + 254 + 301 + 187 + 276));
        assert_eq!(playlist.total_label(), "24:08");

        // Move within the arrangement; the current track follows its new index.
        assert!(playlist.move_track(0, 2));
        assert_eq!(playlist.tracks()[2].title, "Glass Horizon");
        assert_eq!(playlist.current().unwrap().title, "Glass Horizon");
        assert_eq!(playlist.current_index(), Some(2));
        assert!(!playlist.move_track(0, 99), "out-of-range moves are refused");

        // Selecting another track and removing it moves to the next track.
        assert_eq!(playlist.select(3), Some(3));
        let removed = playlist.remove(3).unwrap();
        assert_eq!(removed.title, "Deep Field");
        assert_eq!(playlist.len(), 5);
        assert!(playlist.current().is_some());
        assert_eq!(playlist.order().len(), playlist.len());

        playlist.clear();
        assert!(playlist.is_empty());
        assert_eq!(playlist.current(), None);
        assert_eq!(playlist.position(), None);
        assert_eq!(playlist.total_duration(), Duration::ZERO);
    }

    #[test]
    fn shuffle_is_deterministic_for_a_seed() {
        let mut first = playlist();
        let mut second = playlist();
        first.set_shuffle(true);
        second.set_shuffle(true);
        assert_eq!(first.order(), second.order(), "same seed, same play order");

        let mut other = Playlist::new(99);
        for track in demo_tracks() {
            other.add(track);
        }
        other.set_shuffle(true);
        assert_ne!(other.order(), first.order(), "a different seed shuffles differently");

        // The permutation covers every track exactly once and keeps the current
        // track playing first.
        let mut seen = first.order().to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (0..first.len()).collect::<Vec<_>>());
        assert_eq!(first.order()[0], 0, "the playing track stays first");

        // Turning shuffle off restores the arrangement order.
        first.set_shuffle(false);
        assert_eq!(first.order(), &(0..first.len()).collect::<Vec<_>>());
        assert_eq!(first.current().unwrap().title, "Glass Horizon");
    }

    #[test]
    fn repeat_modes_drive_next_and_previous() {
        let mut playlist = playlist();
        assert_eq!(playlist.repeat(), Repeat::Off);

        // Off: stop at the end.
        for _ in 0..5 {
            assert!(playlist.next_track().is_some());
        }
        assert_eq!(playlist.current().unwrap().title, "Dawn Chorus");
        assert_eq!(playlist.next_track(), None, "repeat off stops at the last track");
        assert_eq!(playlist.current().unwrap().title, "Dawn Chorus");

        // All: wrap around at both ends.
        playlist.set_repeat(Repeat::All);
        assert_eq!(playlist.next_track(), Some(0));
        assert_eq!(playlist.current().unwrap().title, "Glass Horizon");
        assert_eq!(playlist.previous_track(), Some(5), "previous wraps with repeat all");

        // One: next repeats the same track, previous still steps back.
        playlist.set_repeat(Repeat::One);
        let current = playlist.current_index();
        assert_eq!(playlist.next_track(), current);
        assert_eq!(playlist.previous_track(), Some(4));

        assert_eq!(Repeat::Off.cycle(), Repeat::All);
        assert_eq!(Repeat::All.cycle(), Repeat::One);
        assert_eq!(Repeat::One.cycle(), Repeat::Off);
        assert_eq!(Repeat::All.label(), "Repeat all");
        assert_eq!(Repeat::One.code(), 2);
    }

    #[test]
    fn shuffle_walks_every_track_before_stopping() {
        let mut playlist = playlist();
        playlist.set_shuffle(true);
        playlist.set_repeat(Repeat::Off);
        let mut visited = vec![playlist.current_index().unwrap()];
        while let Some(index) = playlist.next_track() {
            visited.push(index);
        }
        visited.sort_unstable();
        assert_eq!(visited, (0..6).collect::<Vec<_>>(), "each track plays once");
    }
}
