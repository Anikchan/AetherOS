//! `aether-browser` (P4-T18 core, T18b data, T18a media).
//!
//! Usage:
//!   aether-browser --render <dir>        write app-browser.png (headless evidence)
//!   aether-browser --media-status        codec/hardware/DRM capability report
//!   aether-browser --media-check <url>   classify a media URL and print its plan
//!   aether-browser --security-status     security baseline and update channel
//!   aether-browser --security-check      the same, and the engine must be installed
//!   aether-browser --help

use aether_browser::security::{SecurityPolicy, WebEngine};
use aether_browser::{media, BrowserApp, BrowserController};
use aether_media::MediaCapabilities;

fn usage() {
    println!(
        "aether-browser — AetherOS default browser (P4-T18)\n\
         \n\
         \x20 --render <dir>       render the browser window to <dir>/app-browser.png\n\
         \x20 --media-status       print media capabilities (acceleration, codecs, DRM)\n\
         \x20 --media-check <url>  classify a media URL and print its playback plan\n\
         \x20 --security-status    print the security baseline and update channel\n\
         \x20 --security-check     as --security-status, and require the WebKitGTK engine\n\
         \x20 --help               this message"
    );
}

fn media_status() {
    let capabilities = MediaCapabilities::detect();
    println!("AETHER_BROWSER_MEDIA_OK");
    for line in capabilities.status_lines() {
        println!("{line}");
    }
    println!("streaming=hls,dash");
}

fn media_check(url: &str) -> i32 {
    let capabilities = MediaCapabilities::detect();
    let source = media::classify(url);
    let plan = media::plan_for(url, &capabilities);
    println!(
        "AETHER_BROWSER_MEDIA_CHECK kind={} playable={} reduced={}",
        source.label(),
        !plan.is_blocked(),
        plan.reduced
    );
    println!("message={}", plan.message);
    if plan.is_blocked() {
        3
    } else {
        0
    }
}

fn security_status(strict: bool) -> i32 {
    let root = SecurityPolicy::default_root();
    let policy = match SecurityPolicy::load(&root) {
        Ok(policy) => policy,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let engine = WebEngine::detect();
    let problems = if strict {
        policy.strict_problems(&engine)
    } else {
        policy.problems(&engine)
    };
    if problems.is_empty() {
        println!("AETHER_BROWSER_SECURITY_OK");
    } else {
        println!("AETHER_BROWSER_SECURITY_FAILED");
    }
    for line in policy.status_lines(&engine) {
        println!("{line}");
    }
    if engine.version.is_none() && !strict {
        println!(
            "AETHER_BROWSER_SECURITY_NOTE: WebKitGTK is not installed on this host (engine adapter only); --security-check runs on the image"
        );
    }
    for problem in &problems {
        println!("AETHER_BROWSER_SECURITY_PROBLEM: {problem}");
    }
    if problems.is_empty() {
        0
    } else if engine.verdict() == "outdated" {
        4
    } else {
        5
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--render") => {
            let dir = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "screenshots".into());
            aether_ui::render::init()?;
            let ui = BrowserApp::new()?;
            let controller = BrowserController::with_demo(ui);
            let path = std::path::Path::new(&dir).join("app-browser.png");
            let frame = aether_ui::render::render_png(controller.ui(), 1200, 800, &path)?;
            println!(
                "AETHER_APP_BROWSER_RENDERED {}x{} -> {}",
                frame.width,
                frame.height,
                path.display()
            );
        }
        Some("--media-status") => media_status(),
        Some("--media-check") => match args.get(1) {
            Some(url) => std::process::exit(media_check(url)),
            None => {
                eprintln!("--media-check needs a URL");
                std::process::exit(2);
            }
        },
        Some("--security-status") => std::process::exit(security_status(false)),
        Some("--security-check") => std::process::exit(security_status(true)),
        Some("--help") | None => usage(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}
