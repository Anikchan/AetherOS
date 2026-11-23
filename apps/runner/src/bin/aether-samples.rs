//! Generate P6-T07 sample bundles (native, cross-platform, AppImage).
//!
//! Usage: aether-samples --out DIR
//!
//! Each sample is a real bundle with correct checksums, so it can be inspected,
//! verified, installed and planned exactly like a third-party app.

use std::fs;
use std::path::{Path, PathBuf};

use aether_pkg::manifest::{compute_files, write_manifest, AppId, PackageKind, PackageManifest};

fn main() {
    let mut out = PathBuf::from("apps/runner/samples");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = args.next().map(PathBuf::from).unwrap_or_else(|| {
                    eprintln!("--out needs a directory");
                    std::process::exit(2);
                })
            }
            "-h" | "--help" => {
                println!("usage: aether-samples [--out DIR]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).unwrap();

    let native = write_bundle(
        &out.join("native"),
        "org.aether.samples.Native",
        "Aether Native Sample",
        "1.0.0",
        PackageKind::NativeBundle,
        "bin/native-sample",
    );
    let cross = write_bundle(
        &out.join("cross-platform"),
        "org.aether.samples.CrossPlatform",
        "Cross-platform Sample",
        "1.0.0",
        PackageKind::CrossPlatformBundle,
        "bin/cross-sample",
    );
    let appimage = write_appimage(&out.join("app").join("AetherSample.AppImage"));

    println!("SAMPLES_OK native={} cross={} appimage={}", native, cross, appimage);
}

fn write_bundle(
    root: &Path,
    id: &str,
    name: &str,
    version: &str,
    kind: PackageKind,
    entry: &str,
) -> String {
    let script = root.join(entry);
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(
        &script,
        format!("#!/bin/sh\necho \"{name} {version} running (sample)\"\nexit 0\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&script, fs::Permissions::from_mode(0o755));
    }
    fs::write(
        root.join("share/README.txt"),
        format!("{name} sample bundle for P6-T07.\n"),
    )
    .unwrap_or_else(|_| {
        fs::create_dir_all(root.join("share")).unwrap();
        fs::write(
            root.join("share/README.txt"),
            format!("{name} sample bundle for P6-T07.\n"),
        )
        .unwrap();
    });
    let files = compute_files(root).unwrap();
    let manifest = PackageManifest {
        id: AppId::new(id).unwrap(),
        name: name.into(),
        version: version.into(),
        kind,
        permissions: vec![aether_pkg::Permission::Files {
            scope: aether_pkg::FileScope::PickedByUser,
        }],
        entry: entry.into(),
        installed_bytes: files.iter().map(|file| file.size).sum(),
        signature: None,
        files,
        description: Some("AetherOS sample bundle (P6-T07)".into()),
        homepage: None,
    };
    write_manifest(root, &manifest).unwrap();
    id.to_string()
}

fn write_appimage(path: &Path) -> String {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "#!/bin/sh\necho \"AppImage sample running\"\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    }
    path.to_string_lossy().into_owned()
}
