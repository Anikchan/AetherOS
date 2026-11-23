//! `aether-limits` — Screen Time limits and downtime (Pack E, P7-PK-T02).
//!
//! Usage:
//!   aether-limits status [--config F --state F]
//!   aether-limits set APP MINUTES [--extension MINUTES]
//!   aether-limits downtime START END [--apps a,b] [--label TEXT]
//!   aether-limits record APP SECONDS --day DAY
//!   aether-limits decide APP --day DAY --minute MINUTE

use std::path::PathBuf;

use aether_powerd::limits::{decide, DowntimeWindow, LimitRule, LimitsConfig, LimitsState};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("status");
    let mut config_path = default_config();
    let mut state_path = default_state();
    let mut day: i64 = today_epoch_day();
    let mut minute: u32 = 12 * 60;
    let mut apps: Vec<String> = Vec::new();
    let mut label = "Downtime".to_string();
    let mut extension = 0u32;
    let mut positional = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    config_path = PathBuf::from(value);
                }
            }
            "--state" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    state_path = PathBuf::from(value);
                }
            }
            "--day" => {
                index += 1;
                day = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(day);
            }
            "--minute" => {
                index += 1;
                minute = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(minute);
            }
            "--apps" => {
                index += 1;
                apps = args
                    .get(index)
                    .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default();
            }
            "--label" => {
                index += 1;
                label = args.get(index).cloned().unwrap_or(label);
            }
            "--extension" => {
                index += 1;
                extension = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "-h" | "--help" => {
                println!("usage: aether-limits <status|set|downtime|record|decide> [args]");
                return;
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }

    let mut config = LimitsConfig::load(&config_path).unwrap_or_default();
    let mut state = LimitsState::load(&state_path).unwrap_or_default();

    match command {
        "status" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "day": day,
                    "rules": config.rules,
                    "downtime": config.downtime,
                    "used": state.used_seconds.get(&day),
                }))
                .unwrap_or_default()
            );
        }
        "set" => {
            let (Some(app), Some(minutes)) = (positional.first(), positional.get(1)) else {
                eprintln!("set needs APP MINUTES");
                std::process::exit(2);
            };
            let minutes: u32 = minutes.parse().unwrap_or_else(|_| {
                eprintln!("minutes must be a number");
                std::process::exit(2);
            });
            config.rules.retain(|rule| &rule.app != app);
            config.rules.push(LimitRule {
                app: app.clone(),
                daily_minutes: minutes,
                extension_minutes: extension,
            });
            config.save(&config_path).unwrap_or_else(|error| {
                eprintln!("save failed: {error}");
                std::process::exit(1);
            });
            println!("LIMIT_SET {app} {minutes}m extension={extension}m");
        }
        "downtime" => {
            let (Some(start), Some(end)) = (positional.first(), positional.get(1)) else {
                eprintln!("downtime needs START_MINUTE END_MINUTE");
                std::process::exit(2);
            };
            let start: u32 = start.parse().unwrap_or(0);
            let end: u32 = end.parse().unwrap_or(0);
            config.downtime.push(DowntimeWindow {
                start_minute: start,
                end_minute: end,
                apps,
                label,
            });
            config.save(&config_path).unwrap_or_else(|error| {
                eprintln!("save failed: {error}");
                std::process::exit(1);
            });
            println!("DOWNTIME_SET start={start} end={end}");
        }
        "record" => {
            let (Some(app), Some(seconds)) = (positional.first(), positional.get(1)) else {
                eprintln!("record needs APP SECONDS");
                std::process::exit(2);
            };
            let seconds: u64 = seconds.parse().unwrap_or(0);
            state.record(day, app, seconds);
            state.save(&state_path).unwrap_or_else(|error| {
                eprintln!("save failed: {error}");
                std::process::exit(1);
            });
            println!("USAGE_RECORDED {app} {seconds}s day={day}");
        }
        "decide" => {
            let Some(app) = positional.first() else {
                eprintln!("decide needs APP");
                std::process::exit(2);
            };
            match decide(&config, &state, app, day, minute) {
                decision => {
                    println!("{}", serde_json::to_string_pretty(&format!("{decision:?}")).unwrap_or_default());
                    if decision.blocked() {
                        std::process::exit(3);
                    }
                }
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}

fn default_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/etc"))
        .join("aether")
}

fn default_config() -> PathBuf {
    default_dir().join("limits.toml")
}

fn default_state() -> PathBuf {
    default_dir().join("limits-state.json")
}

fn today_epoch_day() -> i64 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        / 86_400) as i64
}
