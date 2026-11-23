//! Player controller: the only place that mutates Slint properties (P4-T14).
//!
//! Wires the playlist, the transport state machine and the audio engine
//! together. The UI never owns playback state: it sends callbacks and reads
//! properties that [`PlayerController::sync`] writes.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ModelRc, VecModel};

use crate::engine::{Engine, MockEngine};
use crate::playlist::{Playlist, Repeat, Track};
use crate::transport::{PlaybackState, TickOutcome, Transport};
use crate::{PlayerApp, TrackView};

pub struct PlayerController {
    ui: PlayerApp,
    playlist: RefCell<Playlist>,
    transport: RefCell<Transport>,
    engine: RefCell<Box<dyn Engine>>,
    status: RefCell<String>,
}

impl PlayerController {
    pub fn new(
        ui: PlayerApp,
        playlist: Playlist,
        engine: Box<dyn Engine>,
    ) -> Rc<PlayerController> {
        let controller = Rc::new(PlayerController {
            ui,
            playlist: RefCell::new(playlist),
            transport: RefCell::new(Transport::new()),
            engine: RefCell::new(engine),
            status: RefCell::new(String::new()),
        });
        controller.wire();
        controller.load_current(false);
        controller.sync();
        controller
    }

    /// Demo content for the screenshot renderer: six tracks, mid-track
    /// position and the mock engine (rendering must not start audio).
    pub fn with_demo(ui: PlayerApp) -> Rc<PlayerController> {
        // Prefer the real adapter so the header reports what the machine has;
        // rendering never calls `play`, so no audio process is started.
        let (engine, missing): (Box<dyn Engine>, Option<&str>) =
            match crate::engine::GStreamerEngine::detect() {
                Ok(engine) => (Box::new(engine), None),
                Err(_) => (Box::new(MockEngine::new()), Some("No GStreamer backend")),
            };
        let controller = PlayerController::new(ui, demo_playlist(7), engine);
        if let Some(message) = missing {
            *controller.status.borrow_mut() = message.to_string();
        }
        controller.transport.borrow_mut().seek(Duration::from_secs(72));
        controller.sync();
        controller
    }

    pub fn ui(&self) -> &PlayerApp {
        &self.ui
    }

    pub fn transport(&self) -> std::cell::Ref<'_, Transport> {
        self.transport.borrow()
    }

    pub fn playlist(&self) -> std::cell::Ref<'_, Playlist> {
        self.playlist.borrow()
    }

    pub fn status(&self) -> String {
        self.status.borrow().clone()
    }

    pub fn current_title(&self) -> Option<String> {
        self.playlist
            .borrow()
            .current()
            .map(|track| track.title.clone())
    }

    /// Advance playback; the UI calls this from a timer, tests call it
    /// directly.
    pub fn tick(&self, delta: Duration) {
        let outcome = self.transport.borrow_mut().tick(delta);
        if outcome == TickOutcome::Ended {
            self.advance_after_end();
        }
        self.sync();
    }

    // ------------------------------------------------------------ transport ---

    fn play_current(&self) {
        let uri = self
            .playlist
            .borrow()
            .current()
            .map(|track| track.uri.clone());
        let Some(uri) = uri else {
            return;
        };
        self.transport.borrow_mut().play();
        let mut engine = self.engine.borrow_mut();
        match engine.play(&uri) {
            Ok(()) => *self.status.borrow_mut() = String::new(),
            // The transport still runs so the interface is usable without an
            // audio backend; the status pill explains the silence.
            Err(error) => *self.status.borrow_mut() = error.message(),
        }
    }

    fn toggle_play(&self) {
        if self.transport.borrow().is_playing() {
            self.transport.borrow_mut().pause();
            let mut engine = self.engine.borrow_mut();
            let _ = engine.pause();
        } else {
            self.play_current();
        }
        self.sync();
    }

    fn load_current(&self, autoplay: bool) {
        let duration = self
            .playlist
            .borrow()
            .current()
            .map(|track| track.duration)
            .unwrap_or(Duration::ZERO);
        self.transport.borrow_mut().load(duration);
        if autoplay {
            self.play_current();
        } else {
            self.transport.borrow_mut().stop();
        }
    }

    fn next(&self) {
        let was_playing = self.transport.borrow().is_playing();
        let next = self.playlist.borrow_mut().next_track();
        match next {
            Some(_) => self.load_current(was_playing),
            None => {
                self.transport.borrow_mut().stop();
                let mut engine = self.engine.borrow_mut();
                let _ = engine.stop();
            }
        }
        self.sync();
    }

    fn previous(&self) {
        let was_playing = self.transport.borrow().is_playing();
        // Like every other player: restart the track when past the first
        // seconds, otherwise step back.
        if self.transport.borrow().position() > Duration::from_secs(3) {
            self.transport.borrow_mut().seek(Duration::ZERO);
            let mut engine = self.engine.borrow_mut();
            let _ = engine.seek(Duration::ZERO);
        } else if self.playlist.borrow_mut().previous_track().is_some() {
            self.load_current(was_playing);
        }
        self.sync();
    }

    fn advance_after_end(&self) {
        let repeat = self.playlist.borrow().repeat();
        if repeat == Repeat::One {
            self.transport.borrow_mut().seek(Duration::ZERO);
            self.play_current();
            return;
        }
        let next = self.playlist.borrow_mut().next_track();
        match next {
            Some(_) => self.load_current(true),
            None => {
                self.transport.borrow_mut().stop();
                let mut engine = self.engine.borrow_mut();
                let _ = engine.stop();
            }
        }
    }

    fn seek_fraction(&self, fraction: f32) {
        let position = self.transport.borrow_mut().seek_fraction(fraction);
        {
            let mut engine = self.engine.borrow_mut();
            let _ = engine.seek(position);
        }
        self.sync();
    }

    fn set_volume(&self, volume: f32) {
        self.transport.borrow_mut().set_volume(volume);
        self.sync();
    }

    fn select_track(&self, id: &str) {
        let was_playing = self.transport.borrow().is_playing();
        let index = self
            .playlist
            .borrow()
            .tracks()
            .iter()
            .position(|track| track.id == id);
        let Some(index) = index else {
            return;
        };
        if self.playlist.borrow_mut().select(index).is_some() {
            self.load_current(was_playing);
        }
        self.sync();
    }

    fn wire(self: &Rc<Self>) {
        let with = |f: fn(&PlayerController)| {
            let weak = Rc::downgrade(self);
            move || {
                if let Some(controller) = weak.upgrade() {
                    f(&controller);
                }
            }
        };
        self.ui.on_play_pause(with(Self::toggle_play));
        self.ui.on_next(with(Self::next));
        self.ui.on_previous(with(Self::previous));
        self.ui.on_shuffle_toggled(with(Self::toggle_shuffle));
        self.ui.on_repeat_cycled(with(Self::cycle_repeat));
        self.ui.on_mute_toggled(with(Self::toggle_mute));
        {
            let weak = Rc::downgrade(self);
            self.ui.on_seek(move |fraction| {
                if let Some(controller) = weak.upgrade() {
                    controller.seek_fraction(fraction);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_volume_changed(move |volume| {
                if let Some(controller) = weak.upgrade() {
                    controller.set_volume(volume);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            self.ui.on_select_track(move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.select_track(id.as_str());
                }
            });
        }
    }

    fn toggle_shuffle(&self) {
        self.playlist.borrow_mut().toggle_shuffle();
        self.sync();
    }

    fn cycle_repeat(&self) {
        self.playlist.borrow_mut().cycle_repeat();
        self.sync();
    }

    fn toggle_mute(&self) {
        self.transport.borrow_mut().toggle_mute();
        self.sync();
    }

    // ---------------------------------------------------------------- sync ---

    pub fn sync(&self) {
        let track = self.playlist.borrow().current().cloned();
        let transport = self.transport.borrow();

        match &track {
            Some(track) => {
                self.ui.set_now_title(track.title.clone().into());
                self.ui.set_now_artist(track.artist.clone().into());
                self.ui.set_now_album(track.album.clone().into());
            }
            None => {
                self.ui.set_now_title("Nothing playing".into());
                self.ui.set_now_artist("".into());
                self.ui.set_now_album("".into());
            }
        }
        self.ui.set_now_elapsed(transport.elapsed_label().into());
        self.ui.set_now_total(transport.total_label().into());
        self.ui.set_progress(transport.progress());
        self.ui.set_playing(transport.is_playing());
        self.ui.set_volume(transport.volume());
        self.ui.set_muted(transport.is_muted());

        let (shuffle, repeat, queue_len, total, current_index) = {
            let playlist = self.playlist.borrow();
            (
                playlist.is_shuffled(),
                playlist.repeat(),
                playlist.len(),
                playlist.total_label(),
                playlist.current_index(),
            )
        };
        self.ui.set_shuffle(shuffle);
        self.ui.set_repeat_code(repeat.code());
        self.ui.set_repeat_label(repeat.label().into());
        self.ui.set_queue_label(
            format!(
                "{queue_len} track{} · {total}",
                if queue_len == 1 { "" } else { "s" }
            )
            .into(),
        );

        let engine_name = self.engine.borrow().name();
        self.ui.set_engine_status(
            if self.status.borrow().is_empty() {
                format!("{engine_name} backend")
            } else {
                self.status.borrow().clone()
            }
            .into(),
        );

        let rows: Vec<TrackView> = {
            let playlist = self.playlist.borrow();
            playlist
                .tracks()
                .iter()
                .enumerate()
                .map(|(index, track)| TrackView {
                    id: track.id.clone().into(),
                    index: (index + 1) as i32,
                    title: track.title.clone().into(),
                    subtitle: track.subtitle().into(),
                    duration: track.duration_label().into(),
                    active: current_index == Some(index),
                })
                .collect()
        };
        self.ui.set_tracks(ModelRc::new(VecModel::from(rows)));
    }
}

/// Demo queue used by the screenshot renderer and the first-run experience.
pub fn demo_playlist(seed: u64) -> Playlist {
    let tracks = [
        Track::new("glass-horizon", "Glass Horizon", "Aether Ensemble", "Quiet Machines", 232),
        Track::new("slow-orbit", "Slow Orbit", "Aether Ensemble", "Quiet Machines", 198),
        Track::new("neon-rain", "Neon Rain", "Mira Voss", "Night Signals", 254),
        Track::new("deep-field", "Deep Field", "Mira Voss", "Night Signals", 301),
        Track::new("drift", "Drift", "Kite & Co", "Paper Skies", 187),
        Track::new("dawn-chorus", "Dawn Chorus", "Kite & Co", "Paper Skies", 276),
    ];
    let mut playlist = Playlist::new(seed);
    for track in tracks {
        playlist.add(track);
    }
    playlist.set_repeat(Repeat::All);
    playlist
}

/// Playback state label for `--status`.
pub fn state_label(state: PlaybackState) -> &'static str {
    state.label()
}
