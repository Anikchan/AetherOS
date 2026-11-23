//! `aether-power-report` — text battery report (F14 BAT-10 style).
//!
//! Usage: aether-power-report [--state PATH] [--json]

use std::path::PathBuf;

use aether_powerd::analytics::BatteryAnalytics;

fn default_state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/var/lib"))
        .join("aether/powerd/battery-history.json")
}

fn main() {
    let mut path = default_state();
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--state" => path = PathBuf::from(args.next().unwrap_or_else(|| {
                eprintln!("--state needs a path");
                std::process::exit(2);
            })),
            "--json" => json = true,
            "-h" | "--help" => {
                println!("usage: aether-power-report [--state PATH] [--json]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let analytics = match BatteryAnalytics::load(&path) {
        Ok(analytics) => analytics,
        Err(error) => {
            eprintln!("no battery history at {}: {error}", path.display());
            eprintln!("Run aether-powerd first; missing history is reported, not invented.");
            std::process::exit(1);
        }
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&analytics).unwrap_or_else(|_| "{}".into())
        );
    } else {
        print!("{}", analytics.report());
    }
}
