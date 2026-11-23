//! AetherOS package format and installer (P6-T01/T02).
//!
//! Contract and flow: D8 sections 2–4. A native bundle is a directory with
//! `aether-manifest.toml`; installs are staged, checksum-verified and moved
//! atomically into `~/Applications/<app-id>`.

pub mod installer;
pub mod manifest;

pub use installer::{
    InstallError, InstallOutcome, InstalledApp, Installer, PackageInstaller, Paths, Registry,
};
pub use manifest::{
    appimage_manifest, compute_files, missing_permissions, parse_manifest, read_manifest,
    sha256_bytes, sha256_file, verify_files, write_manifest, AppId, FileEntry, FileScope,
    ManifestError, PackageKind, PackageManifest, Permission, MANIFEST_NAME,
};
