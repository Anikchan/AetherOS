//! `aether-runner` — launch apps, manage Windows prefixes, compatibility DB.
//!
//! Usage:
//!   aether-runner list [--root DIR]
//!   aether-runner plan APP-ID [--root DIR]
//!   aether-runner launch APP-ID [--root DIR] [--dry-run] [--strict-landlock]
//!   aether-runner engines [verify|package] [--engine-root DIR] [--from DIR] [--version V] [--name wine] [--arch x86_64]
//!   aether-runner prefixes list|remove APP-ID [--root DIR]
//!   aether-runner windows install --installer FILE [--id ID] [--entry REL] [--dry-run]
//!   aether-runner windows uninstall APP-ID [--dry-run]
//!   aether-runner compat list|plan|set|report [--db FILE]

use std::collections::BTreeSet;
use std::path::PathBuf;

use aether_pkg::manifest::AppId;
use aether_pkg::{Paths, Registry};
use aether_runner::compat::{CompatDb, CompatSummary, CompatTier};
use aether_runner::compat_harness::{self, HarnessSpec};
use aether_runner::{EngineRegistry, PrefixManager, Runner, WindowsInstallRequest, WindowsInstallerFlow};

fn print_compat_summary(summary: &CompatSummary) {
    println!(
        "COMPAT_SUMMARY total={} a={} b={} c={} d={} untested={} avg_score={} gate={}",
        summary.total,
        summary.tier_a,
        summary.tier_b,
        summary.tier_c,
        summary.tier_d,
        summary.untested,
        summary.avg_score,
        if summary.gate_passes() { "PASS" } else { "OPEN" }
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("list");
    let mut root: Option<PathBuf> = None;
    let mut engine_root: Option<PathBuf> = None;
    let mut db_path = PathBuf::from("docs/2026/11/23/results/compat-db.json");
    let mut dry_run = false;
    let mut strict_landlock = false;
    let mut from_dir: Option<PathBuf> = None;
    let mut version: Option<String> = None;
    let mut engine_name = "wine".to_string();
    let mut engine = "wine-10.0".to_string();
    let mut machine = "reference-machine".to_string();
    let mut payload_dir: Option<PathBuf> = None;
    let mut arch = "x86_64".to_string();
    let mut installer: Option<PathBuf> = None;
    let mut spec_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut app_id: Option<String> = None;
    let mut entry: Option<String> = None;
    let mut installer_args: Vec<String> = Vec::new();
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
            "--strict-landlock" => strict_landlock = true,
            "--from" => {
                index += 1;
                from_dir = args.get(index).map(PathBuf::from);
            }
            "--version" => {
                index += 1;
                version = args.get(index).cloned();
            }
            "--name" => {
                index += 1;
                engine_name = args.get(index).cloned().unwrap_or(engine_name);
            }
            "--arch" => {
                index += 1;
                arch = args.get(index).cloned().unwrap_or(arch);
            }
            "--installer" => {
                index += 1;
                installer = args.get(index).map(PathBuf::from);
            }
            "--spec" => {
                index += 1;
                spec_path = args.get(index).map(PathBuf::from);
            }
            "--payload" => {
                index += 1;
                payload_dir = args.get(index).map(PathBuf::from);
            }
            "--machine" => {
                index += 1;
                machine = args.get(index).cloned().unwrap_or(machine);
            }
            "--engine" => {
                index += 1;
                engine = args.get(index).cloned().unwrap_or(engine);
            }
            "--out" => {
                index += 1;
                out_path = args.get(index).map(PathBuf::from);
            }
            "--id" => {
                index += 1;
                app_id = args.get(index).cloned();
            }
            "--entry" => {
                index += 1;
                entry = args.get(index).cloned();
            }
            "--arg" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    installer_args.push(value.clone());
                }
            }
            "-h" | "--help" => {
                println!("usage: aether-runner <list|plan|launch|engines|prefixes|windows|compat> [args]");
                println!("       launch accepts --strict-landlock (fail-closed Landlock enforcement)");
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
            let runner = runner.require_landlock(strict_landlock);
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
            let action = positional.first().map(|s| s.as_str()).unwrap_or("list");
            if action == "package" {
                let (Some(source), Some(version)) = (from_dir.clone(), version.clone()) else {
                    eprintln!("engines package needs --from DIR --version V");
                    std::process::exit(2);
                };
                match engines.package(&source, &engine_name, &version, &arch) {
                    Ok(package) => println!(
                        "ENGINE_PACKAGED {} files={} bytes={}",
                        package.dir_name(),
                        package.files.len(),
                        package.installed_bytes
                    ),
                    Err(error) => {
                        eprintln!("packaging failed: {error}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            if action == "verify" {
                let mut failed = false;
                for (version, problems) in engines.verify_all() {
                    if problems.is_empty() {
                        println!("wine-{version}\tverified");
                    } else {
                        failed = true;
                        println!("wine-{version}\tFAILED: {}", problems.join("; "));
                    }
                }
                if failed {
                    std::process::exit(1);
                }
                return;
            }
            let versions = engines.wine_versions();
            if versions.is_empty() {
                println!("No Windows engine installed (Pack B).");
            } else {
                let packaged: BTreeSet<String> = engines
                    .packages()
                    .into_iter()
                    .map(|package| package.version)
                    .collect();
                for version in versions {
                    println!(
                        "wine-{version}\t{}",
                        if packaged.contains(&version) {
                            "packaged"
                        } else {
                            "unpackaged"
                        }
                    );
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
        "windows" => {
            let action = positional.first().map(|s| s.as_str()).unwrap_or("install");
            let flow = WindowsInstallerFlow::new(paths.clone(), engines.clone());
            let flow = if dry_run { flow.dry_run() } else { flow };
            if action == "uninstall" {
                let Some(raw) = positional.get(1).or(app_id.as_ref()) else {
                    eprintln!("windows uninstall needs APP-ID");
                    std::process::exit(2);
                };
                let id = AppId::new(raw.clone()).unwrap_or_else(|error| {
                    eprintln!("invalid app id: {error}");
                    std::process::exit(2);
                });
                match flow.uninstall(&id) {
                    Ok(outcome) => println!(
                        "WINDOWS_UNINSTALLED {} prefix_removed={} engine_kept={}",
                        outcome.app,
                        !outcome.removed_prefix.exists(),
                        outcome.kept_engine
                    ),
                    Err(error) => {
                        eprintln!("uninstall failed: {error}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            let Some(installer) = installer.clone() else {
                eprintln!("windows install needs --installer FILE");
                std::process::exit(2);
            };
            let id = app_id
                .clone()
                .map(|raw| {
                    AppId::new(raw).unwrap_or_else(|error| {
                        eprintln!("invalid app id: {error}");
                        std::process::exit(2);
                    })
                })
                .unwrap_or_else(|| {
                    let stem = installer
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_else(|| "app".into())
                        .replace(' ', "-");
                    AppId::new(format!("win.{stem}")).unwrap()
                });
            let request = WindowsInstallRequest {
                id,
                installer,
                entry,
                args: installer_args.clone(),
            };
            match flow.install(&request) {
                Ok(outcome) => println!(
                    "WINDOWS_INSTALLED {} prefix={} entry={} launcher={} executed={}",
                    outcome.app,
                    outcome.prefix.display(),
                    outcome.entry,
                    outcome.launcher.display(),
                    outcome.executed
                ),
                Err(error) => {
                    eprintln!("install failed: {error}");
                    std::process::exit(1);
                }
            }
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
            if action == "slots" || action == "template" {
                let spec = HarnessSpec::template("wine-10.0", "reference-machine");
                if action == "template" {
                    let Some(path) = out_path.clone().or_else(|| spec_path.clone()) else {
                        eprintln!("compat template needs --out FILE");
                        std::process::exit(2);
                    };
                    if let Err(error) = spec.save(&path) {
                        eprintln!("cannot write {}: {error}", path.display());
                        std::process::exit(1);
                    }
                    println!(
                        "COMPAT_TEMPLATE slots={} file={}",
                        spec.slots.len(),
                        path.display()
                    );
                    return;
                }
                let db = CompatDb::test_plan();
                for entry in &db.entries {
                    println!("{}\t{}\t{}", entry.name, entry.category, entry.archetype);
                }
                println!("COMPAT_SLOTS total={}", db.entries.len());
                return;
            }
            if action == "reference-template" || action == "reference-slots" {
                let payload = payload_dir
                    .clone()
                    .unwrap_or_else(aether_runner::compat_reference::default_payload_dir);
                let spec = aether_runner::compat_reference::spec(&engine, &machine, &payload);
                if action == "reference-slots" {
                    for (slot, app) in spec.slots.iter().zip(
                        aether_runner::compat_reference::SLOT_APPS.iter(),
                    ) {
                        println!(
                            "{}\t{}\t{}\t{}\t{}",
                            slot.name,
                            app.app,
                            app.vendor,
                            app.installer,
                            app.window
                        );
                    }
                    println!("COMPAT_REFERENCE_SLOTS total={}", spec.slots.len());
                    return;
                }
                let Some(path) = out_path.clone().or_else(|| spec_path.clone()) else {
                    eprintln!("compat reference-template needs --out FILE");
                    std::process::exit(2);
                };
                if let Err(error) = spec.save(&path) {
                    eprintln!("cannot write {}: {error}", path.display());
                    std::process::exit(1);
                }
                println!(
                    "COMPAT_REFERENCE_TEMPLATE slots={} payload={} file={}",
                    spec.slots.len(),
                    payload.display(),
                    path.display()
                );
                return;
            }
            if action == "harness" {
                let Some(path) = spec_path.clone() else {
                    eprintln!("compat harness needs --spec FILE [--dry-run] [--db FILE]");
                    std::process::exit(2);
                };
                match compat_harness::run_file(&path, &db_path, dry_run) {
                    Ok((harness, summary)) => {
                        println!(
                            "COMPAT_HARNESS executed={} imported={} dry_run={} skipped={} unknown={}",
                            harness.executed,
                            harness.imported,
                            harness.dry_run,
                            harness.skipped,
                            harness.unknown_slots
                        );
                        print_compat_summary(&summary);
                        if harness.unknown_slots > 0 {
                            std::process::exit(1);
                        }
                    }
                    Err(error) => {
                        eprintln!("harness failed: {error}");
                        std::process::exit(1);
                    }
                }
                return;
            }
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
                let Some(tier) = CompatTier::parse(tier) else {
                    eprintln!("tier must be A, B, C, D or untested");
                    std::process::exit(2);
                };
                let mut db = CompatDb::load_or_test_plan(&db_path);
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
            let db = CompatDb::load_or_test_plan(&db_path);
            let summary = db.summary();
            if action == "list" {
                for entry in &db.entries {
                    println!(
                        "{}\t{}\t{}\t{}",
                        entry.name,
                        entry.category,
                        entry.tier.label(),
                        entry.score.map(|s| s.to_string()).unwrap_or_else(|| "—".into())
                    );
                }
            } else {
                print!("{}", db.report());
            }
            print_compat_summary(&summary);
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
    let _ = BTreeSet::<String>::new();
    let _ = dry_run;
}
