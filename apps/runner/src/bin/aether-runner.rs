//! `aether-runner` — launch apps, manage Windows prefixes, compatibility DB.
//!
//! Usage:
//!   aether-runner list [--root DIR]
//!   aether-runner plan APP-ID [--root DIR]
//!   aether-runner launch APP-ID [--root DIR] [--dry-run]
//!   aether-runner engines [--engine-root DIR]
//!   aether-runner prefixes list|remove APP-ID [--root DIR]
//!   aether-runner compat list|plan|set|report [--db FILE]

use std::collections::BTreeSet;
use std::path::PathBuf;

use aether_pkg::manifest::AppId;
use aether_pkg::{Paths, Registry};
use aether_runner::compat::{CompatDb, CompatTier};
use aether_runner::{EngineRegistry, PrefixManager, Runner};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("list");
    let mut root: Option<PathBuf> = None;
    let mut engine_root: Option<PathBuf> = None;
    let mut db_path = PathBuf::from("docs/2026/11/23/results/compat-db.json");
    let mut dry_run = false;
    let mut positional = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                index += 1;
                root = args.get(index).map(PathBuf::from);
            }
            "--engine-root" => {
                index += 1;
                engine_root = args.get(index).map(PathBuf::from);
            }
            "--db" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    db_path = PathBuf::from(value);
                }
            }
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                println!("usage: aether-runner <list|plan|launch|engines|prefixes|compat> [args]");
                return;
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }

    let paths = root.clone().map(Paths::at).unwrap_or_else(Paths::for_user);
    let engines = engine_root
        .clone()
        .map(EngineRegistry::at)
        .unwrap_or_else(EngineRegistry::system);

    match command {
        "list" => {
            let registry = Registry::load(&paths.registry);
            for app in registry.list() {
                println!(
                    "{}\t{}\t{}",
                    app.manifest.id,
                    app.manifest.version,
                    app.manifest.kind.as_str()
                );
            }
        }
        "plan" | "launch" => {
            let Some(raw) = positional.first() else {
                eprintln!("{command} needs APP-ID");
                std::process::exit(2);
            };
            let id = AppId::new(raw.clone()).unwrap_or_else(|error| {
                eprintln!("invalid app id: {error}");
                std::process::exit(2);
            });
            let runner = Runner::new(paths, engines);
            if command == "plan" {
                match runner.plan(&id) {
                    Ok(plan) => {
                        println!("{}", serde_json::to_string_pretty(&plan).unwrap_or_default());
                        println!("preview: {}", plan.preview());
                        for line in aether_sandbox::Enforcer::with_bwrap(
                            "/usr/bin/bwrap",
                            aether_sandbox::SandboxCapabilities {
                                bwrap: Some(PathBuf::from("/usr/bin/bwrap")),
                                user_namespaces: true,
                                landlock: false,
                            },
                        )
                        .describe(&plan.profile)
                        {
                            println!("  {line}");
                        }
                    }
                    Err(error) => {
                        eprintln!("plan failed: {error}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            let runner = if dry_run { runner.dry_run() } else { runner };
            match runner.launch(&id) {
                Ok(mut handle) => {
                    println!(
                        "LAUNCHED {} pid={} engine={}",
                        id,
                        handle.pid,
                        handle.plan.engine.label()
                    );
                    if handle.pid > 0 {
                        let _ = handle.try_wait();
                    }
                }
                Err(error) => {
                    eprintln!("launch failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        "engines" => {
            let versions = engines.wine_versions();
            if versions.is_empty() {
                println!("No Windows engine installed (Pack B).");
            } else {
                for version in versions {
                    println!("wine-{version}");
                }
            }
            println!(
                "system wine: {}",
                engines
                    .system_wine()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "not found".into())
            );
        }
        "prefixes" => {
            let manager = PrefixManager::new(&paths);
            match positional.first().map(|s| s.as_str()) {
                Some("remove") => {
                    let Some(raw) = positional.get(1) else {
                        eprintln!("prefixes remove needs APP-ID");
                        std::process::exit(2);
                    };
                    let id = AppId::new(raw.clone()).unwrap_or_else(|error| {
                        eprintln!("invalid app id: {error}");
                        std::process::exit(2);
                    });
                    if let Err(error) = manager.remove(&id) {
                        eprintln!("remove failed: {error}");
                        std::process::exit(1);
                    }
                    println!("PREFIX_REMOVED {id}");
                }
                _ => {
                    for prefix in manager.list() {
                        println!("{}", prefix.display());
                    }
                }
            }
        }
        "compat" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("report");
            if action == "plan" {
                let db = CompatDb::test_plan();
                let _ = db.save(&db_path);
                println!(
                    "COMPAT_PLAN entries={} db={}",
                    db.entries.len(),
                    db_path.display()
                );
                return;
            }
            if action == "set" {
                let (Some(name), Some(tier), Some(notes)) = (
                    positional.get(1),
                    positional.get(2),
                    positional.get(3),
                ) else {
                    eprintln!("compat set NAME TIER NOTES");
                    std::process::exit(2);
                };
                let tier = match tier.to_ascii_uppercase().as_str() {
                    "A" => CompatTier::A,
                    "B" => CompatTier::B,
                    "C" => CompatTier::C,
                    "D" => CompatTier::D,
                    _ => CompatTier::Untested,
                };
                let mut db = CompatDb::load(&db_path).unwrap_or_else(|_| CompatDb::test_plan());
                let today = format!(
                    "epoch-day {}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() / 86_400)
                        .unwrap_or(0)
                );
                if !db.set_result(name, tier, notes, &today) {
                    eprintln!("no such app slot: {name}");
                    std::process::exit(1);
                }
                db.save(&db_path).unwrap_or_else(|error| {
                    eprintln!("save failed: {error}");
                    std::process::exit(1);
                });
                println!("COMPAT_SET {name} {}", tier.label());
                return;
            }
            let db = CompatDb::load(&db_path).unwrap_or_else(|_| CompatDb::test_plan());
            let summary = db.summary();
            if action == "list" {
                for entry in &db.entries {
                    println!("{}\t{}\t{}", entry.name, entry.category, entry.tier.label());
                }
            } else {
                print!("{}", db.report());
            }
            println!(
                "COMPAT_SUMMARY total={} a={} b={} c={} d={} untested={} gate={}",
                summary.total,
                summary.tier_a,
                summary.tier_b,
                summary.tier_c,
                summary.tier_d,
                summary.untested,
                if summary.gate_passes() { "PASS" } else { "OPEN" }
            );
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
    let _ = BTreeSet::<String>::new();
    let _ = dry_run;
}
