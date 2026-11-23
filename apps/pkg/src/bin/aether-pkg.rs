//! `aether-pkg` — inspect, verify, install and remove app bundles (P6-T01/T02).
//!
//! Usage:
//!   aether-pkg inspect FILE [--json]
//!   aether-pkg verify FILE
//!   aether-pkg install FILE [--yes] [--accept-permissions] [--root DIR] [--dry-run]
//!   aether-pkg uninstall APP-ID [--yes] [--root DIR]
//!   aether-pkg list [--json] [--root DIR]

use std::collections::BTreeSet;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use aether_pkg::manifest::AppId;
use aether_pkg::{
    compute_files, read_manifest, verify_files, InstallOutcome, Installer, PackageInstaller, Paths,
};

#[derive(Default)]
struct Args {
    command: String,
    file: Option<PathBuf>,
    root: Option<PathBuf>,
    json: bool,
    yes: bool,
    accept_permissions: bool,
    dry_run: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    args.command = it.next().unwrap_or_else(|| {
        eprintln!("usage: aether-pkg <inspect|verify|install|uninstall|list> [args]");
        std::process::exit(2);
    });
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--root" => args.root = it.next().map(PathBuf::from),
            "--json" => args.json = true,
            "--yes" => args.yes = true,
            "--accept-permissions" => args.accept_permissions = true,
            "--dry-run" => args.dry_run = true,
            "-h" | "--help" => {
                println!("usage: aether-pkg <inspect|verify|install|uninstall|list> [FILE|APP-ID] [--root DIR] [--json] [--yes] [--accept-permissions] [--dry-run]");
                std::process::exit(0);
            }
            value => {
                if args.file.is_none() {
                    args.file = Some(PathBuf::from(value));
                } else {
                    eprintln!("unexpected argument: {value}");
                    std::process::exit(2);
                }
            }
        }
    }
    args
}

fn main() {
    let args = parse_args();
    let paths = args
        .root
        .clone()
        .map(Paths::at)
        .unwrap_or_else(Paths::for_user);
    let mut installer = Installer::new(paths.clone(), BTreeSet::new());
    if args.dry_run {
        installer = installer.dry_run();
    }

    match args.command.as_str() {
        "inspect" => {
            let Some(file) = &args.file else {
                eprintln!("inspect needs a file");
                std::process::exit(2);
            };
            match installer.inspect(file) {
                Ok(manifest) => {
                    if args.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&manifest).unwrap_or_default()
                        );
                    } else {
                        print_manifest(&manifest);
                    }
                }
                Err(error) => fail(&error.to_string()),
            }
        }
        "verify" => {
            let Some(file) = &args.file else {
                eprintln!("verify needs a bundle directory");
                std::process::exit(2);
            };
            let manifest = match read_manifest(file) {
                Ok(manifest) => manifest,
                Err(error) => fail(&error.to_string()),
            };
            match verify_files(file, &manifest) {
                Ok(total) => {
                    println!(
                        "VERIFY_OK id={} version={} files={} bytes={total}",
                        manifest.id,
                        manifest.version,
                        manifest.files.len()
                    );
                }
                Err(error) => fail(&error.to_string()),
            }
        }
        "install" => {
            let Some(file) = &args.file else {
                eprintln!("install needs a file or bundle directory");
                std::process::exit(2);
            };
            let manifest = match installer.inspect(file) {
                Ok(manifest) => manifest,
                Err(error) => fail(&error.to_string()),
            };
            println!(
                "Inspecting {} {} ({}) — {} permission(s)",
                manifest.name,
                manifest.version,
                manifest.kind.as_str(),
                manifest.permissions.len()
            );
            for permission in &manifest.permissions {
                println!("  - {}", permission.label());
            }
            if args.accept_permissions {
                for permission in &manifest.permissions {
                    installer.grant(permission);
                }
            } else if !args.yes && std::io::stdin().is_terminal() {
                println!("Accept these permissions? [y/N]");
                let mut answer = String::new();
                let _ = std::io::stdin().lock().read_line(&mut answer);
                if answer.trim().eq_ignore_ascii_case("y") {
                    for permission in &manifest.permissions {
                        installer.grant(permission);
                    }
                }
            }
            match installer.install(&manifest, file) {
                Ok(InstallOutcome::Installed(id)) => {
                    println!("INSTALLED {id} (data folder: {})", paths.data_dir(&id).display());
                    let _ = write_desktop_entry(&paths, &manifest);
                }
                Ok(InstallOutcome::NeedsPermission(permissions)) => {
                    for permission in permissions {
                        println!("NEEDS_PERMISSION {}", permission.label());
                    }
                    eprintln!(
                        "Install stopped: rerun with --accept-permissions or accept the prompt."
                    );
                    std::process::exit(3);
                }
                Ok(InstallOutcome::Unsupported { reason, alternatives }) => {
                    println!("UNSUPPORTED {reason}");
                    for alternative in alternatives {
                        println!("  alternative: {alternative}");
                    }
                    std::process::exit(4);
                }
                Err(error) => fail(&error.to_string()),
            }
        }
        "uninstall" => {
            let Some(raw) = &args.file else {
                eprintln!("uninstall needs an app id");
                std::process::exit(2);
            };
            let id = match AppId::new(raw.to_string_lossy().to_string()) {
                Ok(id) => id,
                Err(error) => fail(&error.to_string()),
            };
            if !args.yes && !args.dry_run {
                eprintln!("Refusing to uninstall {} without --yes", id);
                std::process::exit(2);
            }
            match installer.uninstall(&id) {
                Ok(()) => {
                    let desktop = paths.applications.join(format!("{id}.desktop"));
                    let _ = std::fs::remove_file(desktop);
                    println!("UNINSTALLED {id}");
                }
                Err(error) => fail(&error.to_string()),
            }
        }
        "list" => {
            let registry = installer.registry();
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&registry).unwrap_or_default()
                );
            } else {
                for app in registry.list() {
                    println!(
                        "{}\t{}\t{}\t{}",
                        app.manifest.id,
                        app.manifest.version,
                        app.manifest.kind.as_str(),
                        app.target.display()
                    );
                }
                if registry.apps.is_empty() {
                    println!("No apps installed.");
                }
            }
        }
        "manifest" => {
            let Some(file) = &args.file else {
                eprintln!("manifest needs a bundle directory");
                std::process::exit(2);
            };
            let manifest = match read_manifest(file) {
                Ok(manifest) => manifest,
                Err(error) => fail(&error.to_string()),
            };
            println!(
                "{}",
                toml::to_string_pretty(&manifest).unwrap_or_default()
            );
        }
        "hash" => {
            let Some(file) = &args.file else {
                eprintln!("hash needs a bundle directory");
                std::process::exit(2);
            };
            match compute_files(file) {
                Ok(files) => {
                    println!(
                        "{}",
                        toml::to_string_pretty(&files).unwrap_or_default()
                    );
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

fn print_manifest(manifest: &aether_pkg::PackageManifest) {
    println!("id:          {}", manifest.id);
    println!("name:        {}", manifest.name);
    println!("version:     {}", manifest.version);
    println!("kind:        {}", manifest.kind.as_str());
    println!("entry:       {}", manifest.entry);
    println!("size:        {} bytes", manifest.installed_bytes);
    println!("files:       {}", manifest.files.len());
    println!(
        "signature:   {}",
        manifest.signature.clone().unwrap_or_else(|| "none".into())
    );
    println!("permissions:");
    if manifest.permissions.is_empty() {
        println!("  (none)");
    }
    for permission in &manifest.permissions {
        println!("  - {}", permission.label());
    }
}

fn fail(message: &str) -> ! {
    eprintln!("ERROR {message}");
    std::process::exit(1);
}

/// Write a launcher entry so the shell can show the app.
fn write_desktop_entry(paths: &Paths, manifest: &aether_pkg::PackageManifest) -> std::io::Result<()> {
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={}\nX-Aether-App-Id={}\nCategories=Utility;\n",
        manifest.name,
        paths.app_dir(&manifest.id).join(&manifest.entry).display(),
        manifest.id
    );
    std::fs::create_dir_all(&paths.applications)?;
    let path = paths.applications.join(format!("{}.desktop", manifest.id));
    let mut file = std::fs::File::create(path)?;
    file.write_all(entry.as_bytes())
}
