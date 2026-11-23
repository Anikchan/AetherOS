//! Headless interaction test for the Player window (P4-T14).
//!
//! One test per file: the testing backend installs a process-wide platform.

use std::time::Duration;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model};

use aether_player::demo_playlist;
use aether_player::engine::MockEngine;
use aether_player::playlist::Repeat;
use aether_player::{PlaybackState, PlayerApp, PlayerController};

/// Click the visible instance with this accessible label.
async fn click(ui: &PlayerApp, label: &str) {
    let mut candidates: Vec<ElementHandle> = ElementQuery::from_root(ui)
        .match_accessible_label(label)
        .find_all()
        .into_iter()
        .filter(|handle| handle.size().width > 0.0 && handle.size().height > 0.0)
        .collect();
    candidates.sort_by(|a, b| {
        let area = |handle: &ElementHandle| handle.size().width * handle.size().height;
        area(b).partial_cmp(&area(a)).unwrap()
    });
    let handle = candidates
        .first()
        .unwrap_or_else(|| panic!("no visible element labelled {label:?}"));
    handle.single_click(PointerEventButton::Left).await;
}

#[test]
fn player_window_clicking_drives_the_transport() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    slint::spawn_local(async move {
        let ui = PlayerApp::new().unwrap();
        let controller = PlayerController::new(ui, demo_playlist(7), Box::new(MockEngine::new()));
        controller.ui().show().unwrap();
        controller
            .ui()
            .window()
            .set_size(slint::PhysicalSize::new(1200, 800));

        // --- initial state from Rust ---------------------------------------
        assert_eq!(controller.current_title().as_deref(), Some("Glass Horizon"));
        assert_eq!(controller.transport().state(), PlaybackState::Stopped);
        assert_eq!(controller.ui().get_tracks().row_count(), 6);
        assert_eq!(controller.ui().get_now_title().to_string(), "Glass Horizon");
        assert_eq!(controller.ui().get_now_total().to_string(), "3:52");
        assert_eq!(controller.ui().get_repeat_label().to_string(), "Repeat all");

        // --- play / pause ---------------------------------------------------
        click(controller.ui(), "Play").await;
        assert!(controller.transport().is_playing(), "Play starts playback");
        assert!(controller.ui().get_playing());
        click(controller.ui(), "Pause").await;
        assert_eq!(controller.transport().state(), PlaybackState::Paused);
        assert!(!controller.ui().get_playing());
        click(controller.ui(), "Play").await;
        assert!(controller.transport().is_playing());

        // --- next / shuffle / repeat ---------------------------------------
        click(controller.ui(), "Next").await;
        assert_eq!(controller.current_title().as_deref(), Some("Slow Orbit"));
        assert_eq!(controller.ui().get_now_title().to_string(), "Slow Orbit");
        assert!(controller.transport().is_playing(), "next keeps playing");

        click(controller.ui(), "Shuffle").await;
        assert!(controller.playlist().is_shuffled());
        assert!(controller.ui().get_shuffle());
        assert_eq!(
            controller.playlist().order().len(),
            6,
            "shuffle reorders without losing tracks"
        );

        click(controller.ui(), "Repeat").await;
        assert_eq!(controller.playlist().repeat(), Repeat::One);
        assert_eq!(controller.ui().get_repeat_code(), 2);
        assert_eq!(controller.ui().get_repeat_label().to_string(), "Repeat one");

        // --- pick a track from the queue -----------------------------------
        click(controller.ui(), "Deep Field").await;
        assert_eq!(controller.current_title().as_deref(), Some("Deep Field"));
        assert_eq!(controller.ui().get_now_total().to_string(), "5:01");
        assert!(controller.transport().is_playing());

        // --- seek with the progress slider ---------------------------------
        click(controller.ui(), "Progress").await;
        let position = controller.transport().position();
        assert!(
            position > Duration::from_secs(100) && position < Duration::from_secs(200),
            "clicking the middle of the slider seeks to about half the track, got {position:?}"
        );
        let progress = controller.ui().get_progress();
        assert!(progress > 0.35 && progress < 0.65, "progress {progress}");

        // --- repeat one keeps the same track at the end --------------------
        controller.tick(Duration::from_secs(400));
        assert_eq!(
            controller.current_title().as_deref(),
            Some("Deep Field"),
            "repeat one replays the same track"
        );
        assert_eq!(controller.transport().position(), Duration::ZERO);
        assert!(controller.transport().is_playing());

        // --- repeat off advances to the arrangement's next track -----------
        click(controller.ui(), "Repeat").await;
        assert_eq!(controller.playlist().repeat(), Repeat::Off);
        click(controller.ui(), "Shuffle").await;
        assert!(!controller.playlist().is_shuffled(), "shuffle off restores the queue order");
        controller.tick(Duration::from_secs(400));
        assert_eq!(
            controller.current_title().as_deref(),
            Some("Drift"),
            "without repeat the player advances to the next track"
        );
        assert!(controller.transport().is_playing());
        assert_eq!(controller.ui().get_now_title(), controller.current_title().unwrap().as_str());

        slint::quit_event_loop().unwrap();
    })
    .unwrap();
    slint::run_event_loop().unwrap();
}
