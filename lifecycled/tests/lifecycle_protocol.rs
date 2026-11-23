//! P5-T03 acceptance test: the SDK test app saves state when the daemon sends
//! `WillSuspend` and acknowledges the message.
//!
//! This test runs the real binaries and a real Unix socket, not fakes.

use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aether_lifecycled::identity::AppId;
use aether_lifecycled::protocol::{LifecycleMessage, LifecycleNotifier, SocketNotifier};

fn short_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = PathBuf::from("/tmp").join(format!(
        "aet-it-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_app_saves_state_on_will_suspend() {
    let dir = short_dir("p5t03");
    let state_file = dir.join("app-state.txt");
    let app = AppId::new("org.example.TestApp").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_aether-lifecycle-testapp"))
        .args([
            "--id",
            app.as_str(),
            "--socket-dir",
            dir.to_str().unwrap(),
            "--state-file",
            state_file.to_str().unwrap(),
            "--timeout",
            "20",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn test app");

    // Wait for the READY marker line.
    let stdout = child.stdout.take().expect("stdout");
    let reader = std::io::BufReader::new(stdout);
    let mut saw_ready = false;
    let mut lines = reader.lines();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match lines.next() {
            Some(Ok(line)) => {
                if line.starts_with("LIFECYCLE_TESTAPP_READY") {
                    saw_ready = true;
                    break;
                }
            }
            Some(Err(_)) | None => break,
        }
    }
    assert!(saw_ready, "test app did not become ready");

    let notifier = SocketNotifier::at(&dir);
    let ack = notifier
        .notify(&app, LifecycleMessage::WillSuspend)
        .expect("will-suspend delivered");
    assert_eq!(ack, aether_lifecycled::protocol::Ack::Accepted);

    let drain = std::thread::spawn(move || {
        for line in lines {
            let Ok(line) = line else { break };
            if line.contains("LIFECYCLE_TESTAPP_OK") {
                return true;
            }
        }
        false
    });
    let status = child.wait().expect("test app exit");
    assert!(status.success(), "test app exited with {status}");
    assert!(drain.join().unwrap(), "test app did not report success");

    let saved = std::fs::read_to_string(&state_file).expect("state file written");
    assert!(saved.contains("open_documents=2"));
    assert!(saved.contains(app.as_str()));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_app_gets_did_resume_without_waiting_for_ack() {
    let dir = short_dir("p5t03b");
    let state_file = dir.join("app-state.txt");
    let app = AppId::new("org.example.TestApp2").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_aether-lifecycle-testapp"))
        .args([
            "--id",
            app.as_str(),
            "--socket-dir",
            dir.to_str().unwrap(),
            "--state-file",
            state_file.to_str().unwrap(),
            "--timeout",
            "2",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn test app");

    let notifier = SocketNotifier::at(&dir);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !notifier.socket_path(&app).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        notifier.socket_path(&app).exists(),
        "test app did not bind its socket"
    );
    assert_eq!(
        notifier
            .notify(&app, LifecycleMessage::DidResume)
            .expect("did-resume"),
        aether_lifecycled::protocol::Ack::Accepted
    );
    let _ = child.wait();

    let _ = std::fs::remove_dir_all(dir);
}
