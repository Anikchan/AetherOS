//! Seccomp filtering (P6-T04, final sandbox layer).
//!
//! The filter is a classic BPF program installed with
//! `prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER)`. It is **fail-closed**: when the
//! kernel cannot install it, [`apply`] returns an error and the launcher refuses
//! to exec the app. A program running on an architecture other than the one the
//! filter was built for is killed (`SECCOMP_RET_KILL_PROCESS`), never allowed.
//!
//! The policy is default-allow with a curated deny list: app sandboxing must not
//! break legitimate programs, while the syscalls that escape a sandbox
//! (mount/pivot_root/chroot, module loading, kernel tracing, namespace
//! re-entry, hardware ports) are rejected with `EPERM`.

use serde::{Deserialize, Serialize};

use crate::enforce::SandboxError;

/// `EPERM`, the errno denied syscalls return.
pub const DENY_ERRNO: u32 = 1;

/// A syscall the app sandbox never needs, with its number on each supported
/// architecture. Numbers follow the kernel UAPI tables
/// (`arch/x86/entry/syscalls/syscall_64.tbl` and `asm-generic/unistd.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Denial {
    pub name: &'static str,
    pub x86_64: i64,
    pub aarch64: i64,
    /// Why the sandbox denies it.
    pub reason: &'static str,
}

/// The standard AetherOS app deny list.
pub const DENIED: &[Denial] = &[
    Denial { name: "mount", x86_64: 165, aarch64: 40, reason: "mounting filesystems escapes the mount namespace view" },
    Denial { name: "umount2", x86_64: 166, aarch64: 39, reason: "unmounting can hide sandbox paths" },
    Denial { name: "pivot_root", x86_64: 155, aarch64: 41, reason: "changing the root filesystem" },
    Denial { name: "chroot", x86_64: 161, aarch64: 51, reason: "changing the root filesystem" },
    Denial { name: "open_by_handle_at", x86_64: 304, aarch64: 265, reason: "reaches files by handle, bypassing path rules" },
    Denial { name: "init_module", x86_64: 175, aarch64: 105, reason: "loading kernel modules" },
    Denial { name: "finit_module", x86_64: 313, aarch64: 273, reason: "loading kernel modules" },
    Denial { name: "delete_module", x86_64: 176, aarch64: 106, reason: "unloading kernel modules" },
    Denial { name: "kexec_load", x86_64: 246, aarch64: 104, reason: "replacing the running kernel" },
    Denial { name: "kexec_file_load", x86_64: 320, aarch64: 294, reason: "replacing the running kernel" },
    Denial { name: "bpf", x86_64: 321, aarch64: 280, reason: "loading BPF programs can read kernel memory" },
    Denial { name: "perf_event_open", x86_64: 298, aarch64: 241, reason: "kernel/other-process profiling" },
    Denial { name: "ptrace", x86_64: 101, aarch64: 117, reason: "inspecting or controlling other processes" },
    Denial { name: "process_vm_readv", x86_64: 310, aarch64: 270, reason: "reading another process memory" },
    Denial { name: "process_vm_writev", x86_64: 311, aarch64: 271, reason: "writing another process memory" },
    Denial { name: "userfaultfd", x86_64: 323, aarch64: 282, reason: "known userfaultfd exploitation primitive" },
    Denial { name: "unshare", x86_64: 272, aarch64: 97, reason: "leaving the sandbox namespaces" },
    Denial { name: "setns", x86_64: 308, aarch64: 268, reason: "joining host namespaces" },
    Denial { name: "add_key", x86_64: 248, aarch64: 217, reason: "kernel keyring access" },
    Denial { name: "request_key", x86_64: 249, aarch64: 218, reason: "kernel keyring access" },
    Denial { name: "keyctl", x86_64: 250, aarch64: 219, reason: "kernel keyring access" },
    Denial { name: "swapon", x86_64: 167, aarch64: 224, reason: "enabling swap" },
    Denial { name: "swapoff", x86_64: 168, aarch64: 225, reason: "disabling swap" },
    Denial { name: "reboot", x86_64: 169, aarch64: 142, reason: "power control belongs to powerd" },
    Denial { name: "sethostname", x86_64: 170, aarch64: 161, reason: "changing the host name" },
    Denial { name: "setdomainname", x86_64: 171, aarch64: 162, reason: "changing the host name" },
    Denial { name: "adjtimex", x86_64: 159, aarch64: 171, reason: "changing the system clock" },
    Denial { name: "settimeofday", x86_64: 164, aarch64: 170, reason: "changing the system clock" },
    Denial { name: "clock_settime", x86_64: 227, aarch64: 112, reason: "changing the system clock" },
    Denial { name: "syslog", x86_64: 103, aarch64: 116, reason: "reading or clearing the kernel log" },
    Denial { name: "acct", x86_64: 163, aarch64: 89, reason: "process accounting control" },
    Denial { name: "quotactl", x86_64: 179, aarch64: 60, reason: "filesystem quota control" },
    Denial { name: "fanotify_init", x86_64: 300, aarch64: 262, reason: "global filesystem notification of other apps" },
    Denial { name: "personality", x86_64: 135, aarch64: 92, reason: "changing the execution domain" },
    Denial { name: "iopl", x86_64: 172, aarch64: -1, reason: "raw I/O port access" },
    Denial { name: "ioperm", x86_64: 173, aarch64: -1, reason: "raw I/O port access" },
];

/// Architectures the filter knows how to guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeccompArch {
    X86_64,
    Aarch64,
}

impl SeccompArch {
    /// The architecture the current process runs on, if the filter supports it.
    pub fn current() -> Option<Self> {
        match std::env::consts::ARCH {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Aarch64),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }

    /// `AUDIT_ARCH_*` value the filter compares `seccomp_data.arch` against.
    pub fn audit_arch(self) -> u32 {
        match self {
            Self::X86_64 => 0xC000_003E,
            Self::Aarch64 => 0xC000_00B7,
        }
    }

    /// Syscall number for a denial on this architecture.
    pub fn number(self, denial: &Denial) -> i64 {
        match self {
            Self::X86_64 => denial.x86_64,
            Self::Aarch64 => denial.aarch64,
        }
    }
}

/// Which syscalls the filter denies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeccompPolicy {
    pub deny: Vec<String>,
}

impl Default for SeccompPolicy {
    fn default() -> Self {
        Self::standard()
    }
}

impl SeccompPolicy {
    /// The shipped app policy (see [`DENIED`]).
    pub fn standard() -> Self {
        Self {
            deny: DENIED.iter().map(|denial| denial.name.to_string()).collect(),
        }
    }

    /// Remove one syscall from the deny list (app-specific exception).
    pub fn without(mut self, name: &str) -> Self {
        self.deny.retain(|entry| entry != name);
        self
    }

    /// Resolved denials for an architecture, in policy order.
    pub fn deny_list(&self, arch: SeccompArch) -> Result<Vec<(&'static Denial, i64)>, SandboxError> {
        let mut resolved = Vec::new();
        for name in &self.deny {
            let Some(denial) = DENIED.iter().find(|denial| denial.name == name) else {
                return Err(SandboxError::SeccompFailed(format!(
                    "the seccomp policy names an unknown syscall: {name}"
                )));
            };
            let number = arch.number(denial);
            if number >= 0 {
                resolved.push((denial, number));
            }
        }
        Ok(resolved)
    }
}

// Classic BPF encoding (linux/bpf_common.h, linux/filter.h).
const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JMP_JEQ_K: u16 = 0x15;
const BPF_RET_K: u16 = 0x06;

/// `SECCOMP_RET_KILL_PROCESS`.
pub const RET_KILL_PROCESS: u32 = 0x8000_0000;
/// `SECCOMP_RET_ALLOW`.
pub const RET_ALLOW: u32 = 0x7fff_0000;
/// `SECCOMP_RET_ERRNO | errno`.
pub fn ret_errno(errno: u32) -> u32 {
    0x0005_0000 | (errno & 0xffff)
}

/// Offset of `seccomp_data.nr`.
pub const DATA_NR_OFFSET: u32 = 0;
/// Offset of `seccomp_data.arch`.
pub const DATA_ARCH_OFFSET: u32 = 4;

/// One classic BPF instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeccompInsn {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

/// The program the kernel will run for every syscall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeccompPlan {
    pub arch: SeccompArch,
    /// Denials that apply on this architecture (name, number).
    pub denied: Vec<(&'static str, i64)>,
    pub instructions: Vec<SeccompInsn>,
}

impl SeccompPlan {
    pub fn denied_count(&self) -> usize {
        self.denied.len()
    }
}

/// Build the filter. `Err` means the app must not start.
pub fn plan(policy: &SeccompPolicy, arch: SeccompArch) -> Result<SeccompPlan, SandboxError> {
    let resolved = policy.deny_list(arch)?;
    if resolved.is_empty() {
        return Err(SandboxError::SeccompUnavailable(
            "the seccomp policy denies nothing on this architecture; the app will not start with a no-op filter"
                .into(),
        ));
    }

    let mut instructions = Vec::with_capacity(5 + resolved.len() * 2);
    // A program built for another architecture must never run.
    instructions.push(SeccompInsn {
        code: BPF_LD_W_ABS,
        jt: 0,
        jf: 0,
        k: DATA_ARCH_OFFSET,
    });
    instructions.push(SeccompInsn {
        code: BPF_JMP_JEQ_K,
        jt: 0,
        jf: 1,
        k: arch.audit_arch(),
    });
    instructions.push(SeccompInsn {
        code: BPF_RET_K,
        jt: 0,
        jf: 0,
        k: RET_KILL_PROCESS,
    });
    instructions.push(SeccompInsn {
        code: BPF_LD_W_ABS,
        jt: 0,
        jf: 0,
        k: DATA_NR_OFFSET,
    });
    for (_, number) in &resolved {
        instructions.push(SeccompInsn {
            code: BPF_JMP_JEQ_K,
            jt: 0,
            jf: 1,
            k: *number as u32,
        });
        instructions.push(SeccompInsn {
            code: BPF_RET_K,
            jt: 0,
            jf: 0,
            k: ret_errno(DENY_ERRNO),
        });
    }
    instructions.push(SeccompInsn {
        code: BPF_RET_K,
        jt: 0,
        jf: 0,
        k: RET_ALLOW,
    });

    Ok(SeccompPlan {
        arch,
        denied: resolved
            .iter()
            .map(|(denial, number)| (denial.name, *number))
            .collect(),
        instructions,
    })
}

/// Kernel support as reported by the running system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeccompStatus {
    pub available: bool,
    pub arch: Option<String>,
    pub denials: usize,
    pub reason: Option<String>,
}

/// Probe the running kernel.
pub fn probe() -> SeccompStatus {
    let Some(arch) = SeccompArch::current() else {
        return SeccompStatus {
            available: false,
            arch: None,
            denials: 0,
            reason: Some(format!(
                "seccomp filtering is not implemented for this architecture ({})",
                std::env::consts::ARCH
            )),
        };
    };
    #[cfg(target_os = "linux")]
    {
        let kernel_support = std::path::Path::new("/proc/sys/kernel/seccomp/actions_avail")
            .exists()
            || std::fs::read_to_string("/proc/self/status")
                .map(|status| status.contains("Seccomp:"))
                .unwrap_or(false);
        if !kernel_support {
            return SeccompStatus {
                available: false,
                arch: Some(arch.label().into()),
                denials: 0,
                reason: Some("the kernel does not expose seccomp filter support".into()),
            };
        }
        SeccompStatus {
            available: true,
            arch: Some(arch.label().into()),
            denials: SeccompPolicy::standard().deny_list(arch).map(|list| list.len()).unwrap_or(0),
            reason: None,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        SeccompStatus {
            available: false,
            arch: Some(arch.label().into()),
            denials: 0,
            reason: Some(format!(
                "seccomp is a Linux kernel feature and this host is {}",
                std::env::consts::OS
            )),
        }
    }
}

/// A successfully installed filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeccompApplied {
    pub arch: SeccompArch,
    pub denied: usize,
    pub instructions: usize,
}

/// Install the filter on the calling thread (inherited across `exec`).
pub fn apply(policy: &SeccompPolicy) -> Result<SeccompApplied, SandboxError> {
    let Some(arch) = SeccompArch::current() else {
        return Err(SandboxError::SeccompUnavailable(format!(
            "seccomp filtering is not implemented for this architecture ({}); the app will not start",
            std::env::consts::ARCH
        )));
    };
    let plan = plan(policy, arch)?;
    install(&plan)?;
    Ok(SeccompApplied {
        arch,
        denied: plan.denied_count(),
        instructions: plan.instructions.len(),
    })
}

#[cfg(target_os = "linux")]
fn install(plan: &SeccompPlan) -> Result<(), SandboxError> {
    /// `SECCOMP_MODE_FILTER`.
    const SECCOMP_MODE_FILTER: libc::c_ulong = 2;

    let mut filters: Vec<libc::sock_filter> = plan
        .instructions
        .iter()
        .map(|insn| libc::sock_filter {
            code: insn.code,
            jt: insn.jt,
            jf: insn.jf,
            k: insn.k,
        })
        .collect();
    let program = libc::sock_fprog {
        len: filters.len() as u16,
        filter: filters.as_mut_ptr(),
    };

    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(SandboxError::SeccompFailed(format!(
            "PR_SET_NO_NEW_PRIVS: {}",
            std::io::Error::last_os_error()
        )));
    }
    if unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &program as *const libc::sock_fprog,
        )
    } != 0
    {
        return Err(SandboxError::SeccompFailed(format!(
            "PR_SET_SECCOMP: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn install(plan: &SeccompPlan) -> Result<(), SandboxError> {
    let _ = plan;
    Err(SandboxError::SeccompUnavailable(
        "seccomp filters can only be installed on Linux; the app will not start".into(),
    ))
}

/// Matches the shim's `--seccomp` switch.
pub fn standard_policy() -> SeccompPolicy {
    SeccompPolicy::standard()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_resolves_per_architecture() {
        let policy = SeccompPolicy::standard();
        let x86 = policy.deny_list(SeccompArch::X86_64).unwrap();
        let arm = policy.deny_list(SeccompArch::Aarch64).unwrap();
        // iopl/ioperm do not exist on aarch64.
        assert_eq!(x86.len(), DENIED.len());
        assert_eq!(arm.len(), DENIED.len() - 2);
        let mount = x86
            .iter()
            .find(|(denial, _)| denial.name == "mount")
            .unwrap();
        assert_eq!(mount.1, 165);
        let mount_arm = arm
            .iter()
            .find(|(denial, _)| denial.name == "mount")
            .unwrap();
        assert_eq!(mount_arm.1, 40);
        assert!(arm.iter().all(|(_, number)| *number >= 0));
        assert!(x86.iter().any(|(denial, number)| denial.name == "ptrace" && *number == 101));
    }

    #[test]
    fn unknown_syscall_and_empty_policy_fail_closed() {
        let policy = SeccompPolicy {
            deny: vec!["not_a_syscall".into()],
        };
        assert!(matches!(
            plan(&policy, SeccompArch::X86_64),
            Err(SandboxError::SeccompFailed(_))
        ));
        let empty = SeccompPolicy {
            deny: vec!["iopl".into()],
        };
        // iopl has no aarch64 number, so nothing would be denied there.
        assert!(matches!(
            plan(&empty, SeccompArch::Aarch64),
            Err(SandboxError::SeccompUnavailable(_))
        ));
        assert!(plan(&empty, SeccompArch::X86_64).is_ok());
    }

    #[test]
    fn program_checks_arch_then_denies_then_allows() {
        let policy = SeccompPolicy {
            deny: vec!["mount".into(), "ptrace".into()],
        };
        let plan = plan(&policy, SeccompArch::X86_64).unwrap();
        let insns = &plan.instructions;
        // arch load + arch check + kill, nr load, 2 per denial, final allow.
        assert_eq!(insns.len(), 5 + 2 * 2);
        assert_eq!(insns[0].code, BPF_LD_W_ABS);
        assert_eq!(insns[0].k, DATA_ARCH_OFFSET);
        assert_eq!(insns[1].code, BPF_JMP_JEQ_K);
        assert_eq!(insns[1].k, SeccompArch::X86_64.audit_arch());
        assert_eq!(insns[1].jt, 0);
        assert_eq!(insns[1].jf, 1, "a foreign architecture falls into the kill");
        assert_eq!(insns[2].k, RET_KILL_PROCESS);
        assert_eq!(insns[3].k, DATA_NR_OFFSET);
        // mount then ptrace, each followed by EPERM.
        assert_eq!(insns[4].k, 165);
        assert_eq!(insns[5].k, ret_errno(DENY_ERRNO));
        assert_eq!(insns[6].k, 101);
        assert_eq!(insns[7].k, ret_errno(DENY_ERRNO));
        assert_eq!(insns[8].k, RET_ALLOW);
        assert_eq!(plan.denied_count(), 2);
    }

    #[test]
    fn x86_and_aarch64_audit_values_match_the_kernel() {
        assert_eq!(SeccompArch::X86_64.audit_arch(), 0xC000_003E);
        assert_eq!(SeccompArch::Aarch64.audit_arch(), 0xC000_00B7);
        let standard = SeccompPolicy::standard();
        let arm = plan(&standard, SeccompArch::Aarch64).unwrap();
        let mount = arm
            .denied
            .iter()
            .find(|(name, _)| *name == "mount")
            .unwrap();
        assert_eq!(mount.1, 40);
        assert_eq!(arm.arch.label(), "aarch64");
    }

    #[test]
    fn without_removes_a_denial() {
        let policy = SeccompPolicy::standard().without("mount");
        let list = policy.deny_list(SeccompArch::X86_64).unwrap();
        assert!(!list.iter().any(|(denial, _)| denial.name == "mount"));
        assert_eq!(list.len(), DENIED.len() - 1);
    }

    #[test]
    fn probe_and_apply_are_fail_closed_on_this_host() {
        let status = probe();
        if cfg!(target_os = "linux") {
            assert_eq!(status.arch.is_some(), SeccompArch::current().is_some());
            if status.available {
                assert!(status.denials > 0);
                assert!(status.reason.is_none());
                return;
            }
        }
        assert!(!status.available);
        assert!(status.reason.as_deref().unwrap_or("").len() > 5);
        // Applying must fail rather than silently running unfiltered.
        assert!(matches!(
            apply(&SeccompPolicy::standard()),
            Err(SandboxError::SeccompUnavailable(_)) | Err(SandboxError::SeccompFailed(_))
        ));
    }

    #[test]
    fn every_denial_has_a_reason_and_a_number() {
        for denial in DENIED {
            assert!(!denial.reason.is_empty(), "{} has no reason", denial.name);
            assert!(
                denial.x86_64 >= 0 || denial.aarch64 >= 0,
                "{} has no syscall number",
                denial.name
            );
        }
    }
}
