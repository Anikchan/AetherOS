//! Integration tests for the Windows app platform (P6-T05 packaging, P6-T06
//! installer flow) across the module boundaries: package a fake engine, install
//! a Windows app through the flow, resolve the launch plan through the runner,
//! and uninstall without touching the shared engine or other prefixes.
//!
//! The stub engine is a shell script that behaves like `wine`: it reads
//! `WINEPREFIX` and writes the program the vendor installer would produce, so
//! the whole flow runs on the dev host and in CI without a real engine.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use aether_pkg::manifest::{compute_files, write_manifest, AppId, PackageKind, PackageManifest};
use aether_pkg::{Installer, PackageInstaller, Paths};
use aether_runner::{
    windows, EngineRegistry, Runner, WindowsInstallRequest, WindowsInstallerFlow,
};
use aether_sandbox::SandboxCapabilities;

fn temp_root(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "aether-windows-it-{tag}-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A staged engine tree plus its `aether-engine.toml` manifest.
fn packaged_engine(root: &Path) -> EngineRegistry {
    let engine = root.join("engines/wine-10.0");
    fs::create_dir_all(engine.join("bin")).unwrap();
    fs::create_dir_all(engine.join("share/wine")).unwrap();
    let wine = engine.join("bin/wine");
    fs::write(
        &wine,
        "#!/bin/sh\n\
         mkdir -p \"$WINEPREFIX/drive_c/Program Files/Hello\"\n\
         printf 'MZ stub' > \"$WINEPREFIX/drive_c/Program Files/Hello/hello.exe\"\n\
         printf 'uninstaller' > \"$WINEPREFIX/drive_c/Program Files/Hello/unins000.exe\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&wine).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wine, permissions).unwrap();
    }
    fs::write(engine.join("share/wine/hello.txt"), "engine payload").unwrap();

    let registry = EngineRegistry::at(root.join("engines"));
    let package = registry.package(&engine, "wine", "10.0", "x86_64").unwrap();
    assert_eq!(package.dir_name(), "wine-10.0");
    assert!(!package.files.is_empty(), "manifest pins every engine file");
    registry
}

/// Install a Windows installer bundle into the store the runner reads.
fn install_windows_app(root: &Path) -> (Paths, AppId) {
    let bundle = root.join("bundle");
    fs::create_dir_all(&bundle).unwrap();
    fs::write(bundle.join("setup.exe"), "MZ installer payload").unwrap();
    let files = compute_files(&bundle).unwrap();
    let manifest = PackageManifest {
        id: AppId::new("win.hello").unwrap(),
        name: "Hello".into(),
        version: "1.0.0".into(),
        kind: PackageKind::WindowsInstaller,
        permissions: vec![],
        entry: "setup.exe".into(),
        installed_bytes: files.iter().map(|file| file.size).sum(),
        signature: None,
        files,
        description: None,
        homepage: None,
    };
    write_manifest(&bundle, &manifest).unwrap();
    let paths = Paths::at(root.join("store"));
    let installer = Installer::new(paths.clone(), BTreeSet::new());
    installer.install(&manifest, &bundle).unwrap();
    (paths, manifest.id)
}

fn capabilities() -> SandboxCapabilities {
    SandboxCapabilities {
        bwrap: Some(PathBuf::from("/usr/bin/bwrap")),
        user_namespaces: true,
        landlock: false,
    }
}

#[test]
fn packaged_engine_round_trips_and_verifies() {
    let root = temp_root("package");
    let registry = packaged_engine(&root);
    let engine = root.join("engines/wine-10.0");

    let package = aether_runner::EnginePackage::read(&engine).unwrap();
    assert_eq!(package.name, "wine");
    assert_eq!(package.version, "10.0");
    assert!(package.installed_bytes > 0);
    assert!(package.verify(&engine).unwrap().is_empty());
    assert!(engine.join(aether_runner::ENGINE_MANIFEST).is_file());

    // Discovery sees the packaged engine and its version.
    assert_eq!(registry.wine_versions(), vec!["10.0".to_string()]);
    assert_eq!(registry.packages().len(), 1);
    assert!(registry.verify_all().iter().all(|(_, problems)| problems.is_empty()));

    // Tampering is detected before the engine is used.
    fs::OpenOptions::new()
        .append(true)
        .open(engine.join("bin/wine"))
        .unwrap();
    fs::write(engine.join("share/wine/hello.txt"), "tampered payload here").unwrap();
    let problems = registry.verify_all();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].1.iter().any(|problem| problem.contains("share/wine/hello.txt")),
        "changed engine file is reported: {:?}",
        problems
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn windows_install_launch_and_uninstall_keep_the_engine() {
    let root = temp_root("flow");
    let registry = packaged_engine(&root);
    let (paths, id) = install_windows_app(&root);
    let flow = WindowsInstallerFlow::new(paths.clone(), registry.clone());

    let installer = root.join("bundle/setup.exe");
    let request = WindowsInstallRequest {
        id: id.clone(),
        installer: installer.clone(),
        entry: None,
        args: vec!["/S".into()],
    };

    // The installer runs inside its own prefix and the produced program is
    // recorded (the uninstaller is skipped).
    let outcome = flow.install(&request).unwrap();
    assert!(outcome.executed);
    assert_eq!(outcome.entry, "drive_c/Program Files/Hello/hello.exe");
    assert!(outcome.record.is_file());
    assert_eq!(
        windows::record_entry(&outcome.prefix).unwrap(),
        "drive_c/Program Files/Hello/hello.exe"
    );
    assert_eq!(
        windows::program_path(&outcome.prefix).unwrap(),
        outcome.prefix.join("drive_c/Program Files/Hello/hello.exe")
    );
    let desktop = fs::read_to_string(&outcome.launcher).unwrap();
    assert!(desktop.contains("Exec=aether-runner launch win.hello"));
    assert!(desktop.contains("X-Aether-Runtime=windows"));

    // The runner launches the recorded program through the packaged engine
    // inside the sandbox, with WINEPREFIX set to the app's own prefix.
    let runner = Runner::new(paths.clone(), registry.clone()).with_capabilities(capabilities());
    let plan = runner.plan(&id).unwrap();
    assert_eq!(
        plan.engine,
        aether_runner::EngineId::WineEngine {
            version: "10.0".into()
        }
    );
    assert_eq!(
        plan.program,
        root.join("engines/wine-10.0/bin/wine")
    );
    assert_eq!(
        plan.args.first().map(String::as_str),
        Some(
            outcome
                .prefix
                .join("drive_c/Program Files/Hello/hello.exe")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(plan.wine_prefix.as_deref(), Some(outcome.prefix.as_path()));
    assert!(plan.profile.file_write.contains(&outcome.prefix));
    assert!(!plan.profile.network, "a Windows installer gets no network by default");

    // A second app gets its own prefix and launcher.
    let mut second = request.clone();
    second.id = AppId::new("win.other").unwrap();
    second.installer = installer.clone();
    let other = flow.install(&second).unwrap();
    assert_ne!(other.prefix, outcome.prefix);
    assert!(other.prefix.is_dir());

    // Uninstall removes only this app's prefix and launcher.
    let removed = flow.uninstall(&id).unwrap();
    assert!(removed.kept_engine, "the shared engine is never removed");
    assert!(!removed.removed_prefix.exists());
    assert!(!removed.removed_launcher.exists());
    assert!(other.prefix.is_dir(), "other prefixes are untouched");
    assert!(other.launcher.is_file());
    assert!(root.join("engines/wine-10.0/bin/wine").is_file());
    assert!(aether_runner::PrefixManager::new(&paths)
        .list()
        .iter()
        .all(|prefix| prefix == &other.prefix));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn dry_run_install_writes_nothing_and_keeps_the_plan() {
    let root = temp_root("dry");
    let registry = packaged_engine(&root);
    let (paths, _id) = install_windows_app(&root);
    let flow = WindowsInstallerFlow::new(paths.clone(), registry).dry_run();

    let request = WindowsInstallRequest {
        id: AppId::new("win.dry").unwrap(),
        installer: root.join("bundle/setup.exe"),
        entry: Some("drive_c/Program Files/Hello/hello.exe".into()),
        args: vec![],
    };
    let plan = flow.plan(&request).unwrap();
    assert!(plan.dry_run);
    assert_eq!(plan.engine, aether_runner::EngineId::WineEngine { version: "10.0".into() });

    let outcome = flow.install(&request).unwrap();
    assert!(!outcome.executed);
    assert!(!outcome.prefix.exists());
    assert!(!outcome.launcher.exists());
    assert!(matches!(
        flow.uninstall(&request.id),
        Err(aether_runner::WindowsError::PrefixMissing(_))
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn uninstall_refuses_an_app_with_no_prefix() {
    let root = temp_root("missing");
    let registry = packaged_engine(&root);
    let (paths, _id) = install_windows_app(&root);
    let flow = WindowsInstallerFlow::new(paths, registry);
    assert!(matches!(
        flow.uninstall(&AppId::new("win.nope").unwrap()),
        Err(aether_runner::WindowsError::PrefixMissing(_))
    ));
    let _ = fs::remove_dir_all(root);
}
