//! `aether-update` — A/B slot status, staging, boot and rollback (P9-T01/T02).
//!
//! Usage:
//!   aether-update status [--state F]
//!   aether-update install --manifest FILE --image IMG [--signature-valid] [--state F]
//!   aether-update boot|activate|health-pass|health-fail|rollback [--state F]

use std::path::PathBuf;

use aether_update::{
    activate_pending, health_check, install, on_boot, rollback, sha256_file, UpdateManifest,
    UpdateState,
};

fn default_state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/var/lib"))
        .join("aether/update/state.json")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("status");
    let mut state_path = default_state();
    let mut manifest_path: Option<PathBuf> = None;
    let mut image_path: Option<PathBuf> = None;
    let mut signature_valid = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--state" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    state_path = PathBuf::from(value);
                }
            }
            "--manifest" => {
                index += 1;
                manifest_path = args.get(index).map(PathBuf::from);
            }
            "--image" => {
                index += 1;
                image_path = args.get(index).map(PathBuf::from);
            }
            "--signature-valid" => signature_valid = true,
            "-h" | "--help" => {
                println!("usage: aether-update <status|install|boot|activate|health-pass|health-fail|rollback> [args]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        index += 1;
    }

    let mut state = UpdateState::load(&state_path).unwrap_or_default();
    let result = match command {
        "status" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&state).unwrap_or_default()
            );
            Ok(())
        }
        "install" => {
            let (Some(manifest_path), Some(image_path)) = (manifest_path, image_path) else {
                eprintln!("install needs --manifest FILE --image IMG");
                std::process::exit(2);
            };
            let manifest: UpdateManifest = std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|text| toml::from_str(&text).ok())
                .unwrap_or_else(|| {
                    eprintln!("invalid manifest {}", manifest_path.display());
                    std::process::exit(1);
                });
            let digest = sha256_file(&image_path).unwrap_or_else(|error| {
                eprintln!("cannot hash image: {error}");
                std::process::exit(1);
            });
            install(&mut state, &manifest, &digest, signature_valid).map(|_| {
                println!("STAGED {} checksum={digest}", manifest.version)
            })
        }
        "activate" => activate_pending(&mut state).map(|_| println!("ACTIVATED {:?}", state.active)),
        "boot" => on_boot(&mut state).map(|rolled_back| {
            if rolled_back {
                println!("ROLLED_BACK active={:?}", state.active)
            } else {
                println!("BOOT_OK attempts={}", state.boot_attempts)
            }
        }),
        "health-pass" => health_check(&mut state, true)
            .map(|_| println!("HEALTH_OK active={:?}", state.active)),
        "health-fail" => health_check(&mut state, false)
            .map(|rolled_back| println!("HEALTH_FAILED rolled_back={rolled_back} active={:?}", state.active)),
        "rollback" => rollback(&mut state).map(|_| println!("ROLLED_BACK active={:?}", state.active)),
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("update failed: {error}");
        std::process::exit(1);
    }
    if command != "status" {
        if let Err(error) = state.save(&state_path) {
            eprintln!("state save failed: {error}");
            std::process::exit(1);
        }
    }
}
