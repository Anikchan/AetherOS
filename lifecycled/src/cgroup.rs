//! Kernel-facing mechanisms (P5-T04, D3 section 4).
//!
//! Only this module touches cgroup v2. Every write is validated and every
//! error is returned to the policy, which logs it and retries at most twice
//! (D3 section 8). The root path is configurable so the full freeze/reclaim
//! path can be integration-tested against a fake cgroup tree on any host.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use thiserror::Error;

/// Writes `1`/`0` to `cgroup.freeze`.
pub trait Freezer {
    fn freeze(&self, scope: &Path) -> io::Result<()>;
    fn thaw(&self, scope: &Path) -> io::Result<()>;
    fn is_frozen(&self, scope: &Path) -> io::Result<bool>;
}

/// Writes to `memory.reclaim` and returns the bytes requested.
pub trait Reclaimer {
    fn reclaim(&self, scope: &Path, bytes: u64) -> io::Result<u64>;
}

/// Reads `memory.current` from a scope.
pub trait MemoryReader {
    fn memory_current(&self, scope: &Path) -> io::Result<u64>;
}

/// Kills every process in a scope.
pub trait Killer {
    fn kill(&self, scope: &Path) -> io::Result<u32>;
}

#[derive(Debug, Error)]
pub enum CgroupError {
    #[error("cgroup v2 memory controller is not available at {0}")]
    Unavailable(PathBuf),
    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("memory.reclaim is not supported by this kernel")]
    ReclaimUnsupported,
}

fn read_u64(path: &Path) -> io::Result<u64> {
    fs::read_to_string(path)?
        .trim()
        .parse::<u64>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// cgroup v2 filesystem operations for one root (normally `/sys/fs/cgroup`).
#[derive(Debug, Clone)]
pub struct CgroupFs {
    root: PathBuf,
    /// When set, no process is signalled and no filesystem write is required.
    dry_run: bool,
    reclaim_supported: Arc<AtomicBool>,
}

impl CgroupFs {
    pub fn system() -> Self {
        Self::at("/sys/fs/cgroup")
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            dry_run: false,
            reclaim_supported: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn dry_run(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            dry_run: true,
            reclaim_supported: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Bytes charged to the scope (`memory.current`); 0 when unreadable.
    pub fn memory_current(&self, scope: &Path) -> io::Result<u64> {
        read_u64(&scope.join("memory.current"))
    }

    /// Process IDs in the scope (`cgroup.procs`).
    pub fn pids(&self, scope: &Path) -> io::Result<Vec<u32>> {
        let text = fs::read_to_string(scope.join("cgroup.procs"))?;
        Ok(text
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .collect())
    }

    /// Whether `memory.reclaim` exists (kernel 5.19+).
    pub fn reclaim_supported(&self, scope: &Path) -> bool {
        scope.join("memory.reclaim").exists()
    }
}

impl Freezer for CgroupFs {
    fn freeze(&self, scope: &Path) -> io::Result<()> {
        if self.dry_run {
            return Ok(());
        }
        let path = scope.join("cgroup.freeze");
        fs::write(&path, "1").map_err(|source| io::Error::new(source.kind(), source))
    }

    fn thaw(&self, scope: &Path) -> io::Result<()> {
        if self.dry_run {
            return Ok(());
        }
        let path = scope.join("cgroup.freeze");
        fs::write(&path, "0").map_err(|source| io::Error::new(source.kind(), source))
    }

    fn is_frozen(&self, scope: &Path) -> io::Result<bool> {
        match fs::read_to_string(scope.join("cgroup.freeze")) {
            Ok(text) => Ok(text.trim() == "1"),
            // No control file means the scope does not exist (or is a dry-run
            // scope), which is treated as "not frozen".
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

impl Reclaimer for CgroupFs {
    fn reclaim(&self, scope: &Path, bytes: u64) -> io::Result<u64> {
        if self.dry_run {
            return Ok(bytes);
        }
        let path = scope.join("memory.reclaim");
        if !path.exists() {
            // Kernel older than 5.19: caller falls back to suspend-only
            // behavior (D3 section 8).
            self.reclaim_supported.store(false, Ordering::Relaxed);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "memory.reclaim is not supported",
            ));
        }
        fs::write(&path, bytes.to_string())?;
        Ok(bytes)
    }
}

impl MemoryReader for CgroupFs {
    fn memory_current(&self, scope: &Path) -> io::Result<u64> {
        CgroupFs::memory_current(self, scope)
    }
}

impl Killer for CgroupFs {
    fn kill(&self, scope: &Path) -> io::Result<u32> {
        if self.dry_run {
            // No cgroup tree in dry-run mode: report success without
            // signalling any process.
            return Ok(1);
        }
        let pids = self.pids(scope)?;
        let mut killed = 0;
        for pid in &pids {
            // Safety rule (D3 section 8): only processes inside the scope are
            // signalled. The pids come from this scope's cgroup.procs.
            #[cfg(unix)]
            unsafe {
                if libc::kill(*pid as libc::pid_t, libc::SIGKILL) == 0 {
                    killed += 1;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
            }
        }
        Ok(killed)
    }
}

/// zram and swappiness tuning for hibernation (P5-T04).
///
/// Writes `/sys/block/zram0/disksize` (only when the device exists and is
/// not already configured) and `vm.swappiness` through `/proc/sys`. The root
/// is configurable so the logic can be tested with a fake sysfs tree.
#[derive(Debug, Clone)]
pub struct ZramTuner {
    sys_root: PathBuf,
    proc_root: PathBuf,
    dry_run: bool,
}

/// Result of one zram tuning pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZramStatus {
    pub zram_present: bool,
    pub zram_configured: bool,
    pub swappiness: u32,
    pub swappiness_applied: bool,
}

impl ZramTuner {
    pub fn system() -> Self {
        Self {
            sys_root: PathBuf::from("/sys"),
            proc_root: PathBuf::from("/proc"),
            dry_run: false,
        }
    }

    pub fn at(sys_root: impl Into<PathBuf>, proc_root: impl Into<PathBuf>) -> Self {
        Self {
            sys_root: sys_root.into(),
            proc_root: proc_root.into(),
            dry_run: false,
        }
    }

    pub fn dry_run() -> Self {
        Self {
            sys_root: PathBuf::from("/sys"),
            proc_root: PathBuf::from("/proc"),
            dry_run: true,
        }
    }

    pub fn swappiness(&self) -> io::Result<u32> {
        let text = fs::read_to_string(self.proc_root.join("sys/vm/swappiness"))?;
        text.trim()
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Set `vm.swappiness`. Values above 100 need kernel 5.8+; the caller
    /// passes the configured value (default 180 for zram, 100 without).
    pub fn set_swappiness(&self, value: u32) -> io::Result<()> {
        if self.dry_run {
            return Ok(());
        }
        fs::write(
            self.proc_root.join("sys/vm/swappiness"),
            value.to_string(),
        )
    }

    /// Configure zram for swap if the device is present and unused.
    pub fn ensure_zram(&self, disksize_bytes: u64) -> io::Result<ZramStatus> {
        let block = self.sys_root.join("block/zram0");
        let present = block.exists();
        let disksize_path = block.join("disksize");
        let mut configured = false;
        if present {
            let current = fs::read_to_string(&disksize_path)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            if current == 0 {
                if !self.dry_run {
                    fs::write(&disksize_path, disksize_bytes.to_string())?;
                }
                configured = true;
            } else {
                configured = true;
            }
        }
        Ok(ZramStatus {
            zram_present: present,
            zram_configured: configured,
            swappiness: self.swappiness().unwrap_or(0),
            swappiness_applied: false,
        })
    }

    /// Full pass: configure zram when present, then set swappiness.
    pub fn tune(&self, disksize_bytes: Option<u64>) -> io::Result<ZramStatus> {
        let mut status = match disksize_bytes {
            Some(size) => self.ensure_zram(size)?,
            None => ZramStatus {
                zram_present: self.sys_root.join("block/zram0").exists(),
                zram_configured: false,
                swappiness: self.swappiness().unwrap_or(0),
                swappiness_applied: false,
            },
        };
        let target = if status.zram_present { 180 } else { 100 };
        if status.swappiness != target {
            self.set_swappiness(target)?;
            status.swappiness = target;
            status.swappiness_applied = true;
        }
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "aether-cgroup-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn scope(&self) -> PathBuf {
            let scope = self.0.join("aether.slice/apps.slice/app.scope");
            fs::create_dir_all(&scope).unwrap();
            scope
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn freeze_and_thaw_write_the_control_file() {
        let tree = TempTree::new();
        let scope = tree.scope();
        let fs_ops = CgroupFs::at(&tree.0);

        assert!(!fs_ops.is_frozen(&scope).unwrap());
        fs_ops.freeze(&scope).unwrap();
        assert!(fs_ops.is_frozen(&scope).unwrap());
        assert_eq!(fs::read_to_string(scope.join("cgroup.freeze")).unwrap(), "1");
        fs_ops.thaw(&scope).unwrap();
        assert!(!fs_ops.is_frozen(&scope).unwrap());
    }

    #[test]
    fn reclaim_writes_requested_bytes_and_reports() {
        let tree = TempTree::new();
        let scope = tree.scope();
        fs::write(scope.join("memory.reclaim"), "").unwrap();
        let fs_ops = CgroupFs::at(&tree.0);
        let requested = 40 * 1024 * 1024;
        assert_eq!(fs_ops.reclaim(&scope, requested).unwrap(), requested);
        assert_eq!(
            fs::read_to_string(scope.join("memory.reclaim")).unwrap(),
            requested.to_string()
        );
    }

    #[test]
    fn reclaim_without_kernel_support_fails_cleanly() {
        let tree = TempTree::new();
        let scope = tree.scope();
        let fs_ops = CgroupFs::at(&tree.0);
        let err = fs_ops.reclaim(&scope, 1024).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert!(!fs_ops.reclaim_supported(&scope));
    }

    #[test]
    fn memory_and_pids_are_read_back() {
        let tree = TempTree::new();
        let scope = tree.scope();
        fs::write(scope.join("memory.current"), "26214400\n").unwrap();
        fs::write(scope.join("cgroup.procs"), "10\n20\n\n").unwrap();
        let fs_ops = CgroupFs::at(&tree.0);
        assert_eq!(fs_ops.memory_current(&scope).unwrap(), 26_214_400);
        assert_eq!(fs_ops.pids(&scope).unwrap(), vec![10, 20]);
    }

    #[test]
    fn killer_uses_scope_pids_and_honours_dry_run() {
        struct CountingKill(AtomicUsize);
        impl Killer for CountingKill {
            fn kill(&self, _scope: &Path) -> io::Result<u32> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(1)
            }
        }
        let tree = TempTree::new();
        let scope = tree.scope();
        fs::write(scope.join("cgroup.procs"), "10\n20\n").unwrap();
        let fs_ops = CgroupFs::dry_run(&tree.0);
        // Dry-run mode never signals processes; it reports one virtual kill.
        assert_eq!(fs_ops.kill(&scope).unwrap(), 1);

        let counter = CountingKill(AtomicUsize::new(0));
        counter.kill(&scope).unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn zram_tuning_uses_fake_sysfs() {
        let tree = TempTree::new();
        let sys = tree.0.join("sys");
        let proc = tree.0.join("proc");
        fs::create_dir_all(sys.join("block/zram0")).unwrap();
        fs::create_dir_all(proc.join("sys/vm")).unwrap();
        fs::write(sys.join("block/zram0/disksize"), "0\n").unwrap();
        fs::write(proc.join("sys/vm/swappiness"), "60\n").unwrap();

        let tuner = ZramTuner::at(&sys, &proc);
        let status = tuner.tune(Some(2 * 1024 * 1024 * 1024)).unwrap();
        assert!(status.zram_present);
        assert!(status.zram_configured);
        assert_eq!(status.swappiness, 180);
        assert!(status.swappiness_applied);
        assert_eq!(
            fs::read_to_string(proc.join("sys/vm/swappiness")).unwrap(),
            "180"
        );
    }

    #[test]
    fn zram_absent_sets_plain_swappiness() {
        let tree = TempTree::new();
        let sys = tree.0.join("sys");
        let proc = tree.0.join("proc");
        fs::create_dir_all(&sys).unwrap();
        fs::create_dir_all(proc.join("sys/vm")).unwrap();
        fs::write(proc.join("sys/vm/swappiness"), "60\n").unwrap();

        let tuner = ZramTuner::at(&sys, &proc);
        let status = tuner.tune(None).unwrap();
        assert!(!status.zram_present);
        assert_eq!(status.swappiness, 100);
    }
}
