//! Landlock enforcement (P6-T04 follow-up, D8 section 5 failure mode).
//!
//! Landlock is the kernel's unprivileged filesystem access-control LSM. This
//! module turns a [`SandboxProfile`] into a Landlock ruleset, probes the kernel
//! ABI, and applies the ruleset to the calling thread. Enforcement is
//! **fail-closed**: when the kernel does not offer Landlock, or a rule is
//! rejected, [`plan`] / [`apply`] return an error and the caller must refuse to
//! launch instead of exec'ing the app unsandboxed.
//!
//! The ruleset is applied by the `aether-landlock` shim
//! ([`SHIM_PATH`]) inside the bubblewrap namespaces, because Landlock is
//! irreversible for the calling process: the shim applies the rules and then
//! `exec`s the app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::enforce::SandboxError;
use crate::profile::SandboxProfile;

/// Installed location of the Landlock shim on AetherOS.
pub const SHIM_PATH: &str = "/usr/libexec/aether-landlock";

/// Exit code the shim uses when enforcement is impossible (fail-closed).
pub const SHIM_REFUSED_EXIT: i32 = 126;

/// One filesystem access right, in Landlock ABI order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LandlockAccess {
    Execute,
    WriteFile,
    ReadFile,
    ReadDir,
    RemoveDir,
    RemoveFile,
    MakeChar,
    MakeDir,
    MakeReg,
    MakeSock,
    MakeFifo,
    MakeBlock,
    MakeSym,
    Refer,
    Truncate,
    IoctlDev,
}

impl LandlockAccess {
    /// The `landlock_access_fs` bit for this right.
    pub fn bit(self) -> u64 {
        match self {
            Self::Execute => 1 << 0,
            Self::WriteFile => 1 << 1,
            Self::ReadFile => 1 << 2,
            Self::ReadDir => 1 << 3,
            Self::RemoveDir => 1 << 4,
            Self::RemoveFile => 1 << 5,
            Self::MakeChar => 1 << 6,
            Self::MakeDir => 1 << 7,
            Self::MakeReg => 1 << 8,
            Self::MakeSock => 1 << 9,
            Self::MakeFifo => 1 << 10,
            Self::MakeBlock => 1 << 11,
            Self::MakeSym => 1 << 12,
            Self::Refer => 1 << 13,
            Self::Truncate => 1 << 14,
            Self::IoctlDev => 1 << 15,
        }
    }

    /// Landlock ABI version that first accepts this right.
    pub fn since_abi(self) -> u8 {
        match self {
            Self::Refer => 2,
            Self::Truncate => 3,
            Self::IoctlDev => 5,
            _ => 1,
        }
    }

    /// ABI 1 rights.
    pub const ABI1: [Self; 13] = [
        Self::Execute,
        Self::WriteFile,
        Self::ReadFile,
        Self::ReadDir,
        Self::RemoveDir,
        Self::RemoveFile,
        Self::MakeChar,
        Self::MakeDir,
        Self::MakeReg,
        Self::MakeSock,
        Self::MakeFifo,
        Self::MakeBlock,
        Self::MakeSym,
    ];
}

/// Rights for read-only paths: look up, read and execute.
pub const READ_ONLY: [LandlockAccess; 3] = [
    LandlockAccess::Execute,
    LandlockAccess::ReadFile,
    LandlockAccess::ReadDir,
];

/// Rights for the app's own writable folder.
pub const READ_WRITE: [LandlockAccess; 12] = [
    LandlockAccess::Execute,
    LandlockAccess::WriteFile,
    LandlockAccess::ReadFile,
    LandlockAccess::ReadDir,
    LandlockAccess::RemoveDir,
    LandlockAccess::RemoveFile,
    LandlockAccess::MakeDir,
    LandlockAccess::MakeReg,
    LandlockAccess::MakeSock,
    LandlockAccess::MakeFifo,
    LandlockAccess::MakeSym,
    LandlockAccess::Truncate,
];

/// One path with the rights granted beneath it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandlockRule {
    pub path: PathBuf,
    pub access: Vec<LandlockAccess>,
}

impl LandlockRule {
    pub fn read_only(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            access: READ_ONLY.to_vec(),
        }
    }

    pub fn read_write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            access: READ_WRITE.to_vec(),
        }
    }

    pub fn writable(&self) -> bool {
        self.access.contains(&LandlockAccess::WriteFile)
    }

    fn masked(&self, abi: u8) -> u64 {
        self.access
            .iter()
            .filter(|access| access.since_abi() <= abi)
            .fold(0, |mask, access| mask | access.bit())
    }
}

/// Collapse duplicate paths and keep the widest grant.
///
/// The profile builder and the shim parser both go through this, so a path that
/// is listed read-only *and* read-write ends up read-write in the arguments the
/// launcher emits and in the ruleset the shim applies. Without it, the answer
/// depended on which rule came first.
fn normalize(rules: &mut Vec<LandlockRule>) {
    rules.sort_by(|a, b| a.path.cmp(&b.path));
    // `dedup_by` hands the later element first and the kept one second; the
    // widest grant wins and the duplicate is dropped.
    rules.dedup_by(|later, kept| {
        if later.path == kept.path {
            if later.writable() {
                kept.access = READ_WRITE.to_vec();
            }
            true
        } else {
            false
        }
    });
}

/// The ruleset derived from a sandbox profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LandlockPolicy {
    pub rules: Vec<LandlockRule>,
}

impl LandlockPolicy {
    /// Read-only system paths and devices, read/write app data.
    pub fn from_profile(profile: &SandboxProfile) -> Self {
        let mut rules = Vec::new();
        for path in &profile.file_read {
            rules.push(LandlockRule::read_only(path.clone()));
        }
        for path in &profile.file_write {
            rules.push(LandlockRule::read_write(path.clone()));
        }
        for device in &profile.devices {
            rules.push(LandlockRule::read_only(device.clone()));
        }
        // A path listed read-only and writable stays writable, and duplicates
        // are collapsed, so the shim receives exactly one rule per path.
        normalize(&mut rules);
        Self { rules }
    }

    /// Every right requested by any rule (unmasked).
    pub fn handled_mask_any(&self) -> u64 {
        self.rules
            .iter()
            .flat_map(|rule| rule.access.iter())
            .fold(0, |mask, access| mask | access.bit())
    }

    /// Handled rights supported by `abi`.
    pub fn handled_mask(&self, abi: u8) -> u64 {
        self.rules
            .iter()
            .flat_map(|rule| rule.access.iter())
            .filter(|access| access.since_abi() <= abi)
            .fold(0, |mask, access| mask | access.bit())
    }

    /// Rules with the access mask masked to `abi`, sorted by path.
    pub fn rules_for_abi(&self, abi: u8) -> Vec<(PathBuf, u64)> {
        let mut rules: Vec<(PathBuf, u64)> = self
            .rules
            .iter()
            .map(|rule| (rule.path.clone(), rule.masked(abi)))
            .filter(|(_, mask)| *mask != 0)
            .collect();
        rules.sort_by(|a, b| a.0.cmp(&b.0));
        rules
    }
}

/// What the kernel reports for Landlock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum LandlockStatus {
    Available { abi: u8 },
    Unavailable { reason: String },
}

impl LandlockStatus {
    pub fn abi(&self) -> Option<u8> {
        match self {
            Self::Available { abi } => Some(*abi),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Available { .. } => None,
            Self::Unavailable { reason } => Some(reason),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// A ruleset ready to hand to the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandlockPlan {
    pub abi: u8,
    pub handled: u64,
    pub rules: Vec<(PathBuf, u64)>,
}

impl LandlockPlan {
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// Fail-closed planning: `Err` means the app must not start.
pub fn plan(status: &LandlockStatus, policy: &LandlockPolicy) -> Result<LandlockPlan, SandboxError> {
    let abi = match status {
        LandlockStatus::Available { abi } if *abi >= 1 => *abi,
        LandlockStatus::Available { abi } => {
            return Err(SandboxError::LandlockUnavailable(format!(
                "the kernel reported Landlock ABI {abi}, which cannot enforce a ruleset; the app will not start unsandboxed"
            )));
        }
        LandlockStatus::Unavailable { reason } => {
            return Err(SandboxError::LandlockUnavailable(format!(
                "{reason}; the app will not start unsandboxed"
            )));
        }
    };
    let rules = policy.rules_for_abi(abi);
    if rules.is_empty() {
        return Err(SandboxError::LandlockUnavailable(
            "the sandbox profile grants no filesystem paths, so Landlock would deny every access; the app will not start"
                .into(),
        ));
    }
    let handled = policy.handled_mask(abi);
    if handled == 0 {
        return Err(SandboxError::LandlockUnavailable(
            "none of the requested rights are supported by this Landlock ABI; the app will not start".into(),
        ));
    }
    Ok(LandlockPlan {
        abi,
        handled,
        rules,
    })
}

/// Probe the running kernel.
pub fn probe() -> LandlockStatus {
    #[cfg(target_os = "linux")]
    {
        sys::probe()
    }
    #[cfg(not(target_os = "linux"))]
    {
        LandlockStatus::Unavailable {
            reason: format!(
                "Landlock is a Linux kernel feature and this host is {}",
                std::env::consts::OS
            ),
        }
    }
}

/// Result of a successful enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandlockApplied {
    pub abi: u8,
    pub handled: u64,
    pub rules: usize,
}

/// Apply the policy to the calling thread (irreversible).
pub fn apply(policy: &LandlockPolicy) -> Result<LandlockApplied, SandboxError> {
    let plan = plan(&probe(), policy)?;
    apply_plan(&plan)
}

fn apply_plan(plan: &LandlockPlan) -> Result<LandlockApplied, SandboxError> {
    #[cfg(target_os = "linux")]
    {
        sys::apply(plan)?;
        Ok(LandlockApplied {
            abi: plan.abi,
            handled: plan.handled,
            rules: plan.rule_count(),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = plan;
        Err(SandboxError::LandlockUnavailable(
            "Landlock can only be applied on Linux".into(),
        ))
    }
}

/// Parsed `aether-landlock` invocation (the shim reuses this parser).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimInvocation {
    pub policy: LandlockPolicy,
    /// The shim also installs the standard seccomp filter before exec.
    pub seccomp: bool,
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Parse shim arguments: `[--ro PATH]... [--rw PATH]... -- PROGRAM [ARGS]...`.
pub fn parse_shim_args(args: &[String]) -> Result<ShimInvocation, String> {
    let mut policy = LandlockPolicy::default();
    let mut seccomp = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--seccomp" => {
                seccomp = true;
                index += 1;
            }
            "--ro" | "--rw" => {
                let read_write = args[index] == "--rw";
                let Some(value) = args.get(index + 1) else {
                    return Err(format!("{} needs a path", args[index]));
                };
                let path = PathBuf::from(value);
                if path.as_os_str().is_empty() {
                    return Err("empty sandbox path".into());
                }
                policy.rules.push(if read_write {
                    LandlockRule::read_write(path)
                } else {
                    LandlockRule::read_only(path)
                });
                index += 2;
            }
            "--" => {
                let program = args
                    .get(index + 1)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "-- needs a program to run".to_string())?;
                // Same normalization as the launcher side: one rule per path,
                // and read-write wins over read-only whatever the order.
                normalize(&mut policy.rules);
                return Ok(ShimInvocation {
                    policy,
                    seccomp,
                    program: PathBuf::from(program),
                    args: args[index + 2..].to_vec(),
                });
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Err("missing -- PROGRAM: the shim only ever runs a program it can sandbox".into())
}

/// Build the shim argument list for a plan.
///
/// The list is normalized first, so a hand-built policy with duplicate paths or
/// a path granted both ways still produces the one canonical rule the shim
/// expects (and read-write wins).
pub fn shim_arguments(policy: &LandlockPolicy, program: &Path) -> Vec<String> {
    let mut policy = policy.clone();
    normalize(&mut policy.rules);
    let mut args = Vec::new();
    for rule in &policy.rules {
        args.push(if rule.writable() { "--rw" } else { "--ro" }.to_string());
        args.push(rule.path.display().to_string());
    }
    args.push("--".into());
    args.push(program.display().to_string());
    args
}

#[cfg(target_os = "linux")]
mod sys {
    //! Raw Landlock syscalls. The numbers are the generic table values 444/445/446
    //! used on every architecture AetherOS supports; they match
    //! `libc::SYS_landlock_create_ruleset` and friends in libc 0.2.191, but are
    //! spelled out because the dependency allows older 0.2.x releases that
    //! predate those constants.

    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use super::{LandlockPlan, LandlockStatus};

    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
    /// `LANDLOCK_CREATE_RULESET_VERSION`.
    const CREATE_RULESET_VERSION: u32 = 1;
    /// `LANDLOCK_RULE_PATH_BENEATH`.
    const RULE_PATH_BENEATH: u32 = 1;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
        /// Explicit padding: the kernel copies `sizeof(struct)` bytes, so the
        /// four bytes between the fd and the end must be initialized rather
        /// than left as whatever the stack held.
        _pad: u32,
    }

    pub fn probe() -> LandlockStatus {
        let ret = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                CREATE_RULESET_VERSION,
            )
        };
        if ret < 0 {
            return LandlockStatus::Unavailable {
                reason: format!("landlock_create_ruleset failed: {}", io::Error::last_os_error()),
            };
        }
        LandlockStatus::Available { abi: ret as u8 }
    }

    pub fn apply(plan: &LandlockPlan) -> Result<(), crate::enforce::SandboxError> {
        let attr = RulesetAttr {
            handled_access_fs: plan.handled,
        };
        let fd = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                &attr as *const RulesetAttr,
                std::mem::size_of::<RulesetAttr>(),
                0u32,
            )
        };
        if fd < 0 {
            return Err(crate::enforce::SandboxError::LandlockFailed(format!(
                "landlock_create_ruleset: {}",
                io::Error::last_os_error()
            )));
        }
        let ruleset = fd as i32;

        for (path, access) in &plan.rules {
            let opened = open_path(path);
            let Ok(parent_fd) = opened else {
                // Read errno before `close`, which could clobber it.
                let reason = io::Error::last_os_error().to_string();
                unsafe { libc::close(ruleset) };
                return Err(crate::enforce::SandboxError::LandlockRule {
                    path: path.display().to_string(),
                    reason,
                });
            };
            let rule = PathBeneathAttr {
                allowed_access: *access,
                parent_fd,
                _pad: 0,
            };
            let ret = unsafe {
                libc::syscall(
                    SYS_LANDLOCK_ADD_RULE,
                    ruleset,
                    RULE_PATH_BENEATH,
                    &rule as *const PathBeneathAttr,
                    0u32,
                )
            };
            unsafe { libc::close(parent_fd) };
            if ret < 0 {
                let reason = io::Error::last_os_error().to_string();
                unsafe { libc::close(ruleset) };
                return Err(crate::enforce::SandboxError::LandlockRule {
                    path: path.display().to_string(),
                    reason,
                });
            }
        }

        // Landlock requires no_new_privs before the ruleset can be enforced.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            let reason = io::Error::last_os_error().to_string();
            unsafe { libc::close(ruleset) };
            return Err(crate::enforce::SandboxError::LandlockFailed(format!(
                "PR_SET_NO_NEW_PRIVS: {reason}"
            )));
        }
        let ret = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset, 0u32) };
        let reason = if ret < 0 {
            Some(io::Error::last_os_error().to_string())
        } else {
            None
        };
        unsafe { libc::close(ruleset) };
        match reason {
            Some(reason) => Err(crate::enforce::SandboxError::LandlockFailed(format!(
                "landlock_restrict_self: {reason}"
            ))),
            None => Ok(()),
        }
    }

    fn open_path(path: &Path) -> io::Result<i32> {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> SandboxProfile {
        SandboxProfile {
            file_read: vec![PathBuf::from("/usr"), PathBuf::from("/etc")],
            file_write: vec![PathBuf::from("/data/app")],
            network: false,
            devices: vec![PathBuf::from("/dev/video0")],
            home: PathBuf::from("/data/app"),
        }
    }

    #[test]
    fn policy_follows_the_granted_profile() {
        let policy = LandlockPolicy::from_profile(&profile());
        assert_eq!(policy.rules.len(), 4);
        let paths: Vec<_> = policy
            .rules
            .iter()
            .map(|rule| rule.path.display().to_string())
            .collect();
        assert_eq!(paths, vec!["/data/app", "/dev/video0", "/etc", "/usr"]);
        let app = policy
            .rules
            .iter()
            .find(|rule| rule.path == Path::new("/data/app"))
            .unwrap();
        assert!(app.writable());
        assert!(app.access.contains(&LandlockAccess::Truncate));
        let usr = policy
            .rules
            .iter()
            .find(|rule| rule.path == Path::new("/usr"))
            .unwrap();
        assert!(!usr.writable());
        assert_eq!(usr.access, READ_ONLY.to_vec());
    }

    #[test]
    fn read_write_wins_when_a_path_is_listed_twice() {
        let mut profile = profile();
        profile.file_read.push(PathBuf::from("/data/app"));
        let policy = LandlockPolicy::from_profile(&profile);
        let app = policy
            .rules
            .iter()
            .find(|rule| rule.path == Path::new("/data/app"))
            .unwrap();
        assert!(app.writable());
    }

    #[test]
    fn normalize_keeps_the_widest_grant_in_any_order() {
        for writable_first in [false, true] {
            let mut rules = if writable_first {
                vec![
                    LandlockRule::read_write("/data/app"),
                    LandlockRule::read_only("/data/app"),
                ]
            } else {
                vec![
                    LandlockRule::read_only("/data/app"),
                    LandlockRule::read_write("/data/app"),
                ]
            };
            rules.push(LandlockRule::read_only("/usr"));
            rules.push(LandlockRule::read_only("/usr"));
            normalize(&mut rules);
            assert_eq!(rules.len(), 2, "duplicates are collapsed");
            assert_eq!(rules[0].path, PathBuf::from("/data/app"));
            assert!(rules[0].writable(), "read-write wins (order {writable_first})");
            assert_eq!(rules[0].access, READ_WRITE.to_vec());
            assert_eq!(rules[1].path, PathBuf::from("/usr"));
            assert!(!rules[1].writable());
        }
    }

    #[test]
    fn shim_parser_widens_and_collapses_like_the_profile_builder() {
        for order in [
            vec!["--ro", "/data/app", "--rw", "/data/app"],
            vec!["--rw", "/data/app", "--ro", "/data/app"],
            vec!["--ro", "/data/app", "--ro", "/data/app", "--rw", "/data/app"],
        ] {
            let mut args: Vec<String> = order.iter().map(|arg| arg.to_string()).collect();
            args.extend(["--".to_string(), "/bin/app".to_string()]);
            let invocation =
                parse_shim_args(&args).unwrap_or_else(|error| panic!("{order:?}: {error}"));
            assert_eq!(invocation.policy.rules.len(), 1, "{order:?}");
            assert!(invocation.policy.rules[0].writable(), "{order:?}");
            assert_eq!(invocation.policy.rules[0].access, READ_WRITE.to_vec());
        }

        let args: Vec<String> = [
            "--ro", "/usr", "--ro", "/usr", "--ro", "/etc", "--ro", "/usr", "--", "/bin/app",
        ]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
        let invocation = parse_shim_args(&args).unwrap();
        let paths: Vec<String> = invocation
            .policy
            .rules
            .iter()
            .map(|rule| rule.path.display().to_string())
            .collect();
        assert_eq!(paths, vec!["/etc", "/usr"]);
    }

    #[test]
    fn shim_arguments_are_normalized_even_for_a_hand_built_policy() {
        let policy = LandlockPolicy {
            rules: vec![
                LandlockRule::read_only("/usr"),
                LandlockRule::read_only("/data/app"),
                LandlockRule::read_write("/data/app"),
                LandlockRule::read_only("/usr"),
            ],
        };
        let args = shim_arguments(&policy, Path::new("/bin/app"));
        assert_eq!(args, vec!["--rw", "/data/app", "--ro", "/usr", "--", "/bin/app"]);
    }

    #[test]
    fn a_rule_for_a_missing_path_is_still_emitted() {
        // Documented choice: neither the profile nor `shim_arguments` filters on
        // existence. bwrap's binds skip missing sources (bwrap rejects them),
        // but the Landlock rule is kept so the launcher and the shim agree on
        // one ruleset. The shim then fails closed when it cannot open the path
        // (`LandlockRule`, exit 126) instead of starting the app with a
        // silently different rule set.
        let missing = PathBuf::from("/nonexistent/aether/sandbox/path");
        let profile = SandboxProfile {
            file_read: vec![missing.clone()],
            file_write: Vec::new(),
            network: false,
            devices: Vec::new(),
            home: PathBuf::from("/data/app"),
        };
        let policy = LandlockPolicy::from_profile(&profile);
        assert_eq!(policy.rules.len(), 1);
        assert_eq!(policy.rules[0].path, missing);
        assert_eq!(
            shim_arguments(&policy, Path::new("/bin/app")),
            vec!["--ro", "/nonexistent/aether/sandbox/path", "--", "/bin/app"]
        );
        let plan = plan(&LandlockStatus::Available { abi: 3 }, &policy).unwrap();
        assert_eq!(plan.rule_count(), 1, "the plan does not drop missing paths");
    }

    #[test]
    fn handled_rights_are_masked_by_abi() {
        let policy = LandlockPolicy::from_profile(&profile());
        // ABI 1: no REFER/TRUNCATE/IOCTL_DEV.
        let abi1 = policy.handled_mask(1);
        assert_eq!(abi1 & LandlockAccess::Truncate.bit(), 0);
        assert_eq!(abi1 & LandlockAccess::ReadFile.bit(), LandlockAccess::ReadFile.bit());
        // ABI 3 adds TRUNCATE for the writable folder.
        let abi3 = policy.handled_mask(3);
        assert_ne!(abi3 & LandlockAccess::Truncate.bit(), 0);
        assert_eq!(abi3 & LandlockAccess::IoctlDev.bit(), 0);
        // ABI 5 adds IOCTL_DEV when a rule asks for it.
        let mut with_ioctl = policy.clone();
        with_ioctl.rules[0].access.push(LandlockAccess::IoctlDev);
        assert_ne!(with_ioctl.handled_mask(5) & LandlockAccess::IoctlDev.bit(), 0);
        assert_eq!(with_ioctl.handled_mask(4) & LandlockAccess::IoctlDev.bit(), 0);

        let rules = policy.rules_for_abi(1);
        let app = rules
            .iter()
            .find(|(path, _)| path == Path::new("/data/app"))
            .unwrap();
        assert_eq!(app.1 & LandlockAccess::Truncate.bit(), 0);
        assert_eq!(app.1 & LandlockAccess::MakeReg.bit(), LandlockAccess::MakeReg.bit());
    }

    #[test]
    fn unavailable_kernel_fails_closed() {
        let policy = LandlockPolicy::from_profile(&profile());
        let status = LandlockStatus::Unavailable {
            reason: "no landlock in /sys/kernel/security/lsm".into(),
        };
        match plan(&status, &policy) {
            Err(SandboxError::LandlockUnavailable(message)) => {
                assert!(message.contains("no landlock"));
                assert!(message.contains("will not start unsandboxed"));
            }
            other => panic!("expected a fail-closed error, got {other:?}"),
        }
    }

    #[test]
    fn zero_abi_fails_closed() {
        let policy = LandlockPolicy::from_profile(&profile());
        let status = LandlockStatus::Available { abi: 0 };
        assert!(matches!(
            plan(&status, &policy),
            Err(SandboxError::LandlockUnavailable(_))
        ));
    }

    #[test]
    fn empty_policy_fails_closed() {
        let status = LandlockStatus::Available { abi: 3 };
        assert!(matches!(
            plan(&status, &LandlockPolicy::default()),
            Err(SandboxError::LandlockUnavailable(_))
        ));
    }

    #[test]
    fn available_kernel_yields_a_plan() {
        let policy = LandlockPolicy::from_profile(&profile());
        let plan = plan(&LandlockStatus::Available { abi: 3 }, &policy).unwrap();
        assert_eq!(plan.abi, 3);
        assert_eq!(plan.rule_count(), 4);
        assert_ne!(plan.handled, 0);
    }

    #[test]
    fn shim_arguments_round_trip() {
        let policy = LandlockPolicy::from_profile(&profile());
        let mut args = shim_arguments(&policy, Path::new("/data/app/bin/app"));
        args.extend(["--flag".to_string(), "value".to_string()]);
        let invocation = parse_shim_args(&args).unwrap();
        assert!(!invocation.seccomp, "seccomp is opt-in per invocation");
        assert_eq!(invocation.program, PathBuf::from("/data/app/bin/app"));
        assert_eq!(invocation.args, vec!["--flag".to_string(), "value".to_string()]);
        assert_eq!(invocation.policy.rules.len(), 4);
        assert!(invocation
            .policy
            .rules
            .iter()
            .any(|rule| rule.path == Path::new("/data/app") && rule.writable()));
        assert!(invocation
            .policy
            .rules
            .iter()
            .any(|rule| rule.path == Path::new("/usr") && !rule.writable()));
    }

    #[test]
    fn shim_parser_reads_the_seccomp_switch() {
        let mut args = vec!["--seccomp".to_string()];
        args.extend(shim_arguments(
            &LandlockPolicy::from_profile(&profile()),
            Path::new("/data/app/bin/app"),
        ));
        let invocation = parse_shim_args(&args).unwrap();
        assert!(invocation.seccomp);
        assert_eq!(invocation.policy.rules.len(), 4);
        // The flag may also follow the rules.
        let mut args = shim_arguments(
            &LandlockPolicy::from_profile(&profile()),
            Path::new("/data/app/bin/app"),
        );
        args.insert(0, "--seccomp".to_string());
        assert!(parse_shim_args(&args).unwrap().seccomp);
    }

    #[test]
    fn shim_parser_rejects_incomplete_invocations() {
        assert!(parse_shim_args(&[]).is_err());
        assert!(parse_shim_args(&["--ro".to_string()]).is_err());
        assert!(parse_shim_args(&["--bogus".to_string(), "x".into()]).is_err());
        assert!(parse_shim_args(&["--ro".to_string(), "/usr".into()]).is_err());
        assert!(parse_shim_args(&["--".to_string()]).is_err());
    }

    #[test]
    fn probe_never_panics_on_this_host() {
        let status = probe();
        match &status {
            LandlockStatus::Available { abi } => assert!(*abi >= 1),
            LandlockStatus::Unavailable { reason } => assert!(!reason.is_empty()),
        }
        // The plan gate agrees with the live probe.
        let policy = LandlockPolicy::from_profile(&profile());
        assert_eq!(plan(&status, &policy).is_ok(), status.is_available());
    }
}
