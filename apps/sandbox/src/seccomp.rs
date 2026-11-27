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

/// Classic BPF jumps are relative to the instruction after the jump.
fn jump_target(pc: usize, insn: &SeccompInsn, taken: bool) -> usize {
    pc + 1 + usize::from(if taken { insn.jt } else { insn.jf })
}

/// Fetch the instruction a branch must land on, or explain that it left the
/// program (the kernel rejects such a program as well).
fn instruction<'a>(
    insns: &'a [SeccompInsn],
    index: usize,
    origin: &str,
) -> Result<&'a SeccompInsn, String> {
    insns.get(index).ok_or_else(|| {
        format!(
            "{origin} jumps to instruction {index}, past the end of the {} instruction program",
            insns.len()
        )
    })
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

    /// Check the whole program: every instruction is one this crate knows how
    /// to encode, and every conditional jump lands where the builder intended.
    ///
    /// [`plan`] runs this before returning, so an encoding mistake (the class of
    /// bug that shipped an inverted arch check and killed every syscall of every
    /// app) fails the launch with an error on **every** host, including one that
    /// cannot install a seccomp filter. The check is structural rather than a
    /// copy of the builder's index arithmetic: it starts from the documented
    /// header, then follows each comparison's taken and fall-through targets.
    pub fn verify_encoding(&self) -> Result<(), String> {
        let insns = &self.instructions;
        let expected = 5 + self.denied.len() * 2;
        if insns.len() != expected {
            return Err(format!(
                "the program has {} instructions, expected {expected} (5 header/footer plus 2 per denial)",
                insns.len()
            ));
        }
        for (index, insn) in insns.iter().enumerate() {
            if !matches!(insn.code, BPF_LD_W_ABS | BPF_JMP_JEQ_K | BPF_RET_K) {
                return Err(format!(
                    "instruction {index} uses the opcode {:#x}, which this crate does not emit",
                    insn.code
                ));
            }
        }

        // Header: load arch, compare it, kill when it differs, load nr.
        let load_arch = instruction(insns, 0, "the arch load")?;
        if load_arch.code != BPF_LD_W_ABS || load_arch.k != DATA_ARCH_OFFSET {
            return Err("the program does not start by loading seccomp_data.arch".into());
        }
        let arch_jump = instruction(insns, 1, "the arch check")?;
        if arch_jump.code != BPF_JMP_JEQ_K || arch_jump.k != self.arch.audit_arch() {
            return Err(format!(
                "the arch check does not compare against {:#x}",
                self.arch.audit_arch()
            ));
        }
        let matched = instruction(
            insns,
            jump_target(1, arch_jump, true),
            "the arch check's taken branch",
        )?;
        if matched.code != BPF_LD_W_ABS || matched.k != DATA_NR_OFFSET {
            return Err("a matching architecture must reach the seccomp_data.nr load".into());
        }
        let foreign = instruction(
            insns,
            jump_target(1, arch_jump, false),
            "the arch check's fall-through",
        )?;
        if foreign.code != BPF_RET_K || foreign.k != RET_KILL_PROCESS {
            return Err("a foreign architecture must fall into RET_KILL_PROCESS".into());
        }

        for (position, (name, number)) in self.denied.iter().enumerate() {
            let base = 4 + position * 2;
            let comparison = instruction(insns, base, name)?;
            if comparison.code != BPF_JMP_JEQ_K {
                return Err(format!("the denial of {name} is not a comparison"));
            }
            if comparison.k != *number as u32 {
                return Err(format!(
                    "the denial of {name} compares against {}, but its table entry is {number}",
                    comparison.k
                ));
            }
            if jump_target(base, comparison, true) != base + 1 {
                return Err(format!(
                    "the denial of {name} does not land on its own EPERM return"
                ));
            }
            if jump_target(base, comparison, false) != base + 2 {
                return Err(format!(
                    "the denial of {name} does not fall through to the next instruction"
                ));
            }
            let denied = instruction(insns, jump_target(base, comparison, true), name)?;
            if denied.code != BPF_RET_K || denied.k != ret_errno(DENY_ERRNO) {
                return Err(format!("{name} does not return EPERM when it matches"));
            }
            let next = instruction(insns, jump_target(base, comparison, false), name)?;
            if position + 1 == self.denied.len() {
                if next.code != BPF_RET_K || next.k != RET_ALLOW {
                    return Err(format!(
                        "a syscall outside the deny list (checked after {name}) must be allowed"
                    ));
                }
            } else if next.code != BPF_JMP_JEQ_K {
                return Err(format!(
                    "the denial of {name} does not continue to the next comparison"
                ));
            }
        }
        Ok(())
    }

    /// Run the program the way the kernel would, for one syscall.
    ///
    /// The kernel executes the classic-BPF subset this crate emits; evaluating
    /// it here keeps the encoding testable on any host, including the ones
    /// without a usable seccomp/Landlock kernel. Returns the `SECCOMP_RET_*`
    /// action (`RET_ALLOW`, `RET_KILL_PROCESS`, or a `ret_errno` value).
    pub fn evaluate(&self, audit_arch: u32, syscall: u32) -> u32 {
        let mut accumulator: u32 = 0;
        let mut pc = 0usize;
        // Bounded walk: a malformed program must not loop forever in a test.
        for _ in 0..self.instructions.len() + 1 {
            let Some(insn) = self.instructions.get(pc) else {
                return RET_KILL_PROCESS;
            };
            match insn.code {
                BPF_LD_W_ABS => {
                    accumulator = match insn.k {
                        DATA_ARCH_OFFSET => audit_arch,
                        DATA_NR_OFFSET => syscall,
                        _ => return RET_KILL_PROCESS,
                    };
                    pc += 1;
                }
                BPF_JMP_JEQ_K => {
                    let taken = accumulator == insn.k;
                    pc += 1 + if taken { insn.jt as usize } else { insn.jf as usize };
                }
                BPF_RET_K => return insn.k,
                _ => return RET_KILL_PROCESS,
            }
        }
        RET_KILL_PROCESS
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
    // Classic BPF jumps are relative to the *next* instruction: `jt` is the
    // number of instructions to skip when the comparison holds. A matching
    // architecture must therefore skip the kill (jt = 1, over RET_KILL) and a
    // mismatching one must fall into it (jf = 0). Getting these the other way
    // round kills every syscall of the app, which is what the Dell caught.
    instructions.push(SeccompInsn {
        code: BPF_JMP_JEQ_K,
        jt: 1,
        jf: 0,
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

    let plan = SeccompPlan {
        arch,
        denied: resolved
            .iter()
            .map(|(denial, number)| (denial.name, *number))
            .collect(),
        instructions,
    };
    // Never hand the kernel a program whose branches do not land where the
    // builder intended: the arch-inversion bug was invisible to a compile test
    // and only showed up as every syscall being killed on the target kernel.
    plan.verify_encoding().map_err(|error| {
        SandboxError::SeccompFailed(format!(
            "the generated seccomp program is inconsistent ({error}); the app will not start"
        ))
    })?;
    Ok(plan)
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
        // Both markers are optimistic: `/proc/self/status` always carries a
        // `Seccomp:` line and `actions_avail` implies filter support. That is
        // deliberate — the authoritative check is the `prctl(PR_SET_SECCOMP)`
        // in `install`, and the shim refuses to exec when it fails. A false
        // negative here would only block launches the kernel could enforce.
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
    fn evaluated_filter_allows_innocuous_syscalls_and_denies_listed_ones() {
        // The kernel semantics of the emitted program, evaluated here so a
        // wrong jump offset fails on every host instead of only on a Linux
        // kernel that runs the filter (the Dell caught an inverted arch check
        // that killed the app on its first syscall).
        let plan = plan(&standard_policy(), SeccompArch::X86_64).unwrap();
        let arch = SeccompArch::X86_64.audit_arch();
        assert_eq!(plan.evaluate(arch, 1), RET_ALLOW, "write must be allowed");
        assert_eq!(plan.evaluate(arch, 59), RET_ALLOW, "execve must be allowed");
        let mount = plan
            .denied
            .iter()
            .find(|(name, _)| *name == "mount")
            .map(|(_, number)| *number as u32)
            .expect("mount is in the standard policy");
        assert_eq!(plan.evaluate(arch, mount), ret_errno(DENY_ERRNO));
        assert_eq!(
            plan.evaluate(SeccompArch::Aarch64.audit_arch(), 1),
            RET_KILL_PROCESS,
            "a program built for another architecture must not run"
        );
    }

    #[test]
    fn arch_check_skips_the_kill_for_the_right_architecture() {
        let plan = plan(&standard_policy(), SeccompArch::X86_64).unwrap();
        // LD arch, JEQ arch, RET kill, LD nr, ...
        assert_eq!(plan.instructions[1].code, BPF_JMP_JEQ_K);
        assert_eq!(plan.instructions[1].jt, 1, "match skips the kill");
        assert_eq!(plan.instructions[1].jf, 0, "mismatch falls into the kill");
        assert_eq!(plan.instructions[2].k, RET_KILL_PROCESS);
    }

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
        // Classic BPF offsets are relative to the next instruction: a matching
        // architecture skips the kill, a foreign one falls into it.
        assert_eq!(insns[1].jt, 1, "a matching architecture skips the kill");
        assert_eq!(insns[1].jf, 0, "a foreign architecture falls into the kill");
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

    /// Syscalls an app must always be able to make, so a filter that kills or
    /// denies them breaks every app (the Dell's inverted arch check did exactly
    /// that). Numbers from the kernel UAPI tables.
    fn innocuous(arch: SeccompArch) -> Vec<u32> {
        let numbers: &[i64] = match arch {
            SeccompArch::X86_64 => &[
                0, 1, 2, 3, 5, 8, 9, 10, 11, 12, 13, 16, 17, 20, 35, 39, 41, 42, 56, 57, 59, 60,
                61, 62, 63, 72, 79, 80, 202, 228, 231, 257, 262, 302, 318,
            ],
            SeccompArch::Aarch64 => &[
                17, 23, 25, 29, 49, 56, 57, 59, 62, 63, 64, 66, 67, 79, 80, 93, 94, 98, 101, 113,
                129, 134, 160, 172, 198, 203, 214, 215, 220, 221, 222, 226, 261, 278,
            ],
        };
        numbers.iter().map(|number| *number as u32).collect()
    }

    #[test]
    fn every_denied_syscall_is_denied_and_innocuous_ones_are_allowed() {
        for arch in [SeccompArch::X86_64, SeccompArch::Aarch64] {
            let plan = plan(&standard_policy(), arch).unwrap();
            let audit = arch.audit_arch();
            // The plan is the table resolved for this architecture: nothing is
            // silently dropped and nothing is invented.
            let expected: Vec<(&'static str, i64)> = DENIED
                .iter()
                .filter(|denial| arch.number(denial) >= 0)
                .map(|denial| (denial.name, arch.number(denial)))
                .collect();
            assert_eq!(plan.denied, expected, "{} resolves the table", arch.label());

            for (name, number) in &plan.denied {
                assert_eq!(
                    plan.evaluate(audit, *number as u32),
                    ret_errno(DENY_ERRNO),
                    "{name} (#{number}) must be denied on {}",
                    arch.label()
                );
            }
            for number in innocuous(arch) {
                assert!(
                    !plan.denied.iter().any(|(_, denied)| *denied as u32 == number),
                    "syscall {number} is innocuous and must not be in the deny list"
                );
                assert_eq!(
                    plan.evaluate(audit, number),
                    RET_ALLOW,
                    "innocuous syscall {number} must be allowed on {}",
                    arch.label()
                );
            }
        }
    }

    #[test]
    fn a_foreign_architecture_kills_every_syscall() {
        // AUDIT_ARCH_I386: the compat personality. Not a filter target, so it
        // must be killed rather than allowed.
        const AUDIT_ARCH_I386: u32 = 0x4000_0003;
        for arch in [SeccompArch::X86_64, SeccompArch::Aarch64] {
            let plan = plan(&standard_policy(), arch).unwrap();
            let foreign = match arch {
                SeccompArch::X86_64 => SeccompArch::Aarch64.audit_arch(),
                SeccompArch::Aarch64 => SeccompArch::X86_64.audit_arch(),
            };
            let mut probes: Vec<u32> = plan.denied.iter().map(|(_, n)| *n as u32).collect();
            probes.extend(innocuous(arch));
            probes.push(u32::MAX);
            for audit in [foreign, AUDIT_ARCH_I386, 0] {
                for number in &probes {
                    assert_eq!(
                        plan.evaluate(audit, *number),
                        RET_KILL_PROCESS,
                        "audit arch {audit:#x} must never run on a {} filter (syscall {number})",
                        arch.label()
                    );
                }
            }
        }
    }

    #[test]
    fn pseudo_random_syscall_numbers_agree_with_the_deny_list() {
        // Property: for a matching architecture the program never kills; it
        // returns EPERM exactly for the listed numbers and allows everything
        // else. A wrong jump offset or a duplicated entry breaks this.
        let mut seed: u32 = 0x5eed_1234;
        for arch in [SeccompArch::X86_64, SeccompArch::Aarch64] {
            let plan = plan(&standard_policy(), arch).unwrap();
            let audit = arch.audit_arch();
            let (mut denied, mut allowed) = (0usize, 0usize);
            for _ in 0..2_000 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let number = seed % 512;
                let listed = plan.denied.iter().any(|(_, n)| *n as u32 == number);
                let action = plan.evaluate(audit, number);
                if listed {
                    denied += 1;
                    assert_eq!(action, ret_errno(DENY_ERRNO), "listed syscall {number}");
                } else {
                    allowed += 1;
                    assert_eq!(action, RET_ALLOW, "unlisted syscall {number}");
                }
            }
            assert!(denied > 0 && allowed > 0, "the sample covers both outcomes");
        }
    }

    #[test]
    fn malformed_instruction_streams_fail_closed() {
        let base = plan(&standard_policy(), SeccompArch::X86_64).unwrap();
        let audit = SeccompArch::X86_64.audit_arch();
        let unlisted = u32::MAX;

        let empty = SeccompPlan {
            arch: base.arch,
            denied: base.denied.clone(),
            instructions: Vec::new(),
        };
        assert_eq!(empty.evaluate(audit, 0), RET_KILL_PROCESS);
        assert!(empty.verify_encoding().is_err());

        // Truncated: the final allow is missing, so a syscall that is not
        // denied walks off the end.
        let mut truncated = base.clone();
        truncated.instructions.pop();
        assert_eq!(truncated.evaluate(audit, unlisted), RET_KILL_PROCESS);
        assert!(truncated.verify_encoding().is_err());

        // Unknown opcode: the kernel would reject the program at load time.
        let mut unknown = base.clone();
        unknown.instructions[6].code = 0x99;
        assert_eq!(unknown.evaluate(audit, unlisted), RET_KILL_PROCESS);
        assert!(unknown.verify_encoding().is_err());

        // A load from an offset that is not seccomp_data.nr/arch.
        let mut bogus_load = base.clone();
        bogus_load.instructions[3].k = 8;
        assert_eq!(bogus_load.evaluate(audit, unlisted), RET_KILL_PROCESS);
        assert!(bogus_load.verify_encoding().is_err());

        // A jump past the end of the program.
        let mut overrun = base.clone();
        overrun.instructions[1].jt = 200;
        assert_eq!(overrun.evaluate(audit, 0), RET_KILL_PROCESS);
        assert!(overrun.verify_encoding().is_err());

        // A jump that skips a comparison, so one denial would never match.
        let mut skip = base.clone();
        skip.instructions[4].jf = 3;
        assert_eq!(
            skip.evaluate(audit, 165),
            ret_errno(DENY_ERRNO),
            "the mount comparison itself still matches"
        );
        assert_eq!(
            skip.evaluate(audit, 166),
            RET_ALLOW,
            "umount2 is skipped by the widened fall-through"
        );
        assert!(
            skip.verify_encoding().is_err(),
            "a denial that cannot be reached must fail verification"
        );
    }

    #[test]
    fn verification_catches_the_historical_inverted_arch_check() {
        let mut plan = plan(&standard_policy(), SeccompArch::X86_64).unwrap();
        // The bug that shipped: a matching architecture skipped the *allow*
        // path and fell into the kill, so the app died on its first syscall.
        plan.instructions[1].jt = 0;
        plan.instructions[1].jf = 1;
        assert_eq!(
            plan.evaluate(SeccompArch::X86_64.audit_arch(), 0),
            RET_KILL_PROCESS,
            "the inverted check kills the first syscall"
        );
        assert!(
            plan.verify_encoding().is_err(),
            "verify_encoding must reject the inverted arch check"
        );
    }

    #[test]
    fn verification_rejects_a_wrong_number_or_a_wrong_default() {
        let base = plan(&standard_policy(), SeccompArch::X86_64).unwrap();
        let mut wrong_number = base.clone();
        wrong_number.instructions[4].k += 1;
        assert!(wrong_number.verify_encoding().is_err());

        let mut wrong_default = base.clone();
        let last = wrong_default.instructions.len() - 1;
        wrong_default.instructions[last].k = ret_errno(DENY_ERRNO);
        assert!(wrong_default.verify_encoding().is_err());

        let mut wrong_deny_action = base.clone();
        wrong_deny_action.instructions[5].k = RET_ALLOW;
        assert!(wrong_deny_action.verify_encoding().is_err());

        for arch in [SeccompArch::X86_64, SeccompArch::Aarch64] {
            plan(&standard_policy(), arch)
                .unwrap()
                .verify_encoding()
                .unwrap_or_else(|error| panic!("{}: {error}", arch.label()));
        }
    }

    #[test]
    fn deny_table_has_unique_names_and_numbers() {
        let mut names: Vec<&str> = DENIED.iter().map(|denial| denial.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), DENIED.len(), "duplicate name in DENIED");
        for arch in [SeccompArch::X86_64, SeccompArch::Aarch64] {
            let mut numbers: Vec<i64> = DENIED
                .iter()
                .map(|denial| arch.number(denial))
                .filter(|number| *number >= 0)
                .collect();
            numbers.sort_unstable();
            numbers.dedup();
            let expected = DENIED.iter().filter(|d| arch.number(d) >= 0).count();
            assert_eq!(
                numbers.len(),
                expected,
                "duplicate syscall number for {}",
                arch.label()
            );
        }
    }
}
