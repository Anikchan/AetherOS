//! `aether-installer` — detection, dry-run plan, uninstall plan (P8).
//!
//! Usage:
//!   aether-installer detect [--json]
//!   aether-installer hardware [--json]
//!   aether-installer plan --disk DEV [--mode alongside|erase] [--root-gib N]
//!                         [--confirm DEV] [--json] [--backup-confirmed]
//!                         [--fast-startup] [--bitlocker] [--recovery-key-saved]
//!                         [--on-battery PERCENT] [--ram-gib N] [--force]
//!   aether-installer uninstall-plan --disk DEV [--grow-windows] [--json]
//!   aether-installer apply --plan FILE     (refused outside the live image)

use std::path::PathBuf;

use aether_installer::precheck::{can_proceed, run as run_prechecks, CheckStatus, PreCheckInput};
use aether_installer::{
    detect_system, human_size, plan_install, plan_uninstall, InstallMode, UninstallMode,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(|s| s.as_str()).unwrap_or("detect");
    let mut disk_device: Option<String> = None;
    let mut mode = "alongside".to_string();
    let mut root_gib = 64u64;
    let mut confirm: Option<String> = None;
    let mut json = false;
    let mut backup_confirmed = false;
    let mut fast_startup = false;
    let mut bitlocker = false;
    let mut recovery_key_saved = false;
    let mut on_battery: Option<u8> = None;
    let mut ram_gib = 8u32;
    let mut force = false;
    let mut grow_windows = false;
    let mut plan_file: Option<PathBuf> = None;

    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--disk" => {
                index += 1;
                disk_device = args.get(index).cloned();
            }
            "--mode" => {
                index += 1;
                mode = args.get(index).cloned().unwrap_or_else(|| "alongside".into());
            }
            "--root-gib" => {
                index += 1;
                root_gib = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(64);
            }
            "--confirm" => {
                index += 1;
                confirm = args.get(index).cloned();
            }
            "--json" => json = true,
            "--backup-confirmed" => backup_confirmed = true,
            "--fast-startup" => fast_startup = true,
            "--bitlocker" => bitlocker = true,
            "--recovery-key-saved" => recovery_key_saved = true,
            "--on-battery" => {
                index += 1;
                on_battery = args.get(index).and_then(|v| v.parse().ok());
            }
            "--ram-gib" => {
                index += 1;
                ram_gib = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(8);
            }
            "--force" => force = true,
            "--grow-windows" => grow_windows = true,
            "--plan" => {
                index += 1;
                plan_file = args.get(index).map(PathBuf::from);
            }
            "-h" | "--help" => {
                println!("usage: aether-installer <detect|hardware|plan|uninstall-plan|apply> [args]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        index += 1;
    }

    let report = detect_system();

    match command {
        "detect" | "hardware" => {
            if json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
            } else {
                println!("Firmware: {}", report.firmware);
                println!("CPU:      {}", report.cpu);
                println!("Memory:   {}", human_size(report.memory_bytes));
                println!(
                    "GPU:      {}",
                    if report.gpu.is_empty() {
                        "not reported".into()
                    } else {
                        report.gpu.join(", ")
                    }
                );
                println!("Wi-Fi:    {}", report.wifi.clone().unwrap_or_else(|| "not reported".into()));
                println!("Audio:    {}", report.audio.clone().unwrap_or_else(|| "not reported".into()));
                println!("Windows:  present={} version={:?} bitlocker={} fast_startup={}",
                    report.windows.present, report.windows.version, report.windows.bitlocker,
                    report.windows.fast_startup);
                println!("Disks:");
                for disk in &report.disks {
                    println!("  {} free={} removable={}", disk.label(), human_size(disk.free_bytes), disk.removable);
                    for partition in &disk.partitions {
                        println!(
                            "    p{:<2} {:<18} {:>10} {:<6} {:?}",
                            partition.number,
                            partition.name,
                            human_size(partition.size_bytes),
                            partition.fs.clone().unwrap_or_else(|| "-".into()),
                            partition.os
                        );
                    }
                }
                if report.disks.is_empty() {
                    println!("  (no disks reported; lsblk unavailable on this host)");
                }
            }
        }
        "plan" => {
            let Some(device) = &disk_device else {
                eprintln!("plan needs --disk DEV");
                std::process::exit(2);
            };
            let Some(disk) = report.disks.iter().find(|disk| &disk.device == device) else {
                eprintln!("disk {device} was not detected; run `aether-installer detect`");
                std::process::exit(1);
            };
            let mut windows = report.windows.clone();
            windows.fast_startup |= fast_startup;
            windows.bitlocker |= bitlocker;
            windows.recovery_key_saved |= recovery_key_saved;
            let mode = match mode.as_str() {
                "erase" => InstallMode::Erase,
                _ => InstallMode::Alongside,
            };
            let checks = run_prechecks(&PreCheckInput {
                windows: windows.clone(),
                mode,
                backup_confirmed,
                ac_power: on_battery.is_none(),
                battery_percent: on_battery,
                free_bytes: disk.free_bytes,
                ram_gib,
            });
            for check in &checks {
                println!(
                    "[{:?}] {} — {}",
                    check.status, check.title, check.detail
                );
            }
            if !can_proceed(&checks) && !force {
                eprintln!("PRECHECK_BLOCKED; resolve blocking checks or pass --force for the dry run only");
                std::process::exit(1);
            }
            match plan_install(disk, &windows, mode, root_gib, ram_gib, confirm.as_deref()) {
                Ok(plan) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&plan).unwrap_or_default());
                    } else {
                        print!("{}", plan.render_text());
                    }
                    println!("PLAN_OK dry_run={} operations={}", plan.dry_run, plan.operations.len());
                }
                Err(error) => {
                    eprintln!("PLAN_FAILED {error}");
                    std::process::exit(1);
                }
            }
        }
        "uninstall-plan" => {
            let Some(device) = &disk_device else {
                eprintln!("uninstall-plan needs --disk DEV");
                std::process::exit(2);
            };
            let Some(disk) = report.disks.iter().find(|disk| &disk.device == device) else {
                eprintln!("disk {device} was not detected");
                std::process::exit(1);
            };
            let mode = if grow_windows {
                UninstallMode::GrowWindows
            } else {
                UninstallMode::KeepUnallocated
            };
            match plan_uninstall(disk, mode) {
                Ok(plan) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&plan).unwrap_or_default());
                    } else {
                        print!("{}", plan.render_text());
                    }
                    println!("UNINSTALL_PLAN_OK dry_run={}", plan.dry_run);
                }
                Err(error) => {
                    eprintln!("UNINSTALL_PLAN_FAILED {error}");
                    std::process::exit(1);
                }
            }
        }
        "apply" => {
            let _ = plan_file;
            eprintln!(
                "APPLY_REFUSED: disk writes run only from the AetherOS live image. \
                 Boot the USB and review the dry run there (INS-12)."
            );
            std::process::exit(5);
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
    let _ = CheckStatus::Pass;
}
