//! `aether-update` — A/B slot status, staging, boot, rollback and RAUC wiring
//! (P9-T01/T02).
//!
//! Usage:
//!   aether-update status [--state F]
//!   aether-update install --manifest FILE --image IMG [--signature-valid] [--state F]
//!   aether-update boot|activate|health-pass|health-fail|rollback [--state F]
//!   aether-update health-check [--report FILE] [--boot-at EPOCH] [--policy-file F] [--state F]
//!   aether-update probes [--save-report FILE]
//!   aether-update rauc-status [--rauc-json FILE]
//!   aether-update rauc-sync [--rauc-json FILE] [--state F]
//!   aether-update rauc-install --image BUNDLE [--dry-run] [--state F]
//!   aether-update rauc-mark-good|rauc-mark-bad [--dry-run] [--state F]
//!   aether-update rauc-config
//!   aether-update bundle-manifest --version V --image IMG [--out FILE]

use std::path::PathBuf;

use aether_update::rauc::{
    bootname, bundle_manifest, parse_status, sync_state, validate_system_conf, RaucCommand,
    RaucStatus, SYSTEM_CONF,
};
use aether_update::{
    activate_pending, default_probes, health_check, install, on_boot, rollback, run_probes,
    sha256_file, stage_pending, HealthAction, HealthPolicy, HealthReport, HealthService,
    UpdateManifest, UpdateState,
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
    let mut rauc_json: Option<PathBuf> = None;
    let mut report_path: Option<PathBuf> = None;
    let mut save_report: Option<PathBuf> = None;
    let mut policy_file: Option<PathBuf> = None;
    let mut boot_at: Option<u64> = None;
    let mut version = String::from("0.0.0");
    let mut description = String::from("AetherOS update");
    let mut out_path: Option<PathBuf> = None;
    let mut signature_valid = false;
    let mut dry_run = false;
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
            "--rauc-json" => {
                index += 1;
                rauc_json = args.get(index).map(PathBuf::from);
            }
            "--report" => {
                index += 1;
                report_path = args.get(index).map(PathBuf::from);
            }
            "--save-report" => {
                index += 1;
                save_report = args.get(index).map(PathBuf::from);
            }
            "--policy-file" => {
                index += 1;
                policy_file = args.get(index).map(PathBuf::from);
            }
            "--boot-at" => {
                index += 1;
                boot_at = args.get(index).and_then(|value| value.parse().ok());
            }
            "--version" => {
                index += 1;
                version = args.get(index).cloned().unwrap_or(version);
            }
            "--description" => {
                index += 1;
                description = args.get(index).cloned().unwrap_or(description);
            }
            "--out" => {
                index += 1;
                out_path = args.get(index).map(PathBuf::from);
            }
            "--signature-valid" => signature_valid = true,
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                println!("usage: aether-update <status|install|boot|activate|health-pass|health-fail|health-check|probes|rollback|rauc-status|rauc-sync|rauc-install|rauc-mark-good|rauc-mark-bad|rauc-config|bundle-manifest> [args]");
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
        "probes" => {
            let report = run_probes(&default_probes());
            for probe in &report.probes {
                println!(
                    "PROBE {}\t{}\t{}{}",
                    probe.id,
                    if probe.passed { "pass" } else { "fail" },
                    if probe.required { "required" } else { "optional" },
                    if probe.detail.is_empty() {
                        String::new()
                    } else {
                        format!("\t{}", probe.detail)
                    }
                );
            }
            if let Some(path) = save_report.clone() {
                report.save(&path).unwrap_or_else(|error| {
                    eprintln!("cannot write {}: {error}", path.display());
                    std::process::exit(1);
                });
                println!("HEALTH_REPORT {}", path.display());
            }
            println!("HEALTH_REPORT_PASSED passed={}", report.passed());
            Ok(())
        }
        "health-check" => {
            let policy = match policy_file.as_deref() {
                Some(path) => {
                    let text = std::fs::read_to_string(path).unwrap_or_else(|error| {
                        eprintln!("cannot read policy {}: {error}", path.display());
                        std::process::exit(1);
                    });
                    serde_json::from_str::<HealthPolicy>(&text).unwrap_or_else(|error| {
                        eprintln!("invalid policy {}: {error}", path.display());
                        std::process::exit(1);
                    })
                }
                None => HealthPolicy::default(),
            };
            let mut service = HealthService::new(state.clone()).with_policy(policy);
            let now = now_secs();
            let action = match report_path.as_deref() {
                Some(path) => {
                    let report = HealthReport::load(path).unwrap_or_else(|error| {
                        eprintln!("cannot read report {}: {error}", path.display());
                        std::process::exit(1);
                    });
                    service.evaluate(&report, now)
                }
                None => {
                    // The service runs the probes itself on a real boot.
                    let report = run_probes(&default_probes());
                    for probe in &report.probes {
                        println!(
                            "PROBE {}\t{}{}",
                            probe.id,
                            if probe.passed { "pass" } else { "fail" },
                            if probe.detail.is_empty() {
                                String::new()
                            } else {
                                format!("\t{}", probe.detail)
                            }
                        );
                    }
                    if let Some(boot_at) = boot_at {
                        service.enforce_deadline(boot_at, now)
                    } else {
                        service.evaluate(&report, now)
                    }
                }
            };
            action.map(|action| {
                state = service.state;
                println!("HEALTH {}", describe_action(&action));
            })
        }
        "rollback" => rollback(&mut state).map(|_| println!("ROLLED_BACK active={:?}", state.active)),
        "rauc-config" => {
            let problems = validate_system_conf();
            print!("{SYSTEM_CONF}");
            if problems.is_empty() {
                println!("RAUC_CONFIG_OK");
                Ok(())
            } else {
                eprintln!("RAUC_CONFIG_INVALID missing={}", problems.join(","));
                std::process::exit(1);
            }
        }
        "bundle-manifest" => {
            let Some(image) = image_path else {
                eprintln!("bundle-manifest needs --image IMG");
                std::process::exit(2);
            };
            let digest = sha256_file(&image).unwrap_or_else(|error| {
                eprintln!("cannot hash image: {error}");
                std::process::exit(1);
            });
            let size = std::fs::metadata(&image).map(|meta| meta.len()).unwrap_or(0);
            let filename = image
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "rootfs.squashfs".into());
            let manifest = bundle_manifest(&version, &filename, &digest, size, &description);
            if let Some(path) = out_path {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&path, &manifest).unwrap_or_else(|error| {
                    eprintln!("cannot write {}: {error}", path.display());
                    std::process::exit(1);
                });
                println!("BUNDLE_MANIFEST {}", path.display());
            } else {
                print!("{manifest}");
            }
            Ok(())
        }
        "rauc-status" | "rauc-sync" => {
            let status = match read_status(rauc_json.as_deref()) {
                Ok(status) => status,
                Err(error) => {
                    eprintln!("rauc status failed: {error}");
                    std::process::exit(1);
                }
            };
            if command == "rauc-status" {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).unwrap_or_default()
                );
                Ok(())
            } else {
                let changed = sync_state(&mut state, &status);
                println!(
                    "RAUC_SYNC booted={:?} active={:?} changed={changed}",
                    status.booted, state.active
                );
                Ok(())
            }
        }
        "rauc-install" => {
            let Some(bundle) = image_path else {
                eprintln!("rauc-install needs --image BUNDLE");
                std::process::exit(2);
            };
            let command = RaucCommand::Install {
                bundle: bundle.display().to_string(),
            };
            match run_rauc(&command, dry_run) {
                Ok(()) => {
                    let target = state.active.other();
                    if let Err(error) = stage_pending(&mut state, target) {
                        eprintln!("cannot stage {target:?}: {error}");
                        std::process::exit(1);
                    }
                    println!("RAUC_INSTALLED pending={target:?}");
                    Ok(())
                }
                Err(error) => {
                    eprintln!("rauc install failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        "rauc-mark-good" | "rauc-mark-bad" => {
            let good = command == "rauc-mark-good";
            let slot = state.active;
            let command = if good {
                RaucCommand::MarkGood {
                    bootname: bootname(slot).into(),
                }
            } else {
                RaucCommand::MarkBad {
                    bootname: bootname(slot).into(),
                }
            };
            match run_rauc(&command, dry_run) {
                Ok(()) => health_check(&mut state, good).map(|rolled_back| {
                    if good {
                        println!("HEALTH_OK active={:?}", state.active);
                    } else {
                        println!("HEALTH_FAILED rolled_back={rolled_back} active={:?}", state.active);
                    }
                }),
                Err(error) => {
                    eprintln!("rauc mark failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("update failed: {error}");
        std::process::exit(1);
    }
    if !matches!(
        command,
        "status" | "probes" | "rauc-status" | "rauc-config" | "bundle-manifest"
    ) {
        if let Err(error) = state.save(&state_path) {
            eprintln!("state save failed: {error}");
            std::process::exit(1);
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn describe_action(action: &HealthAction) -> String {
    match action {
        HealthAction::NotPending => "not-pending".into(),
        HealthAction::Confirmed { version } => format!(
            "confirmed version={}",
            version.clone().unwrap_or_else(|| "unknown".into())
        ),
        HealthAction::RolledBack { to, reason } => {
            format!("rolled-back to={to:?} because={reason:?}")
        }
        HealthAction::Waiting {
            attempts,
            next_retry_in_secs,
        } => format!("waiting attempts={attempts} retry_in={next_retry_in_secs}s"),
    }
}

fn read_status(path: Option<&std::path::Path>) -> Result<RaucStatus, String> {
    let text = match path {
        Some(path) => std::fs::read_to_string(path).map_err(|error| error.to_string())?,
        None => {
            let output = std::process::Command::new("rauc")
                .args(["status", "--output-format=json"])
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
    };
    parse_status(&text).map_err(|error| error.to_string())
}

fn run_rauc(command: &RaucCommand, dry_run: bool) -> Result<(), String> {
    let argv = command.argv();
    println!("RAUC_CMD {}", argv.join(" "));
    if dry_run {
        return Ok(());
    }
    let status = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(status.to_string())
    }
}
