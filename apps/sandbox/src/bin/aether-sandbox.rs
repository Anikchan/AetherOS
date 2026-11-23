//! `aether-sandbox` — permission and sandbox helper (P6-T04).
//!
//! Usage:
//!   aether-sandbox doctor
//!   aether-sandbox grant APP-ID PERMISSION
//!   aether-sandbox revoke APP-ID PERMISSION
//!   aether-sandbox list APP-ID
//!   aether-sandbox profile --manifest FILE --data DIR [--granted a,b] [--network]

use std::collections::BTreeSet;
use std::path::PathBuf;

use aether_pkg::manifest::AppId;
use aether_pkg::read_manifest;
use aether_sandbox::{describe, Broker, Enforcer, Sandbox};

fn default_store() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/var/lib"))
        .join("aether/permissions.json")
}

fn permission_from_key(key: &str) -> Option<aether_pkg::Permission> {
    use aether_pkg::{FileScope, Permission};
    match key {
        "network" => Some(Permission::Network),
        "camera" => Some(Permission::Camera),
        "microphone" => Some(Permission::Microphone),
        "location" => Some(Permission::Location),
        "notifications" => Some(Permission::Notifications),
        "background" => Some(Permission::Background),
        "files:pickedbyuser" => Some(Permission::Files {
            scope: FileScope::PickedByUser,
        }),
        "files:appfolder" => Some(Permission::Files {
            scope: FileScope::AppFolder,
        }),
        "files:home" => Some(Permission::Files {
            scope: FileScope::Home,
        }),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("doctor");
    let mut store_path = default_store();
    let mut manifest_path: Option<PathBuf> = None;
    let mut data_dir: Option<PathBuf> = None;
    let mut granted: BTreeSet<String> = BTreeSet::new();
    let mut positional = Vec::new();

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--store" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    store_path = PathBuf::from(value);
                }
            }
            "--manifest" => {
                index += 1;
                manifest_path = args.get(index).map(PathBuf::from);
            }
            "--data" => {
                index += 1;
                data_dir = args.get(index).map(PathBuf::from);
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
            "-h" | "--help" => {
                println!("usage: aether-sandbox <doctor|grant|revoke|list|profile> [args]");
                return;
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }

    match command {
        "doctor" => {
            let caps = aether_sandbox::SandboxCapabilities::detect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "bwrap": caps.bwrap,
                    "user_namespaces": caps.user_namespaces,
                    "landlock": caps.landlock,
                    "usable": caps.usable(),
                    "note": if caps.usable() { "sandbox enforcement available" } else { "apps will refuse to launch unsandboxed (D8 failure mode)" },
                }))
                .unwrap_or_default()
            );
            match Enforcer::system() {
                Ok(_) => println!("ENFORCER_OK"),
                Err(error) => println!("ENFORCER_UNAVAILABLE {error}"),
            }
        }
        "grant" | "revoke" => {
            let (Some(app), Some(permission_key)) = (positional.first(), positional.get(1)) else {
                eprintln!("usage: aether-sandbox {command} APP-ID PERMISSION");
                std::process::exit(2);
            };
            let id = AppId::new(app.clone()).unwrap_or_else(|error| {
                eprintln!("invalid app id: {error}");
                std::process::exit(2);
            });
            let Some(permission) = permission_from_key(permission_key) else {
                eprintln!("unknown permission: {permission_key}");
                std::process::exit(2);
            };
            let mut broker = Broker::load(&store_path);
            if command == "grant" {
                broker.grant(&id, &permission).unwrap_or_else(|error| {
                    eprintln!("grant failed: {error}");
                    std::process::exit(1);
                });
                println!("GRANTED {} ({})", permission.key(), describe(&permission));
            } else {
                broker.revoke(&id, &permission).unwrap_or_else(|error| {
                    eprintln!("revoke failed: {error}");
                    std::process::exit(1);
                });
                println!("REVOKED {}", permission.key());
            }
        }
        "list" => {
            let Some(app) = positional.first() else {
                eprintln!("usage: aether-sandbox list APP-ID");
                std::process::exit(2);
            };
            let id = AppId::new(app.clone()).unwrap_or_else(|error| {
                eprintln!("invalid app id: {error}");
                std::process::exit(2);
            });
            let broker = Broker::load(&store_path);
            let granted = broker.store.granted(&id);
            let denied = broker.store.denied.get(id.as_str()).cloned().unwrap_or_default();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "app": id,
                    "granted": granted,
                    "denied": denied,
                }))
                .unwrap_or_default()
            );
        }
        "profile" => {
            let Some(manifest_path) = manifest_path else {
                eprintln!("profile needs --manifest FILE");
                std::process::exit(2);
            };
            let manifest = read_manifest(&manifest_path).unwrap_or_else(|error| {
                eprintln!("manifest error: {error}");
                std::process::exit(1);
            });
            let data = data_dir.unwrap_or_else(|| PathBuf::from("/tmp/aether-app-data"));
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/home/user"));
            let sandbox = Sandbox::new(granted, &data, home);
            let profile = sandbox.profile_for(&manifest);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "app": manifest.id,
                    "file_read": profile.file_read,
                    "file_write": profile.file_write,
                    "network": profile.network,
                    "devices": profile.devices,
                }))
                .unwrap_or_default()
            );
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}
