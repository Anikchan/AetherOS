//! Codec-policy tests for the Player controller (P4-T14, DA-AC-06).
//!
//! One test function: the Slint testing backend installs a process-wide
//! platform. The probe is scripted and the engine records calls, so no
//! GStreamer installation or audio device is needed.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use aether_player::codec::{MediaInfo, MockProbe};
use aether_player::demo_playlist;
use aether_player::engine::{Engine, EngineError};
use aether_player::{PlaybackState, PlayerApp, PlayerController};

/// Engine that records calls without starting a process.
struct SharedEngine {
    calls: Rc<RefCell<Vec<String>>>,
}

impl Engine for SharedEngine {
    fn name(&self) -> &'static str {
        "shared mock"
    }

    fn play(&mut self, uri: &str) -> Result<(), EngineError> {
        self.calls.borrow_mut().push(format!("play {uri}"));
        Ok(())
    }

    fn pause(&mut self) -> Result<(), EngineError> {
        self.calls.borrow_mut().push("pause".into());
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.calls.borrow_mut().push("stop".into());
        Ok(())
    }

    fn seek(&mut self, position: Duration) -> Result<(), EngineError> {
        self.calls.borrow_mut().push(format!("seek {}", position.as_secs()));
        Ok(())
    }

    fn position(&self) -> Duration {
        Duration::ZERO
    }
}

fn controller(
    probe: MockProbe,
    calls: Rc<RefCell<Vec<String>>>,
) -> Rc<PlayerController> {
    let ui = PlayerApp::new().unwrap();
    PlayerController::new_with_probe(
        ui,
        demo_playlist(7),
        Box::new(SharedEngine { calls }),
        Box::new(probe),
    )
}

#[test]
fn codec_policy_gates_playback_and_uses_discovered_duration() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    // --- restricted H.264/AAC never reaches the engine -------------------
    {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let probe = MockProbe::new(MediaInfo {
            duration: Some(Duration::from_secs(10)),
            audio: vec!["audio/mpeg, mpegversion=(int)4".into()],
            video: vec!["video/x-h264, width=(int)1920".into()],
            analysis_failed: false,
        });
        let controller = controller(probe, calls.clone());
        controller.play();
        assert_eq!(controller.transport().state(), PlaybackState::Stopped);
        assert!(controller.status().contains("codec bundle"));
        assert!(controller.status().contains("H.264"));
        assert!(calls.borrow().is_empty(), "engine must not start");
    }

    // --- allowed media plays and the discovered duration wins ------------
    {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let probe = MockProbe::new(MediaInfo {
            duration: Some(Duration::from_secs(232)),
            audio: vec!["audio/x-vorbis".into()],
            video: Vec::new(),
            analysis_failed: false,
        });
        let controller = controller(probe, calls.clone());
        assert_eq!(controller.probe_name(), "mock probe");
        controller.play();
        assert_eq!(controller.transport().state(), PlaybackState::Playing);
        assert_eq!(controller.transport().duration(), Duration::from_secs(232));
        assert!(controller.status().is_empty());
        assert_eq!(
            calls.borrow().as_slice(),
            &["play file:///home/aether/Music/glass-horizon.flac".to_string()]
        );
    }

    // --- without a discoverer playback still reaches the engine ----------
    {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let controller = controller(MockProbe::unavailable(), calls.clone());
        controller.play();
        assert_eq!(controller.transport().state(), PlaybackState::Playing);
        assert_eq!(calls.borrow().len(), 1);
        assert!(controller.status().is_empty());
    }

    // --- the surface's Play button goes through the same gate ------------
    {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let probe = MockProbe::new(MediaInfo {
            duration: None,
            audio: vec!["audio/mpeg, mpegversion=(int)4".into()],
            video: Vec::new(),
            analysis_failed: false,
        });
        let controller = controller(probe, calls.clone());
        controller.play(); // mirrored by the callback below
        controller.ui().show().unwrap();
        assert!(controller.ui().get_engine_status().contains("backend"));
        assert!(calls.borrow().is_empty());
    }
}
