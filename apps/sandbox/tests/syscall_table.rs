//! Cross-check the hard-coded seccomp syscall table against `libc` (S).
//!
//! The filter compares `seccomp_data.nr` against numbers taken from the kernel
//! UAPI tables. A typo there is silent: the filter installs, and the app either
//! loses a syscall it needs or keeps one it must not have. `libc` carries the
//! same numbers for the architecture it was built for, so these tests compare
//! the table against `libc::SYS_*`.
//!
//! `libc` only exposes the numbers of the *host* architecture, so each test is
//! compiled for a Linux host of that architecture. On the macOS development host
//! this file compiles to zero tests; the check runs in the Linux CI job (x86_64)
//! and on any aarch64 Linux host. That residual gap is recorded in
//! `docs/2026/11/27/results/sandbox-seccomp-hardening.md`.

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
use aether_sandbox::seccomp::{SeccompArch, DENIED};

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
fn compare(arch: SeccompArch, expected: &[(&str, i64)]) {
    // Every denial needs an independent number to compare with: a name that is
    // missing here would silently go unchecked.
    assert_eq!(
        expected.len(),
        DENIED.len(),
        "every DENIED entry needs a libc::SYS_* constant for {}",
        arch.label()
    );
    for (name, libc_number) in expected {
        let denial = DENIED
            .iter()
            .find(|denial| denial.name == *name)
            .unwrap_or_else(|| panic!("{name} is in the libc list but not in DENIED"));
        let table = arch.number(denial);
        assert_eq!(
            table,
            *libc_number,
            "{name} on {}: the table says {table}, libc::SYS_* says {libc_number}",
            arch.label()
        );
    }
    // The table marks the syscalls this architecture does not have with -1,
    // and they must not appear in the libc comparison at all.
    for denial in DENIED {
        if arch.number(denial) < 0 {
            assert!(
                !expected.iter().any(|(name, _)| *name == denial.name),
                "{} is marked absent on {} but compared against libc",
                denial.name,
                arch.label()
            );
        }
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn x86_64_numbers_match_libc() {
    compare(
        SeccompArch::X86_64,
        &[
            ("mount", libc::SYS_mount as i64),
            ("umount2", libc::SYS_umount2 as i64),
            ("pivot_root", libc::SYS_pivot_root as i64),
            ("chroot", libc::SYS_chroot as i64),
            ("open_by_handle_at", libc::SYS_open_by_handle_at as i64),
            ("init_module", libc::SYS_init_module as i64),
            ("finit_module", libc::SYS_finit_module as i64),
            ("delete_module", libc::SYS_delete_module as i64),
            ("kexec_load", libc::SYS_kexec_load as i64),
            ("kexec_file_load", libc::SYS_kexec_file_load as i64),
            ("bpf", libc::SYS_bpf as i64),
            ("perf_event_open", libc::SYS_perf_event_open as i64),
            ("ptrace", libc::SYS_ptrace as i64),
            ("process_vm_readv", libc::SYS_process_vm_readv as i64),
            ("process_vm_writev", libc::SYS_process_vm_writev as i64),
            ("userfaultfd", libc::SYS_userfaultfd as i64),
            ("unshare", libc::SYS_unshare as i64),
            ("setns", libc::SYS_setns as i64),
            ("add_key", libc::SYS_add_key as i64),
            ("request_key", libc::SYS_request_key as i64),
            ("keyctl", libc::SYS_keyctl as i64),
            ("swapon", libc::SYS_swapon as i64),
            ("swapoff", libc::SYS_swapoff as i64),
            ("reboot", libc::SYS_reboot as i64),
            ("sethostname", libc::SYS_sethostname as i64),
            ("setdomainname", libc::SYS_setdomainname as i64),
            ("adjtimex", libc::SYS_adjtimex as i64),
            ("settimeofday", libc::SYS_settimeofday as i64),
            ("clock_settime", libc::SYS_clock_settime as i64),
            ("syslog", libc::SYS_syslog as i64),
            ("acct", libc::SYS_acct as i64),
            ("quotactl", libc::SYS_quotactl as i64),
            ("fanotify_init", libc::SYS_fanotify_init as i64),
            ("personality", libc::SYS_personality as i64),
            ("iopl", libc::SYS_iopl as i64),
            ("ioperm", libc::SYS_ioperm as i64),
        ],
    );
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
#[test]
fn aarch64_numbers_match_libc() {
    // iopl/ioperm do not exist on aarch64 (the table marks them -1 and
    // `compare` checks that they are absent from this list).
    compare(
        SeccompArch::Aarch64,
        &[
            ("mount", libc::SYS_mount as i64),
            ("umount2", libc::SYS_umount2 as i64),
            ("pivot_root", libc::SYS_pivot_root as i64),
            ("chroot", libc::SYS_chroot as i64),
            ("open_by_handle_at", libc::SYS_open_by_handle_at as i64),
            ("init_module", libc::SYS_init_module as i64),
            ("finit_module", libc::SYS_finit_module as i64),
            ("delete_module", libc::SYS_delete_module as i64),
            ("kexec_load", libc::SYS_kexec_load as i64),
            ("kexec_file_load", libc::SYS_kexec_file_load as i64),
            ("bpf", libc::SYS_bpf as i64),
            ("perf_event_open", libc::SYS_perf_event_open as i64),
            ("ptrace", libc::SYS_ptrace as i64),
            ("process_vm_readv", libc::SYS_process_vm_readv as i64),
            ("process_vm_writev", libc::SYS_process_vm_writev as i64),
            ("userfaultfd", libc::SYS_userfaultfd as i64),
            ("unshare", libc::SYS_unshare as i64),
            ("setns", libc::SYS_setns as i64),
            ("add_key", libc::SYS_add_key as i64),
            ("request_key", libc::SYS_request_key as i64),
            ("keyctl", libc::SYS_keyctl as i64),
            ("swapon", libc::SYS_swapon as i64),
            ("swapoff", libc::SYS_swapoff as i64),
            ("reboot", libc::SYS_reboot as i64),
            ("sethostname", libc::SYS_sethostname as i64),
            ("setdomainname", libc::SYS_setdomainname as i64),
            ("adjtimex", libc::SYS_adjtimex as i64),
            ("settimeofday", libc::SYS_settimeofday as i64),
            ("clock_settime", libc::SYS_clock_settime as i64),
            ("syslog", libc::SYS_syslog as i64),
            ("acct", libc::SYS_acct as i64),
            ("quotactl", libc::SYS_quotactl as i64),
            ("fanotify_init", libc::SYS_fanotify_init as i64),
            ("personality", libc::SYS_personality as i64),
        ],
    );
}
