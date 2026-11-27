//! Launch handshake for Files (P4 launch path).
//!
//! `--launch <own id>` must continue into the app (the shell spawns it on a
//! dock click), while `--launch-check <own id>` is the dry handshake. No window
//! backend is needed, so this runs headless.

use aether_launcher_registry::{entries, launch_argv, launch_decision, LaunchDecision};

const EXEC: &str = "aether-files";

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|value| value.to_string()).collect()
}

#[test]
fn launch_decision_matches_this_app() {
    assert_eq!(
        launch_decision(EXEC, &args(&[])),
        LaunchDecision::Continue { marker: None }
    );
    assert_eq!(
        launch_argv(EXEC),
        Some(vec![
            EXEC.to_string(),
            "--launch".to_string(),
            EXEC.to_string()
        ])
    );
    assert_eq!(
        launch_decision(EXEC, &args(&["--launch", EXEC])),
        LaunchDecision::Continue {
            marker: Some(format!("AETHER_LAUNCH {EXEC}"))
        }
    );
    assert_eq!(
        launch_decision(EXEC, &args(&["--launch-check", EXEC])),
        LaunchDecision::Exit {
            code: 0,
            marker: Some(format!("AETHER_LAUNCH {EXEC}")),
            message: None
        }
    );
    let foreign = entries()
        .iter()
        .map(|entry| entry.id)
        .find(|id| *id != EXEC)
        .expect("another app");
    match launch_decision(EXEC, &args(&["--launch", foreign])) {
        LaunchDecision::Exit { code: 2, .. } => {}
        other => panic!("foreign id decided {other:?}"),
    }
    match launch_decision(EXEC, &args(&["--launch"])) {
        LaunchDecision::Exit { code: 2, .. } => {}
        other => panic!("missing id decided {other:?}"),
    }
}
