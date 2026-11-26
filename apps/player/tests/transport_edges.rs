//! Play/pause/seek edge cases at controller level (P4-T14).
//!
//! One test function: the testing backend installs a process-wide platform.
//! The engine is shared through an `Rc<RefCell<MockEngine>>` so the test can
//! assert the exact call order the controller produces — that order is the
//! contract the `gst-play-1.0` adapter relies on (record position, then play;
//! pause, then record).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use aether_player::codec::{MediaInfo, MediaProbe};
use aether_player::engine::{Engine, EngineError, MockEngine};
use aether_player::{demo_playlist, PlayerApp, PlayerController, PlaybackState};

/// `Engine` that forwards to a shared mock so the test can read its calls.
struct SharedEngine(Rc<RefCell<MockEngine>>);

impl Engine for SharedEngine {
    fn name(&self) -> &'static str {
        self.0.borrow().name()
    }

    fn play(&mut self, uri: &str) -> Result<(), EngineError> {
        self.0.borrow_mut().play(uri)
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.0.borrow_mut().pause()
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.0.borrow_mut().stop()
    }

    fn seek(&mut self, position: Duration) -> Result<(), EngineError> {
        self.0.borrow_mut().seek(position)
    }

    fn set_position(&mut self, position: Duration) {
        self.0.borrow_mut().set_position(position)
    }

    fn set_duration(&mut self, duration: Duration) {
        self.0.borrow_mut().set_duration(duration)
    }

    fn position(&self) -> Duration {
        self.0.borrow().position()
    }
}

/// Probe that reports a restricted (H.264/AAC) stream.
struct RestrictedProbe;

impl MediaProbe for RestrictedProbe {
    fn name(&self) -> &'static str {
        "restricted-probe"
    }

    fn probe(&self, _uri: &str) -> Result<MediaInfo, EngineError> {
        Ok(MediaInfo {
            duration: Some(Duration::from_secs(120)),
            audio: vec!["audio/mpeg, mpegversion=(int)4".to_string()],
            video: vec!["video/x-h264".to_string()],
            analysis_failed: false,
        })
    }
}

#[test]
fn play_pause_seek_edge_cases_keep_the_engine_calls_honest() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let ui = PlayerApp::new().unwrap();
        let engine = Rc::new(RefCell::new(MockEngine::new()));
        let controller = PlayerController::new(
            ui,
            demo_playlist(7),
            Box::new(SharedEngine(engine.clone())),
        );
        let calls = || engine.borrow().calls().to_vec();

        // --- 1. play records the position, then starts the pipeline --------
        controller.play();
        assert_eq!(
            calls(),
            ["position 0", "play file:///home/aether/Music/glass-horizon.flac"],
            "play must record the resume position instead of seeking"
        );
        assert!(controller.transport().is_playing());

        // --- 2. pausing pauses first, then records the position ------------
        controller.tick(Duration::from_secs(30));
        controller.ui().invoke_play_pause(); // playing -> pause
        assert_eq!(controller.transport().state(), PlaybackState::Paused);
        assert_eq!(
            calls()[2..],
            ["pause", "position 30"],
            "a paused seek would relaunch gst-play-1.0 just to kill it"
        );

        // --- 3. the slider while paused only records -----------------------
        controller.ui().invoke_play_pause(); // resume
        assert_eq!(
            calls()[4..],
            ["position 30", "play file:///home/aether/Music/glass-horizon.flac"],
            "resume continues exactly where playback paused"
        );
        controller.ui().invoke_play_pause(); // pause again at 30 s
        let before_seek = calls().len();
        controller.ui().invoke_seek(0.5);
        assert_eq!(
            controller.transport().position(),
            Duration::from_secs(116),
            "half of 3:52"
        );
        assert_eq!(
            calls()[before_seek..],
            ["seek 116"],
            "one recorded seek while paused, no pipeline restart"
        );

        // --- 4. next track does not restart the previous URI ---------------
        let before_next = calls().len();
        controller.ui().invoke_play_pause(); // resume at 1:56 ...
        let resumed_at = calls().len();
        assert!(resumed_at > before_next);
        controller.ui().invoke_next();
        let next_calls = calls()[resumed_at..].to_vec();
        assert!(
            next_calls.contains(&"play file:///home/aether/Music/slow-orbit.flac".to_string()),
            "next starts the new track: {next_calls:?}"
        );
        assert_eq!(
            next_calls.last().unwrap(),
            "play file:///home/aether/Music/slow-orbit.flac",
            "the old URI is not relaunched before the new one starts"
        );
        assert!(
            !next_calls.iter().any(|call| call.starts_with("seek")),
            "changing tracks never issues a seek: {next_calls:?}"
        );

        // --- 5. repeat one replays from zero without a stale seek ----------
        controller.ui().invoke_repeat_cycled(); // Repeat all -> Repeat one
        controller.tick(Duration::from_secs(400));
        let replay = calls().last().cloned().unwrap();
        assert_eq!(
            replay, "play file:///home/aether/Music/slow-orbit.flac",
            "repeat one restarts the same track"
        );
        assert_eq!(controller.transport().position(), Duration::ZERO);

        // --- 6. a restricted codec never reaches the engine ----------------
        let restricted_ui = PlayerApp::new().unwrap();
        let restricted_engine = Rc::new(RefCell::new(MockEngine::new()));
        let restricted = PlayerController::new_with_probe(
            restricted_ui,
            demo_playlist(7),
            Box::new(SharedEngine(restricted_engine.clone())),
            Box::new(RestrictedProbe),
        );
        restricted.play();
        assert!(
            restricted_engine.borrow().calls().is_empty(),
            "no pipeline for a file that needs the codec bundle"
        );
        assert!(!restricted.transport().is_playing());
        assert!(restricted.status().contains("codec bundle"));

        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
