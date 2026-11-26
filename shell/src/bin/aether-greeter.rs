//! `aether-greeter` — the AetherOS login greeter (P4-T06, SHL-AC-08).
//!
//! greetd starts this binary as the `greeter` user. It authenticates through
//! greetd's IPC (greetd calls PAM as root), then asks greetd to start the
//! session command from `/etc/aether/greetd/greeter.toml`.
//!
//! ```text
//! aether-greeter --render <dir>   render the login surface to PNG
//! aether-greeter --stdin          log in interactively on the terminal
//! aether-greeter --status         print backend and socket status
//! aether-greeter --help
//! ```
//!
//! The Slint window is hosted by the layer-shell host (P3-T02); until that
//! lands, `--render` and `--stdin` are the reproducible ways to exercise the
//! greeter, including in the QEMU image.

use std::io::{self, Write};
use std::path::PathBuf;

use aether_shell::auth::{pam_linked, Authenticator, LocalCredentials};
use aether_shell::greeter::{GreeterConfig, GreetdSession, LoginEvent, LoginSession};
use aether_shell::greeter_app::{DemoSession, GreeterController};
use aether_shell::{GreetdConnection, GreeterRoot};

const DEFAULT_CONFIG: &str = "/etc/aether/greetd/greeter.toml";

fn usage() {
    println!(
        "aether-greeter — AetherOS login greeter (P4-T06)\n\
         \n\
         Usage:\n\
         \x20 aether-greeter --render <dir>   render the login surface to PNG\n\
         \x20 aether-greeter --stdin          log in interactively on the terminal\n\
         \x20 aether-greeter --status         print backend and socket status\n\
         \x20 aether-greeter --help           this message\n\
         \n\
         Options:\n\
         \x20 --config <path>               greeter.toml (default {DEFAULT_CONFIG})\n\
         \x20 --user <name>                 pre-fill the user name"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = load_config(&args);
    match args.first().map(String::as_str) {
        Some("--render") => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("screenshots"));
            render(&dir)?;
        }
        Some("--stdin") => {
            let user = arg_value(&args, "--user").unwrap_or_else(|| config.user.clone());
            std::process::exit(terminal_login(&config, &user));
        }
        Some("--status") => status(&config),
        Some("--help") | None => usage(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1).cloned())
}

fn load_config(args: &[String]) -> GreeterConfig {
    let path = arg_value(args, "--config")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    match GreeterConfig::load(&path) {
        Ok(config) => config,
        Err(error) => {
            // Running without a config is fine: the defaults start aether-shell.
            eprintln!("aether-greeter: {error}; using defaults");
            GreeterConfig::default()
        }
    }
}

fn status(config: &GreeterConfig) {
    let socket = GreetdConnection::<std::os::unix::net::UnixStream>::socket_path();
    println!("greetd-socket: {}", socket.display());
    println!("greetd-socket-exists: {}", socket.exists());
    println!("session: {}", config.session_label());
    println!("session-env: {}", config.env.join(" "));
    println!("user: {}", config.user);
    println!("max-attempts: {}", config.max_attempts);
    println!("pam-linked: {}", pam_linked());
    println!(
        "lock-backend: {}",
        LocalCredentials::default().name()
    );
    println!("AETHER_GREETER_STATUS_OK");
}

fn render(dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    aether_ui::render::init()?;
    std::fs::create_dir_all(dir)?;
    let config = GreeterConfig::default();

    let ui = GreeterRoot::new()?;
    let controller = GreeterController::new(
        ui,
        Box::new(DemoSession::new("aether")),
        config.clone(),
    );
    // Two deterministic frames: the idle form and a rejected password.
    let idle = controller.ui();
    let path = dir.join("greeter-login.png");
    aether_ui::render::render_png(idle, 1440, 900, &path)?;
    println!("AETHER_GREETER_RENDERED login -> {}", path.display());

    let failed = GreeterRoot::new()?;
    let failed_controller = GreeterController::new(
        failed,
        Box::new(DemoSession::new("aether")),
        config,
    );
    failed_controller.submit("aether", "wrong");
    let path = dir.join("greeter-denied.png");
    aether_ui::render::render_png(failed_controller.ui(), 1440, 900, &path)?;
    println!("AETHER_GREETER_RENDERED denied -> {}", path.display());
    println!("AETHER_GREETER_RENDER_DONE");
    Ok(())
}

/// Interactive login on the terminal, driven by the real greetd socket.
fn terminal_login(config: &GreeterConfig, user: &str) -> i32 {
    let connection = match GreetdConnection::connect() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("aether-greeter: {error}");
            return 1;
        }
    };
    let mut session = GreetdSession::with_max_attempts(connection, config.max_attempts);
    let mut event = session.start(user);
    loop {
        match event {
            LoginEvent::Prompt(prompt) => {
                print!("{}", prompt.message);
                let _ = io::stdout().flush();
                let mut answer = String::new();
                if io::stdin().read_line(&mut answer).unwrap_or(0) == 0 {
                    session.cancel();
                    return 1;
                }
                event = session.respond(answer.trim_end_matches(['\r', '\n']));
            }
            LoginEvent::Authenticated => {
                if let Err(error) = session.begin_session(&config.session, &config.env) {
                    eprintln!("aether-greeter: {error}");
                    return 1;
                }
                println!("AETHER_GREETER_SESSION_STARTED {}", config.session_label());
                return 0;
            }
            LoginEvent::Denied {
                message,
                attempts_left,
            } => {
                eprintln!("{message} ({attempts_left} attempts left)");
                event = session.start(user);
            }
            LoginEvent::LockedOut { message } => {
                eprintln!("{message}");
                return 1;
            }
            LoginEvent::Unavailable(message) | LoginEvent::Error(message) => {
                eprintln!("aether-greeter: {message}");
                return 1;
            }
        }
    }
}
