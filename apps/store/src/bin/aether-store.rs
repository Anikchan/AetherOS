//! `aether-store` — Software app core CLI (P6-T09).
//!
//! Usage:
//!   aether-store --repo DIR list|search QUERY
//!   aether-store --repo DIR install APP-ID [--granted a,b] [--root DIR]
//!   aether-store --repo DIR updates|update-all [--granted a,b]
//!   aether-store --repo DIR remove APP-ID --yes [--root DIR]
//!   aether-store --repo DIR --json '{"action":"list"}' [--compat-db FILE] [--root DIR]
//!
//! `--json` is the contract the Software app UI binds to (D8 §9): one request
//! in, one JSON response out.

use std::collections::BTreeSet;
use std::path::PathBuf;

use aether_pkg::manifest::AppId;
use aether_pkg::{InstallOutcome, Paths};
use aether_store::{CompatIndex, Store, StoreIndex, StoreResponse};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut repo: Option<PathBuf> = None;
    let mut root: Option<PathBuf> = None;
    let mut compat_db: Option<PathBuf> = None;
    let mut json_request: Option<String> = None;
    let mut granted: BTreeSet<String> = BTreeSet::new();
    let mut yes = false;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--repo" => {
                index += 1;
                repo = args.get(index).map(PathBuf::from);
            }
            "--root" => {
                index += 1;
                root = args.get(index).map(PathBuf::from);
            }
            "--compat-db" => {
                index += 1;
                compat_db = args.get(index).map(PathBuf::from);
            }
            "--json" => {
                index += 1;
                json_request = args.get(index).cloned();
            }
            "--granted" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    granted = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            "--yes" => yes = true,
            "-h" | "--help" => {
                println!("usage: aether-store [--repo DIR] <list|search|install|updates|update-all|remove|--json REQUEST> [args]");
                return;
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }

    let repo = repo.unwrap_or_else(|| {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("/var/lib"))
            .join("aether/repository")
    });
    let store_dir = root.clone().map(Paths::at).unwrap_or_else(Paths::for_user);
    let index = StoreIndex::scan(&repo).unwrap_or_else(|error| {
        eprintln!("cannot read repository {}: {error}", repo.display());
        std::process::exit(1);
    });
    let store = Store::new(store_dir, index);

    if let Some(request) = json_request {
        let compat = match compat_db.as_deref() {
            Some(path) => CompatIndex::load(path).unwrap_or_else(|error| {
                eprintln!("cannot read compat db {}: {error}", path.display());
                std::process::exit(1);
            }),
            None => CompatIndex::default(),
        };
        let answer = aether_store::handle_json(&store, &request, &compat);
        println!("{answer}");
        match serde_json::from_str::<StoreResponse>(&answer) {
            Ok(StoreResponse::NeedsPermission { .. }) => std::process::exit(3),
            Ok(StoreResponse::Failed { message }) => {
                eprintln!("store request failed: {message}");
                std::process::exit(1);
            }
            _ => {}
        }
        return;
    }

    let command = positional.first().map(|s| s.as_str()).unwrap_or("list");
    match command {
        "list" => {
            for entry in store.index.search("") {
                println!(
                    "{}\t{}\t{}\t{}",
                    entry.manifest.id,
                    entry.manifest.name,
                    entry.manifest.version,
                    entry.manifest.kind.as_str()
                );
            }
        }
        "search" => {
            let query = positional.get(1).cloned().unwrap_or_default();
            for entry in store.index.search(&query) {
                println!(
                    "{}\t{}\t{}\t{}",
                    entry.manifest.id,
                    entry.manifest.name,
                    entry.manifest.version,
                    entry
                        .manifest
                        .description
                        .clone()
                        .unwrap_or_default()
                );
            }
        }
        "install" => {
            let Some(id) = positional.get(1) else {
                eprintln!("install needs APP-ID");
                std::process::exit(2);
            };
            match store.install(id, granted.iter().cloned()) {
                Ok(InstallOutcome::Installed(id)) => println!("INSTALLED {id}"),
                Ok(InstallOutcome::NeedsPermission(permissions)) => {
                    for permission in permissions {
                        println!("NEEDS_PERMISSION {}", permission.label());
                    }
                    std::process::exit(3);
                }
                Ok(InstallOutcome::Unsupported { reason, .. }) => {
                    println!("UNSUPPORTED {reason}");
                    std::process::exit(4);
                }
                Err(error) => {
                    eprintln!("install failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        "updates" => {
            let updates = store.updates();
            if updates.is_empty() {
                println!("No updates.");
            }
            for (id, from, to) in updates {
                println!("{id}\t{from} -> {to}");
            }
        }
        "update-all" => {
            for (id, result) in store.update_all(granted.iter().cloned()) {
                match result {
                    Ok(InstallOutcome::Installed(_)) => println!("UPDATED {id}"),
                    Ok(other) => println!("SKIPPED {id}: {other:?}"),
                    Err(error) => println!("FAILED {id}: {error}"),
                }
            }
        }
        "remove" => {
            let Some(id) = positional.get(1) else {
                eprintln!("remove needs APP-ID");
                std::process::exit(2);
            };
            if !yes {
                eprintln!("Refusing to remove {id} without --yes");
                std::process::exit(2);
            }
            let id = AppId::new(id.clone()).unwrap_or_else(|error| {
                eprintln!("invalid app id: {error}");
                std::process::exit(2);
            });
            match store.remove(&id, granted.iter().cloned()) {
                Ok(()) => println!("REMOVED {id}"),
                Err(error) => {
                    eprintln!("remove failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}
