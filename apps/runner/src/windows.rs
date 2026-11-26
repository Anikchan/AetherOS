//! Windows installer flow and prefix-only uninstall (P6-T06, D8 section 5).
//!
//! The OS owns the whole experience: the user opens a `.exe`/`.msi`, the
//! installer runs inside a dedicated WINEPREFIX, and the installed program gets
//! a launcher entry. Uninstall removes only that prefix and its launcher; the
//! shared engine and every other prefix stay untouched (AP-AC-03/04).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aether_pkg::manifest::AppId;
use aether_pkg::Paths;

use crate::engine_package::EngineError;
use crate::{desktop_entry, EngineId, EngineRegistry, LaunchError, PrefixManager};

/// Per-prefix record of the installed Windows program.
pub const WINDOWS_APP_RECORD: &str = "aether-windows-app.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsAppRecord {
    pub app: String,
    pub name: String,
    /// Program path relative to the prefix.
    pub entry: String,
    pub installer: String,
    pub installed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInstallRequest {
    pub id: AppId,
    pub installer: PathBuf,
    /// Program produced by the installer, relative to the prefix. When omitted
    /// the flow discovers the first `.exe` under `drive_c/Program Files`.
    pub entry: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsInstallPlan {
    pub app: AppId,
    pub engine: EngineId,
    pub prefix: PathBuf,
    pub installer: PathBuf,
    pub args: Vec<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsInstallOutcome {
    pub app: AppId,
    pub prefix: PathBuf,
    pub record: PathBuf,
    pub launcher: PathBuf,
    pub entry: String,
    /// False on a dry run or in tests where the installer was not executed.
    pub executed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsUninstallOutcome {
    pub app: AppId,
    pub removed_prefix: PathBuf,
    pub removed_launcher: PathBuf,
    /// The shared Windows engine is never removed by an app uninstall.
    pub kept_engine: bool,
}

#[derive(Debug, Error)]
pub enum WindowsError {
    #[error(transparent)]
    Launch(#[from] LaunchError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("no Windows engine is installed (Pack B)")]
    EngineMissing,
    #[error("installer {0} does not exist")]
    InstallerMissing(PathBuf),
    #[error("installer exited with status {0}")]
    InstallerFailed(String),
    #[error("the installer did not produce a program; pass --entry <path in prefix>")]
    EntryNotSpecified,
    #[error("entry must be relative to the prefix: {0}")]
    InvalidEntry(String),
    #[error("app {0} has no Windows prefix")]
    PrefixMissing(AppId),
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl WindowsError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Flow driver: plan, run the vendor installer, write the launcher, uninstall.
#[derive(Debug)]
pub struct WindowsInstallerFlow {
    paths: Paths,
    engines: EngineRegistry,
    dry_run: bool,
}

impl WindowsInstallerFlow {
    pub fn new(paths: Paths, engines: EngineRegistry) -> Self {
        Self {
            paths,
            engines,
            dry_run: false,
        }
    }

    pub fn dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    pub fn prefix(&self, id: &AppId) -> PathBuf {
        PrefixManager::new(&self.paths).prefix(id)
    }

    pub fn plan(&self, request: &WindowsInstallRequest) -> Result<WindowsInstallPlan, WindowsError> {
        let engine = self
            .engines
            .engine_for(aether_pkg::PackageKind::WindowsInstaller)
            .ok_or(WindowsError::EngineMissing)?;
        if !request.installer.is_file() {
            return Err(WindowsError::InstallerMissing(request.installer.clone()));
        }
        if let Some(entry) = &request.entry {
            validate_entry(entry)?;
        }
        Ok(WindowsInstallPlan {
            app: request.id.clone(),
            engine,
            prefix: self.prefix(&request.id),
            installer: request.installer.clone(),
            args: request.args.clone(),
            dry_run: self.dry_run,
        })
    }

    /// Run the installer in its own prefix and publish a launcher entry.
    pub fn install(&self, request: &WindowsInstallRequest) -> Result<WindowsInstallOutcome, WindowsError> {
        let plan = self.plan(request)?;
        let prefix = plan.prefix.clone();
        let launcher = desktop_entry(&self.paths, &plan.app);
        if self.dry_run {
            let entry = request
                .entry
                .clone()
                .unwrap_or_else(|| "drive_c/Program Files/<app>/<app>.exe".into());
            return Ok(WindowsInstallOutcome {
                app: plan.app,
                prefix,
                record: plan.prefix.join(WINDOWS_APP_RECORD),
                launcher,
                entry,
                executed: false,
            });
        }

        fs::create_dir_all(&prefix).map_err(|error| WindowsError::io(&prefix, error))?;
        let wine = wine_program(&self.engines, &plan.engine).ok_or(WindowsError::EngineMissing)?;
        let status = std::process::Command::new(&wine)
            .arg(&plan.installer)
            .args(&plan.args)
            .env("WINEPREFIX", &prefix)
            .stdin(Stdio::null())
            .status()
            .map_err(|error| WindowsError::io(&wine, error))?;
        if !status.success() {
            return Err(WindowsError::InstallerFailed(status.to_string()));
        }

        let entry = match &request.entry {
            Some(entry) => entry.clone(),
            None => discover_entry(&prefix).ok_or(WindowsError::EntryNotSpecified)?,
        };
        let record = WindowsAppRecord {
            app: plan.app.to_string(),
            name: request
                .installer
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| plan.app.to_string()),
            entry: entry.clone(),
            installer: plan.installer.display().to_string(),
            installed_at: now_secs(),
        };
        let record_path = prefix.join(WINDOWS_APP_RECORD);
        let text = serde_json::to_string_pretty(&record)
            .map_err(|error| WindowsError::io(&record_path, io::Error::other(error)))?;
        fs::write(&record_path, text).map_err(|error| WindowsError::io(&record_path, error))?;

        fs::create_dir_all(&self.paths.applications)
            .map_err(|error| WindowsError::io(&self.paths.applications, error))?;
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec=aether-runner launch {}\nX-Aether-App-Id={}\nX-Aether-Runtime=windows\nCategories=Utility;\n",
            record.name, plan.app, plan.app
        );
        fs::write(&launcher, desktop).map_err(|error| WindowsError::io(&launcher, error))?;

        Ok(WindowsInstallOutcome {
            app: plan.app,
            prefix,
            record: record_path,
            launcher,
            entry,
            executed: true,
        })
    }

    /// Remove only the app's prefix and its launcher (AP-AC-04).
    pub fn uninstall(&self, id: &AppId) -> Result<WindowsUninstallOutcome, WindowsError> {
        let prefix = self.prefix(id);
        if !prefix.exists() {
            return Err(WindowsError::PrefixMissing(id.clone()));
        }
        let launcher = desktop_entry(&self.paths, id);
        if !self.dry_run {
            fs::remove_dir_all(&prefix).map_err(|error| WindowsError::io(&prefix, error))?;
            if launcher.exists() {
                fs::remove_file(&launcher).map_err(|error| WindowsError::io(&launcher, error))?;
            }
        }
        Ok(WindowsUninstallOutcome {
            app: id.clone(),
            removed_prefix: prefix,
            removed_launcher: launcher,
            kept_engine: true,
        })
    }
}

/// Recorded program for a prefix, if the installer flow wrote one.
pub fn record_entry(prefix: &Path) -> Option<String> {
    let text = fs::read_to_string(prefix.join(WINDOWS_APP_RECORD)).ok()?;
    let record: WindowsAppRecord = serde_json::from_str(&text).ok()?;
    Some(record.entry)
}

/// Full path of the recorded program.
pub fn program_path(prefix: &Path) -> Option<PathBuf> {
    record_entry(prefix).map(|entry| prefix.join(entry))
}

fn wine_program(engines: &EngineRegistry, engine: &EngineId) -> Option<PathBuf> {
    match engine {
        EngineId::WineEngine { version } => engines.wine_binary(version),
        _ => None,
    }
}

fn validate_entry(entry: &str) -> Result<(), WindowsError> {
    if entry.contains("..") || Path::new(entry).is_absolute() {
        return Err(WindowsError::InvalidEntry(entry.into()));
    }
    Ok(())
}

/// First `.exe` under `drive_c/Program Files`, skipping uninstallers.
fn discover_entry(prefix: &Path) -> Option<String> {
    let root = prefix.join("drive_c/Program Files");
    let mut found = Vec::new();
    collect_exes(prefix, &root, &mut found);
    found.sort();
    found.into_iter().next().map(|path| path.replace('\\', "/"))
}

fn collect_exes(prefix: &Path, at: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(at) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_exes(prefix, &path, found);
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !name.ends_with(".exe") || name.contains("unins") {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(prefix) {
            found.push(relative.to_string_lossy().into_owned());
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_pkg::manifest::AppId;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aether-windows-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_engine(root: &Path) -> EngineRegistry {
        let bin = root.join("engines/wine-10.0/bin");
        fs::create_dir_all(&bin).unwrap();
        let wine = bin.join("wine");
        fs::write(
            &wine,
            "#!/bin/sh\nmkdir -p \"$WINEPREFIX/drive_c/Program Files/Hello\"\nprintf MZ > \"$WINEPREFIX/drive_c/Program Files/Hello/hello.exe\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&wine).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&wine, permissions).unwrap();
        }
        EngineRegistry::at(root.join("engines"))
    }

    fn request(root: &Path) -> WindowsInstallRequest {
        let installer = root.join("setup.exe");
        fs::write(&installer, "MZ").unwrap();
        WindowsInstallRequest {
            id: AppId::new("win.hello").unwrap(),
            installer,
            entry: None,
            args: vec!["/S".into()],
        }
    }

    #[test]
    fn plan_refuses_without_an_engine() {
        let root = temp_root("noengine");
        let flow = WindowsInstallerFlow::new(
            Paths::at(root.join("store")),
            EngineRegistry::at(root.join("engines")),
        );
        assert!(matches!(
            flow.plan(&request(&root)),
            Err(WindowsError::EngineMissing)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_runs_in_its_own_prefix_and_publishes_a_launcher() {
        let root = temp_root("install");
        let engines = fake_engine(&root);
        let paths = Paths::at(root.join("store"));
        fs::write(root.join("setup.exe"), "MZ").unwrap();
        let flow = WindowsInstallerFlow::new(paths.clone(), engines.clone());
        let outcome = flow.install(&request(&root)).unwrap();
        assert!(outcome.executed);
        assert!(outcome.prefix.join(WINDOWS_APP_RECORD).is_file());
        assert!(outcome.launcher.is_file());
        assert_eq!(outcome.entry, "drive_c/Program Files/Hello/hello.exe");
        assert_eq!(
            program_path(&outcome.prefix).unwrap(),
            outcome.prefix.join("drive_c/Program Files/Hello/hello.exe")
        );
        let desktop = fs::read_to_string(&outcome.launcher).unwrap();
        assert!(desktop.contains("Exec=aether-runner launch win.hello"));
        // A second install gets its own prefix.
        let mut second = request(&root);
        second.id = AppId::new("win.other").unwrap();
        let other = flow.install(&second).unwrap();
        assert_ne!(outcome.prefix, other.prefix);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_entry_must_stay_inside_the_prefix() {
        let root = temp_root("entry");
        let engines = fake_engine(&root);
        let flow = WindowsInstallerFlow::new(Paths::at(root.join("store")), engines);
        let mut bad = request(&root);
        bad.entry = Some("../escape.exe".into());
        assert!(matches!(flow.plan(&bad), Err(WindowsError::InvalidEntry(_))));
        bad.entry = Some("drive_c/Program Files/Hello/hello.exe".into());
        assert!(flow.plan(&bad).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_removes_only_its_prefix_and_launcher() {
        let root = temp_root("uninstall");
        let engines = fake_engine(&root);
        let paths = Paths::at(root.join("store"));
        fs::write(root.join("setup.exe"), "MZ").unwrap();
        let flow = WindowsInstallerFlow::new(paths.clone(), engines);
        let outcome = flow.install(&request(&root)).unwrap();
        let mut other = request(&root);
        other.id = AppId::new("win.other").unwrap();
        let other_outcome = flow.install(&other).unwrap();

        let removed = flow.uninstall(&outcome.app).unwrap();
        assert!(removed.kept_engine);
        assert!(!removed.removed_prefix.exists());
        assert!(!removed.removed_launcher.exists());
        assert!(other_outcome.prefix.exists(), "other prefixes are untouched");
        assert!(other_outcome.launcher.exists());
        assert!(root.join("engines/wine-10.0/bin/wine").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let root = temp_root("dry");
        let engines = fake_engine(&root);
        let paths = Paths::at(root.join("store"));
        let flow = WindowsInstallerFlow::new(paths, engines).dry_run();
        let outcome = flow.install(&request(&root)).unwrap();
        assert!(!outcome.executed);
        assert!(!outcome.prefix.exists());
        assert!(!outcome.launcher.exists());
        let _ = fs::remove_dir_all(root);
    }
}
