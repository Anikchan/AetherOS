//! `aetherctl` — configuration and shell control CLI (P7-T03, F4 section 6).
//!
//! Usage:
//!   aetherctl config get KEY | set KEY VALUE | reset KEY [--file F | --all]
//!   aetherctl config check
//!   aetherctl reload
//!   aetherctl theme show
//!   aetherctl keybindings list|check
//!   aetherctl hooks list|run EVENT|enable NAME|disable NAME
//!   aetherctl features list|install ID|remove ID|disable ID|enable ID
//!   aetherctl apps list|suspend APP-ID|quit APP-ID
//!   aetherctl version

use std::path::PathBuf;
use std::time::Duration;

use aether_config::hooks::{HookEvent, HookManager, LuaCliRunner};
use aether_config::keybindings::BindingSet;
use aether_config::store::{ConfigStore, Roots};
use aether_config::theme::{ThemeEngine, ThemeTokens};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut roots = Roots::for_user();
    let mut extra_root: Option<PathBuf> = None;
    let mut file_target: Option<String> = None;
    let mut all = false;
    let mut positional: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                index += 1;
                extra_root = args.get(index).map(PathBuf::from);
            }
            "--file" => {
                index += 1;
                file_target = args.get(index).cloned();
            }
            "--all" => all = true,
            "-h" | "--help" => {
                println!("usage: aetherctl <config|reload|theme|keybindings|hooks|features|apps|version> [args]");
                return;
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    if let Some(root) = extra_root {
        roots = Roots::at(root);
    }
    let command_owned = if positional.is_empty() {
        "version".to_string()
    } else {
        positional.remove(0)
    };
    let command = command_owned.as_str();

    match command {
        "version" => {
            println!("AetherOS 0.1.0");
            println!("lifecycled {}", env!("CARGO_PKG_VERSION"));
            println!("powerd {}", env!("CARGO_PKG_VERSION"));
            println!("config schema keys: {}", aether_config::SCHEMA.len());
            println!(
                "schema files: {}",
                aether_config::schema_files().join(", ")
            );
        }
        "config" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("check");
            let mut store = ConfigStore::load(roots.clone());
            let key = positional.get(1).cloned();
            match action {
                "get" => {
                    let Some(key) = key else {
                        eprintln!("config get needs KEY");
                        std::process::exit(2);
                    };
                    match store.get(&key) {
                        Ok(value) => {
                            let source = store
                                .source(&key)
                                .map(|(layer, path)| {
                                    format!("{} ({})", path.display(), layer.label())
                                })
                                .unwrap_or_else(|| "default".into());
                            println!("{value}");
                            eprintln!("# {key} from {source}");
                        }
                        Err(error) => fail(&error.to_string()),
                    }
                }
                "set" => {
                    let (Some(key), Some(value)) = (key, positional.get(2)) else {
                        eprintln!("config set needs KEY VALUE");
                        std::process::exit(2);
                    };
                    if let Err(error) = store.set(&key, value) {
                        fail(&error.to_string());
                    }
                    println!("SET {key}={}", store.get(&key).unwrap());
                }
                "reset" => {
                    if all {
                        match store.reset_all() {
                            Ok(removed) => println!("RESET all files={removed}"),
                            Err(error) => fail(&error.to_string()),
                        }
                    } else if let Some(file) = file_target {
                        match store.reset_file(&file) {
                            Ok(removed) => println!("RESET file={file} removed={removed}"),
                            Err(error) => fail(&error.to_string()),
                        }
                    } else if let Some(key) = key {
                        match store.reset_key(&key) {
                            Ok(removed) => println!("RESET {key} removed={removed}"),
                            Err(error) => fail(&error.to_string()),
                        }
                    } else {
                        eprintln!("config reset needs KEY, --file FILE or --all");
                        std::process::exit(2);
                    }
                }
                "check" => {
                    let errors = store.check();
                    if errors.is_empty() {
                        println!("CONFIG_OK keys={}", store.effective().len());
                    } else {
                        for error in &errors {
                            eprintln!("{error}");
                        }
                        eprintln!("CONFIG_INVALID errors={}", errors.len());
                        std::process::exit(1);
                    }
                }
                other => {
                    eprintln!("unknown config action: {other}");
                    std::process::exit(2);
                }
            }
        }
        "reload" => {
            let store = ConfigStore::load(roots.clone());
            let mut engine = ThemeEngine::new(roots.user.join("theme/tokens.toml"));
            let reload = engine.load();
            if let Some(error) = reload.error {
                eprintln!("theme reload rejected: {error}");
                std::process::exit(1);
            }
            println!(
                "RELOADED theme_accent={} config_errors={}",
                engine.tokens().accent,
                store.check().len()
            );
        }
        "theme" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("show");
            let mut engine = ThemeEngine::new(roots.user.join("theme/tokens.toml"));
            engine.load();
            match action {
                "reload" => {
                    let reload = engine.reload_if_changed();
                    println!(
                        "THEME_RELOADED changed={} error={}",
                        reload.changed,
                        reload.error.unwrap_or_else(|| "none".into())
                    );
                }
                _ => {
                    let tokens: &ThemeTokens = engine.tokens();
                    println!("{}", toml::to_string_pretty(tokens).unwrap_or_default());
                }
            }
        }
        "keybindings" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("list");
            let path = roots.user.join("input/keybindings.toml");
            let set = BindingSet::load(&path)
                .unwrap_or_else(|error| fail(&error.to_string()))
                .merge_with_defaults();
            for conflict in &set.conflicts {
                eprintln!("CONFLICT {conflict}");
            }
            if action == "check" {
                if set.conflicts.is_empty() {
                    println!("KEYBINDINGS_OK count={}", set.bindings.len());
                } else {
                    std::process::exit(1);
                }
            } else {
                for binding in &set.bindings {
                    println!("{}\t{}", binding.key, binding.action);
                }
            }
        }
        "hooks" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("list");
            let scripts = roots.user.join("scripts");
            let disabled = roots.user.join("hooks-disabled.json");
            let mut manager =
                HookManager::load(&scripts, &disabled).with_timeout(Duration::from_millis(200));
            match action {
                "list" => {
                    for script in manager.scripts() {
                        let state = if manager.disabled().contains_key(&script.name) {
                            "disabled"
                        } else {
                            "enabled"
                        };
                        println!(
                            "{}\t{}\t{}\t{}",
                            script.name,
                            script.event.as_str(),
                            state,
                            script.path.display()
                        );
                    }
                }
                "run" => {
                    let Some(event) = positional
                        .get(1)
                        .and_then(|value| HookEvent::parse(value))
                    else {
                        eprintln!("hooks run needs one of: on_login, on_focus_change, on_window_open, on_window_close, on_network_change, on_power_change");
                        std::process::exit(2);
                    };
                    let runner = LuaCliRunner::system();
                    if !runner.available() {
                        eprintln!("lua interpreter not found; hooks cannot run");
                        std::process::exit(1);
                    }
                    let results = manager.dispatch(event, &runner);
                    for result in &results {
                        println!(
                            "{}\t{}\t{}",
                            result.script,
                            if result.ok { "OK" } else { "FAILED" },
                            result.error.clone().unwrap_or_default()
                        );
                    }
                    if results.is_empty() {
                        println!("No hooks for {}", event.as_str());
                    }
                }
                "enable" | "disable" => {
                    let Some(name) = positional.get(1) else {
                        eprintln!("hooks {action} needs NAME");
                        std::process::exit(2);
                    };
                    let result = if action == "enable" {
                        manager.enable(name)
                    } else {
                        manager.disable(name, "disabled by user")
                    };
                    match result {
                        Ok(()) => println!("{action} {name}"),
                        Err(error) => fail(&error.to_string()),
                    }
                }
                other => {
                    eprintln!("unknown hooks action: {other}");
                    std::process::exit(2);
                }
            }
        }
        "features" => {
            let path = roots.user.join("features/features.toml");
            let mut manager = aether_config::FeatureManager::load(&path);
            let action = positional.first().map(|s| s.as_str()).unwrap_or("list");
            match action {
                "list" => {
                    for row in manager.rows() {
                        println!(
                            "{}\t{}\t{}\t{} MB\t{:.1}% idle\t{} MB idle\t{}",
                            row.id,
                            row.name,
                            row.status,
                            row.disk_used_mb,
                            row.idle_cpu_percent,
                            row.memory_mb,
                            row.description
                        );
                    }
                    println!("TOTAL_DISK {} MB", manager.total_disk_mb());
                }
                "install" | "preview" => {
                    let Some(id) = positional.get(1) else {
                        eprintln!("features {action} needs PACK-ID");
                        std::process::exit(2);
                    };
                    if action == "preview" {
                        match manager.cost_preview(id) {
                            Ok(row) => println!(
                                "{}\t{} MB\t{:.1}% idle\t{} MB idle",
                                row.name, row.disk_used_mb, row.idle_cpu_percent, row.memory_mb
                            ),
                            Err(error) => fail(&error.to_string()),
                        }
                        return;
                    }
                    match manager.install(id) {
                        Ok(row) => println!(
                            "INSTALLED {} {} ({} MB)",
                            row.id, row.name, row.disk_used_mb
                        ),
                        Err(error) => fail(&error.to_string()),
                    }
                }
                "remove" | "disable" | "enable" => {
                    let Some(id) = positional.get(1) else {
                        eprintln!("features {action} needs PACK-ID");
                        std::process::exit(2);
                    };
                    let result = match action {
                        "remove" => manager.remove(id),
                        "disable" => manager.set_enabled(id, false),
                        _ => manager.set_enabled(id, true),
                    };
                    match result {
                        Ok(()) => println!("{action} {id}"),
                        Err(error) => fail(&error.to_string()),
                    }
                }
                other => {
                    eprintln!("unknown features action: {other}");
                    std::process::exit(2);
                }
            }
        }
        "apps" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("list");
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"));
            let state = std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/state"));
            let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"));
            let status_path = state.join("aether/lifecycled/status.json");
            if action == "list" {
                match std::fs::read_to_string(&status_path) {
                    Ok(text) => {
                        let status: serde_json::Value =
                            serde_json::from_str(&text).unwrap_or_default();
                        if let Some(rows) = status.get("rows").and_then(|rows| rows.as_array()) {
                            for row in rows {
                                println!(
                                    "{}\t{}\t{}\t{} bytes",
                                    row.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                    row.get("state").and_then(|v| v.as_str()).unwrap_or("?"),
                                    row.get("band").and_then(|v| v.as_str()).unwrap_or("?"),
                                    row.get("memory_current")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                );
                            }
                        } else {
                            println!("No apps reported (lifecycled not running?)");
                        }
                    }
                    Err(_) => println!(
                        "No lifecycle status at {} (lifecycled not running?)",
                        status_path.display()
                    ),
                }
                return;
            }
            let Some(app) = positional.get(1) else {
                eprintln!("apps {action} needs APP-ID");
                std::process::exit(2);
            };
            if action != "suspend" && action != "quit" {
                eprintln!("unknown apps action: {action}");
                std::process::exit(2);
            }
            let control = runtime.join("aether/lifecycled-control.jsonl");
            if let Some(parent) = control.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let line = format!("{{\"command\":\"{action}\",\"app\":\"{app}\"}}\n");
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&control)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    if let Err(error) = file.write_all(line.as_bytes()) {
                        fail(&error.to_string());
                    }
                    println!("QUEUED {action} {app} -> {}", control.display());
                }
                Err(error) => fail(&error.to_string()),
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}

fn fail(message: &str) -> ! {
    eprintln!("ERROR {message}");
    std::process::exit(1);
}
