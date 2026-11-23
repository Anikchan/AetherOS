//! SDK test app for the lifecycle protocol (P5-T03).
//!
//! Binds the app socket, then on `WillSuspend` saves state to a file and
//! acknowledges. Markers printed to stdout are used by the acceptance test:
//!
//! - `LIFECYCLE_TESTAPP_READY <app> <socket>`
//! - `LIFECYCLE_TESTAPP_SAVED <app> <state-file>`
//! - `LIFECYCLE_TESTAPP_ACKED <app>`
//!
//! Usage: aether-lifecycle-testapp --id org.example.TestApp
//!        [--socket-dir DIR] [--state-file PATH] [--timeout SECONDS]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aether_lifecycled::identity::AppId;
use aether_lifecycled::protocol::{LifecycleServer, SuspendHandler};

struct Saver {
    state_file: PathBuf,
    saves: AtomicUsize,
    saved: AtomicBool,
}

impl Saver {
    fn new(state_file: PathBuf) -> Self {
        Self {
            state_file,
            saves: AtomicUsize::new(0),
            saved: AtomicBool::new(false),
        }
    }
}

impl SuspendHandler for Saver {
    fn on_will_suspend(&self, id: &AppId) {
        // Simulated app state: cursor position, open documents, unsaved text.
        let text = format!(
            "app={id}\nsaved_at={}\nopen_documents=2\ncursor=42\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        if let Some(parent) = self.state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&self.state_file, text) {
            Ok(()) => {
                self.saved.store(true, Ordering::SeqCst);
                self.saves.fetch_add(1, Ordering::SeqCst);
                println!("LIFECYCLE_TESTAPP_SAVED {id} {}", self.state_file.display());
            }
            Err(e) => eprintln!("failed to save state: {e}"),
        }
    }
}

fn main() {
    let mut id = None;
    let mut socket_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("aether/lifecycle");
    let mut state_file = PathBuf::from("/tmp/aether-testapp-state.txt");
    let mut timeout = Duration::from_secs(60);

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--id" => id = args.next(),
            "--socket-dir" => {
                socket_dir = args.next().map(PathBuf::from).unwrap_or_else(|| {
                    eprintln!("--socket-dir needs a value");
                    std::process::exit(2);
                })
            }
            "--state-file" => {
                state_file = args.next().map(PathBuf::from).unwrap_or_else(|| {
                    eprintln!("--state-file needs a value");
                    std::process::exit(2);
                })
            }
            "--timeout" => {
                let secs = args
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(60);
                timeout = Duration::from_secs(secs);
            }
            "-h" | "--help" => {
                println!("usage: aether-lifecycle-testapp --id <app-id> [--socket-dir DIR] [--state-file PATH] [--timeout SECONDS]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let Some(raw) = id else {
        eprintln!("--id is required");
        std::process::exit(2);
    };
    let app = match AppId::new(raw.clone()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("invalid app id {raw:?}: {e}");
            std::process::exit(2);
        }
    };

    let server = match LifecycleServer::bind(&socket_dir, &app) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("bind failed in {}: {e}", socket_dir.display());
            std::process::exit(1);
        }
    };
    println!(
        "LIFECYCLE_TESTAPP_READY {app} {}",
        server.path().display()
    );

    let saver = Arc::new(Saver::new(state_file));
    let stop = server.stop_handle();
    let handle = server.spawn(app.clone(), saver.clone());
    let started = Instant::now();
    while started.elapsed() < timeout {
        if saver.saved.load(Ordering::SeqCst) {
            println!("LIFECYCLE_TESTAPP_ACKED {app}");
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    stop.store(true, Ordering::SeqCst);
    let _ = handle.join();
    if saver.saved.load(Ordering::SeqCst) {
        println!(
            "LIFECYCLE_TESTAPP_OK {app} saves={}",
            saver.saves.load(Ordering::SeqCst)
        );
    } else {
        println!("LIFECYCLE_TESTAPP_NO_MESSAGE {app}");
    }
}
