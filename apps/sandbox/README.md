# aether-sandbox — sandbox, permission broker and Landlock enforcement (P6-T04)

Profiles come from the persisted **grants**, never from the manifest alone (a
denied permission stays denied). Launch is always enforced and **fail-closed**:
if bubblewrap, user namespaces or Landlock are unavailable, the app does not
start and the reason is printed — it is never started unsandboxed
(D8 section 5 failure mode).

## Layers

| Layer | Where | What it stops |
|---|---|---|
| Namespaces + mounts | `src/enforce.rs` via `bwrap` | App sees only its granted mounts, no network unless granted, no host PID/IPC/UTS |
| Landlock ruleset | `src/landlock.rs` + `src/bin/aether-landlock.rs` | Filesystem access outside the profile paths, even through symlinks or `..` |
| Seccomp filter | `src/seccomp.rs` + `src/bin/aether-landlock.rs` | Syscalls that escape the sandbox (mount/pivot_root/chroot, module loading, kexec, bpf, ptrace/process_vm_*, userfaultfd, unshare/setns, keyring, swap, reboot, clock, syslog, raw I/O ports) |
| Permission broker | `src/permissions.rs` | Prompts, "don't ask again" denials that survive restarts |
| Profiles | `src/profile.rs` | Builds the path set from grants (read-only system dirs, writable app folder) |

## Landlock (fail-closed)

`landlock::probe()` asks the kernel for the Landlock ABI
(`landlock_create_ruleset` with the VERSION flag). `landlock::plan()` masks the
requested rights down to the ABI the kernel supports (ABI 2 adds `refer`,
ABI 3 `truncate`, ABI 5 `ioctl-dev`) and returns an error when nothing can be
enforced.

`Enforcer::command_with_landlock` wraps the entry point with the shim inside
the namespace sandbox:

```
bwrap … -- /usr/libexec/aether-landlock --ro /usr --ro /etc --rw /data/app-… -- /data/app-…/bin/app
```

The shim applies the ruleset (create → add rules → `PR_SET_NO_NEW_PRIVS` →
`restrict_self`) and only then `exec`s the app. On any failure it exits 126
without executing the program, so a partial ruleset can never let the app run.

`Runner::require_landlock(true)` (CLI: `aether-runner launch APP --strict-sandbox`)
uses this path. The image must install the shim at
`/usr/libexec/aether-landlock`.

## Seccomp (fail-closed)

`seccomp::plan()` compiles the policy into a classic BPF program for the target
architecture. The program first compares `seccomp_data.arch` and **kills the
process** (`SECCOMP_RET_KILL_PROCESS`) if it does not match the architecture it
was built for, so a filter can never be bypassed by running foreign-architecture
code. It then compares `seccomp_data.nr` against every denial and returns
`EPERM` for a match; anything else is allowed.

The program is hand-built, so it is also checked before it reaches the kernel:
`SeccompPlan::verify_encoding()` walks every branch and fails the plan (and the
launch) when a jump does not land on the instruction the builder intended, and
`SeccompPlan::evaluate()` runs the program the way the kernel would, on any
host. The unit tests evaluate every denial on both architectures, the syscalls an
app cannot live without (`read`/`write`/`execve`/`mmap`/`clone`/`exit`), foreign
`AUDIT_ARCH_*` values, and truncated or malformed programs; a truncated stream
fails closed with `SECCOMP_RET_KILL_PROCESS`. The syscall-number table is
cross-checked against `libc::SYS_*` on a Linux host of each architecture.

The policy is default-allow with a curated deny list (`DENIED`), because a
strict allowlist would break ordinary applications; the deny list covers the
syscalls that escape a sandbox (see the table above). Each denial carries a
reason and its number on x86_64 and aarch64 from the kernel UAPI tables
(`syscall_64.tbl`, `asm-generic/unistd.h`); `iopl`/`ioperm` exist only on
x86_64.

`Enforcer::command_strict` requires namespaces **and** Landlock **and** seccomp.
`Runner::require_strict_layers(true)` uses it. In the shim, `--seccomp` installs
the filter after Landlock and before the exec; if installation fails the shim
exits 126 without running the program.

```
aether-sandbox seccomp [--arch x86_64|aarch64]   # print the filter plan
aether-landlock --probe --json                   # Landlock + seccomp verdicts
```

## Commands

```bash
aether-sandbox doctor                 # capabilities, Landlock ABI, seccomp, strict verdict
aether-sandbox profile --manifest APP/manifest.json --data DIR --granted network
aether-sandbox landlock --manifest APP/manifest.json --data DIR   # plan or refusal
aether-sandbox seccomp [--arch x86_64|aarch64]                     # filter plan
aether-sandbox grant APP-ID PERMISSION
aether-sandbox revoke APP-ID PERMISSION
aether-sandbox list APP-ID

aether-landlock --probe [--json]      # ABI/verdicts or LANDLOCK_UNAVAILABLE (exit 126)
aether-landlock [--seccomp] --ro /usr --rw /data/app -- /data/app/bin/app
```

## Honest limits

- Kernel enforcement is only exercised on Linux. On the macOS dev host the
  shim and both plan gates are tested for their refusal path (exit 126, program
  never run); the allow/deny Landlock and seccomp tests are skipped with a
  printed reason.
- Landlock cannot mediate network access below ABI 4 TCP rules; network is
  still controlled by the namespace (`--unshare-net`) and by the permission
  broker.
- Seccomp is default-allow with a deny list, not an allowlist: a syscall that is
  not on the list is permitted. Apps that legitimately need a denied syscall
  (for example an emulator using `personality`) need an explicit exception
  (`SeccompPolicy::without`), which is not yet surfaced in the manifest.
- Seccomp cannot filter by path or argument; Landlock and the mount set cover
  those. The syscall-number table is cross-checked against `libc::SYS_*` in the
  Linux CI job (`tests/syscall_table.rs`); that test compiles to zero tests on
  the macOS dev host, where the numbers are only checked for internal
  consistency and exercised through the evaluator.

## Rule normalization (Landlock)

A path may appear in `file_read`, `file_write` and `devices` more than once
(e.g. a picked file inside the app folder). `LandlockPolicy::from_profile` and
the shim's argument parser both normalize the rule list through the same
function: duplicates are collapsed and **read-write wins**, whatever the order.
The launcher therefore passes one rule per path, and a hand-written
`aether-landlock --ro DIR --rw DIR` invocation behaves the same way.

A rule for a path that does not exist is **kept**, not silently skipped: bwrap
skips the bind (it rejects a missing source), but the Landlock rule stays so the
launcher and the shim agree on one rule set, and the shim then fails closed when
it cannot open the path (exit 126, program never run). The alternative —
dropping it — would start the app with a ruleset nobody reviewed.
