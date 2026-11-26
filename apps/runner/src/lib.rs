//! Engine selection and launch planning (P6-T02/T03/T05/T07, D8 section 5).
//!
//! The runner never starts an app unsandboxed: the sandbox enforcer must be
//! available, otherwise launch fails with an explanation (D8 failure modes).

pub mod compat;
pub mod compat_harness;
pub mod compat_reference;
pub mod engine_package;
pub mod windows;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aether_pkg::manifest::{AppId, PackageKind};
use aether_pkg::{InstalledApp, Installer, Paths, Registry};
use aether_sandbox::{Enforcer, Sandbox, SandboxCapabilities, SandboxError, SandboxProfile};

pub use engine_package::{EngineError, EngineFile, EnginePackage, ENGINE_MANIFEST};
pub use windows::{
    WindowsAppRecord, WindowsError, WindowsInstallOutcome, WindowsInstallPlan,
    WindowsInstallRequest, WindowsInstallerFlow, WindowsUninstallOutcome, WINDOWS_APP_RECORD,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "engine")]
pub enum EngineId {
    NativeRunner,
    AppImageRunner,
    CrossPlatformRunner,
    WineEngine { version: String },
}

impl EngineId {
    pub fn label(&self) -> String {
        match self {
            Self::NativeRunner => "Native runner".into(),
            Self::AppImageRunner => "AppImage runner".into(),
            Self::CrossPlatformRunner => "Cross-platform runner".into(),
            Self::WineEngine { version } => format!("Windows engine {version}"),
        }
    }
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("app {0} is not installed")]
    NotInstalled(AppId),
    #[error("no engine is available for {0}")]
    EngineMissing(String),
    #[error("sandbox failed: {0}")]
    SandboxFailed(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<SandboxError> for LaunchError {
    fn from(error: SandboxError) -> Self {
        Self::SandboxFailed(error.to_string())
    }
}

/// Finds Windows engine versions on disk.
#[derive(Debug, Clone)]
pub struct EngineRegistry {
    engine_root: PathBuf,
}

impl EngineRegistry {
    pub fn system() -> Self {
        Self {
            engine_root: PathBuf::from("/usr/lib/aether/engines"),
        }
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            engine_root: root.into(),
        }
    }

    /// Installed `wine-<version>` directories, newest name first.
    pub fn wine_versions(&self) -> Vec<String> {
        let mut versions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.engine_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(version) = name.strip_prefix("wine-") {
                    if entry.path().is_dir() {
                        versions.push(version.to_string());
                    }
                }
            }
        }
        versions.sort();
        versions.reverse();
        versions
    }

    /// PATH fallback keeps development builds usable.
    pub fn system_wine(&self) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("wine"))
            .find(|candidate| candidate.is_file())
    }

    pub fn engine_for(&self, kind: PackageKind) -> Option<EngineId> {
        match kind {
            PackageKind::NativeBundle => Some(EngineId::NativeRunner),
            PackageKind::AppImage => Some(EngineId::AppImageRunner),
            PackageKind::CrossPlatformBundle => Some(EngineId::CrossPlatformRunner),
            PackageKind::WindowsInstaller => {
                self.wine_versions()
                    .first()
                    .map(|version| EngineId::WineEngine {
                        version: version.clone(),
                    })
                    .or_else(|| {
                        self.system_wine().map(|_| EngineId::WineEngine {
                            version: "system".into(),
                        })
                    })
            }
            PackageKind::MacosPackage => None,
        }
    }

    /// The wine binary for an engine, if any.
    pub fn wine_binary(&self, version: &str) -> Option<PathBuf> {
        if version == "system" {
            return self.system_wine();
        }
        let candidate = self.engine_root.join(format!("wine-{version}")).join("bin/wine");
        candidate.is_file().then_some(candidate)
    }

    /// Packaged engines with their manifests, sorted by version.
    pub fn packages(&self) -> Vec<EnginePackage> {
        let mut packages = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.engine_root) else {
            return packages;
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            if let Ok(package) = EnginePackage::read(&entry.path()) {
                packages.push(package);
            }
        }
        packages.sort_by(|a, b| a.version.cmp(&b.version));
        packages.reverse();
        packages
    }

    /// Verify every packaged engine; returns `(version, problems)`.
    pub fn verify_all(&self) -> Vec<(String, Vec<String>)> {
        self.packages()
            .into_iter()
            .map(|package| {
                let root = self.engine_root.join(package.dir_name());
                let problems = package.verify(&root).unwrap_or_else(|error| vec![error.to_string()]);
                (package.version.clone(), problems)
            })
            .collect()
    }

    /// Write `aether-engine.toml` into a staged engine directory (P6-T05).
    pub fn package(
        &self,
        source: &Path,
        name: &str,
        version: &str,
        arch: &str,
    ) -> Result<EnginePackage, EngineError> {
        let package = EnginePackage::compute(source, name, version, arch)?;
        package.write(source)?;
        Ok(package)
    }
}

/// Plans a launch without starting it (used by the UI and tests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub app: AppId,
    pub engine: EngineId,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub profile: SandboxProfile,
    pub wine_prefix: Option<PathBuf>,
}

impl LaunchPlan {
    /// Human-readable command line for the UI/diagnostics.
    pub fn preview(&self) -> String {
        let mut line = self.program.display().to_string();
        for arg in &self.args {
            line.push(' ');
            line.push_str(arg);
        }
        line
    }
}

/// One running app process.
#[derive(Debug)]
pub struct ProcessHandle {
    pub pid: u32,
    pub plan: LaunchPlan,
    child: Option<std::process::Child>,
}

impl ProcessHandle {
    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match &mut self.child {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }
}

/// Windows prefix storage (D8 section 3).
#[derive(Debug, Clone)]
pub struct PrefixManager {
    pub root: PathBuf,
}

impl PrefixManager {
    pub fn new(paths: &Paths) -> Self {
        Self {
            root: paths.prefixes.clone(),
        }
    }

    pub fn prefix(&self, id: &AppId) -> PathBuf {
        self.root.join(id.as_str())
    }

    pub fn ensure(&self, id: &AppId) -> std::io::Result<PathBuf> {
        let prefix = self.prefix(id);
        std::fs::create_dir_all(&prefix)?;
        // WINEPREFIX is initialised by wine on first run; we only own the
        // folder so uninstall can remove it.
        Ok(prefix)
    }

    pub fn remove(&self, id: &AppId) -> std::io::Result<()> {
        let prefix = self.prefix(id);
        if prefix.exists() {
            std::fs::remove_dir_all(prefix)?;
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<PathBuf> {
        let mut prefixes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    prefixes.push(entry.path());
                }
            }
        }
        prefixes.sort();
        prefixes
    }
}

#[derive(Debug)]
pub struct Runner {
    pub paths: Paths,
    pub engines: EngineRegistry,
    pub dry_run: bool,
    /// Fail-closed Landlock enforcement through the `aether-landlock` shim.
    pub strict_landlock: bool,
    capabilities: Option<SandboxCapabilities>,
}

impl Runner {
    pub fn new(paths: Paths, engines: EngineRegistry) -> Self {
        Self {
            paths,
            engines,
            dry_run: false,
            strict_landlock: false,
            capabilities: None,
        }
    }

    pub fn dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// Require every kernel-enforced layer on top of the namespace sandbox:
    /// Landlock and seccomp (P6-T04). When either cannot be enforced, launch
    /// fails closed instead of falling back to a weaker sandbox.
    pub fn require_strict_layers(mut self, required: bool) -> Self {
        self.strict_landlock = required;
        self
    }

    /// Backwards-compatible name for [`Self::require_strict_layers`].
    pub fn require_landlock(self, required: bool) -> Self {
        self.require_strict_layers(required)
    }

    /// Test seam for the sandbox capabilities.
    pub fn with_capabilities(mut self, capabilities: SandboxCapabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }

    fn registry(&self) -> Registry {
        Registry::load(&self.paths.registry)
    }

    fn enforcer(&self) -> Result<Enforcer, LaunchError> {
        match &self.capabilities {
            Some(capabilities) => {
                let bwrap = capabilities
                    .bwrap
                    .clone()
                    .ok_or_else(|| LaunchError::SandboxFailed(
                        "bubblewrap is not installed; the app will not start unsandboxed".into(),
                    ))?;
                Ok(Enforcer::with_bwrap(bwrap, capabilities.clone()))
            }
            None => Enforcer::system().map_err(LaunchError::from),
        }
    }

    pub fn plan(&self, id: &AppId) -> Result<LaunchPlan, LaunchError> {
        let registry = self.registry();
        let installed = registry
            .get(id)
            .ok_or_else(|| LaunchError::NotInstalled(id.clone()))?;
        let manifest = &installed.manifest;
        let engine = self.engines.engine_for(manifest.kind).ok_or_else(|| {
            LaunchError::EngineMissing(format!(
                "{} packages cannot run on AetherOS (detector P6-T08)",
                manifest.kind.as_str()
            ))
        })?;

        let granted: BTreeSet<String> = aether_sandbox::Broker::load(
            self.paths.registry.with_file_name("permissions.json"),
        )
        .store
        .granted(id);
        let sandbox = Sandbox::new(granted, self.paths.data_dir(id), home_dir());
        let mut profile = sandbox.profile_for(manifest);

        let (program, args, wine_prefix) = match &engine {
            EngineId::WineEngine { version } => {
                let wine = self
                    .engines
                    .wine_binary(version)
                    .ok_or_else(|| LaunchError::EngineMissing(format!("wine {version}")))?;
                let prefix = PrefixManager::new(&self.paths).prefix(id);
                profile.file_write.push(prefix.clone());
                // Once the vendor installer has run, launch the program it
                // produced; before that the payload is the installer itself.
                let entry = windows::program_path(&prefix)
                    .unwrap_or_else(|| installed.target.join(&manifest.entry));
                (wine, vec![entry.display().to_string()], Some(prefix))
            }
            EngineId::NativeRunner
            | EngineId::AppImageRunner
            | EngineId::CrossPlatformRunner => {
                (installed.target.join(&manifest.entry), Vec::new(), None)
            }
        };

        Ok(LaunchPlan {
            app: id.clone(),
            engine,
            program,
            args,
            profile,
            wine_prefix,
        })
    }

    /// Launch the app inside the sandbox.
    pub fn launch(&self, id: &AppId) -> Result<ProcessHandle, LaunchError> {
        let plan = self.plan(id)?;
        let enforcer = self.enforcer()?;
        let program = plan.program.clone();
        let mut command = if self.strict_landlock {
            enforcer.command_strict(
                &plan.profile,
                &program,
                Path::new(aether_sandbox::landlock::SHIM_PATH),
            )?
        } else {
            enforcer.command(&plan.profile, &program)?
        };
        for arg in &plan.args {
            command.arg(arg);
        }
        if let Some(prefix) = &plan.wine_prefix {
            command.env("WINEPREFIX", prefix);
        }
        command.stdin(Stdio::null());
        if self.dry_run {
            return Ok(ProcessHandle {
                pid: 0,
                plan,
                child: None,
            });
        }
        let child = command.spawn()?;
        Ok(ProcessHandle {
            pid: child.id(),
            plan,
            child: Some(child),
        })
    }

    /// Installed apps grouped by kind (launcher refresh).
    pub fn installed(&self) -> Vec<InstalledApp> {
        self.registry().list().into_iter().cloned().collect()
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/user"))
}

/// Build an installer using this runner's paths.
pub fn installer_for(paths: &Paths) -> Installer {
    Installer::new(paths.clone(), BTreeSet::new())
}

/// Location of the desktop entry the shell shows.
pub fn desktop_entry(paths: &Paths, id: &AppId) -> PathBuf {
    paths.applications.join(format!("{id}.desktop"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_pkg::manifest::{compute_files, write_manifest, PackageManifest};
    use aether_pkg::PackageInstaller;
    use std::path::Path;
    use std::io::Write;

    fn temp_root(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aether-runner-{tag}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_bundle(root: &Path, kind: PackageKind) -> (Paths, AppId) {
        let bundle = root.join("bundle");
        std::fs::create_dir_all(bundle.join("bin")).unwrap();
        let mut file = std::fs::File::create(bundle.join("bin/app")).unwrap();
        writeln!(file, "#!/bin/sh\necho hello").unwrap();
        let files = compute_files(&bundle).unwrap();
        let manifest = PackageManifest {
            id: AppId::new("org.example.Hello").unwrap(),
            name: "Hello".into(),
            version: "1.0.0".into(),
            kind,
            permissions: vec![],
            entry: "bin/app".into(),
            installed_bytes: files.iter().map(|f| f.size).sum(),
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

    fn capabilities(bwrap: Option<&str>) -> SandboxCapabilities {
        SandboxCapabilities {
            bwrap: bwrap.map(PathBuf::from),
            user_namespaces: true,
            landlock: false,
        }
    }

    #[test]
    fn plans_native_launch_inside_the_sandbox() {
        let root = temp_root("native");
        let (paths, id) = make_bundle(&root, PackageKind::NativeBundle);
        let runner = Runner::new(paths.clone(), EngineRegistry::at(root.join("engines")))
            .with_capabilities(capabilities(Some("/usr/bin/bwrap")));
        let plan = runner.plan(&id).unwrap();
        assert_eq!(plan.engine, EngineId::NativeRunner);
        assert!(plan.program.ends_with("bin/app"));
        assert!(!plan.profile.network);
        assert!(plan.profile.file_write.contains(&paths.data_dir(&id)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_sandbox_refuses_launch() {
        let root = temp_root("nosandbox");
        let (paths, id) = make_bundle(&root, PackageKind::NativeBundle);
        let runner = Runner::new(paths, EngineRegistry::at(root.join("engines")))
            .with_capabilities(capabilities(None));
        assert!(matches!(
            runner.launch(&id),
            Err(LaunchError::SandboxFailed(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dry_run_launch_returns_a_plan_without_spawning() {
        let root = temp_root("dry");
        let (paths, id) = make_bundle(&root, PackageKind::NativeBundle);
        let runner = Runner::new(paths, EngineRegistry::at(root.join("engines")))
            .with_capabilities(capabilities(Some("/usr/bin/bwrap")))
            .dry_run();
        let handle = runner.launch(&id).unwrap();
        assert_eq!(handle.pid, 0);
        assert!(handle.plan.preview().contains("bin/app"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn strict_layers_are_fail_closed() {
        let root = temp_root("strict-ll");
        let (paths, id) = make_bundle(&root, PackageKind::NativeBundle);
        let landlock = aether_sandbox::landlock::probe().is_available();
        let runner = Runner::new(paths.clone(), EngineRegistry::at(root.join("engines")))
            .with_capabilities(SandboxCapabilities {
                bwrap: Some(PathBuf::from("/usr/bin/bwrap")),
                user_namespaces: true,
                landlock,
            })
            .require_strict_layers(true)
            .dry_run();
        let seccomp = aether_sandbox::seccomp::probe().available;
        match runner.launch(&id) {
            Ok(handle) => {
                assert!(
                    landlock && seccomp,
                    "a strict launch only succeeds where both kernel layers exist"
                );
                assert_eq!(handle.pid, 0);
            }
            Err(LaunchError::SandboxFailed(message)) => {
                let message = message.to_ascii_lowercase();
                assert!(
                    message.contains("landlock") || message.contains("seccomp"),
                    "unexpected refusal: {message}"
                );
            }
            Err(other) => panic!("unexpected launch error: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn uninstalled_app_is_reported() {
        let root = temp_root("missing");
        let runner = Runner::new(
            Paths::at(root.join("store")),
            EngineRegistry::at(root.join("engines")),
        )
        .with_capabilities(capabilities(Some("/usr/bin/bwrap")));
        let id = AppId::new("org.example.Nope").unwrap();
        assert!(matches!(runner.plan(&id), Err(LaunchError::NotInstalled(_))));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn macos_package_has_no_engine() {
        let engines = EngineRegistry::at("/nonexistent/engines");
        assert!(engines.engine_for(PackageKind::MacosPackage).is_none());
    }

    #[test]
    fn wine_engine_is_detected_from_engine_root() {
        let root = temp_root("wine");
        std::fs::create_dir_all(root.join("engines/wine-10.0/bin")).unwrap();
        std::fs::write(root.join("engines/wine-10.0/bin/wine"), "#!/bin/sh\n").unwrap();
        let engines = EngineRegistry::at(root.join("engines"));
        assert_eq!(engines.wine_versions(), vec!["10.0".to_string()]);
        assert_eq!(
            engines.engine_for(PackageKind::WindowsInstaller),
            Some(EngineId::WineEngine {
                version: "10.0".into()
            })
        );
        assert!(engines.wine_binary("10.0").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn windows_plan_uses_prefix_and_wine() {
        let root = temp_root("winplan");
        std::fs::create_dir_all(root.join("engines/wine-10.0/bin")).unwrap();
        std::fs::write(root.join("engines/wine-10.0/bin/wine"), "#!/bin/sh\n").unwrap();
        let (paths, id) = make_bundle(&root, PackageKind::WindowsInstaller);
        let runner = Runner::new(paths.clone(), EngineRegistry::at(root.join("engines")))
            .with_capabilities(capabilities(Some("/usr/bin/bwrap")));
        let plan = runner.plan(&id).unwrap();
        assert!(matches!(plan.engine, EngineId::WineEngine { .. }));
        assert!(plan.wine_prefix.is_some());
        assert!(plan.profile.file_write.iter().any(|path| path.starts_with(root.join("store/prefixes"))));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prefixes_are_created_and_removed() {
        let root = temp_root("prefix");
        let paths = Paths::at(root.join("store"));
        let manager = PrefixManager::new(&paths);
        let id = AppId::new("win.example").unwrap();
        let prefix = manager.ensure(&id).unwrap();
        assert!(prefix.is_dir());
        assert_eq!(manager.list().len(), 1);
        manager.remove(&id).unwrap();
        assert!(manager.list().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn signature_helper_is_exposed_for_bundles() {
        // P6-T07 samples use the same manifest helpers as real bundles.
        let root = temp_root("sample");
        let bundle = root.join("sample");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("run.sh"), "#!/bin/sh\n").unwrap();
        assert!(!compute_files(&bundle).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
