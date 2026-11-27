# Sandbox hardening — seccomp encoding, Landlock rules, strict launch (2026-11-27)

Session `sandbox` (overnight parallel run). Follow-up to `a22f52e`
("seccomp arch check skipped the kill branch (every syscall was killed); evaluate
the BPF program in tests").

The fix shipped, but nothing in the crate *proved* the filter program was what
the builder intended. The bug was invisible for three reasons, and each one is
closed here:

1. The encoding was asserted by **index-based unit tests** (`insns[1].jt == 1`)
   that repeated the builder's own arithmetic, so a mistaken offset and a
   mistaken expectation agreed.
2. The program was **never executed** by a test, so a program that killed every
   syscall only failed on a kernel that ran it (the Dell).
3. The **launcher** was only tested for the layers that existed on the macOS dev
   host, so a missing layer could be added to `command_strict` and fall back to
   a weaker sandbox without a red test.

Territory: `apps/sandbox/**` and this file. No hardware was touched; the Dell is
in use by another workstream and Linux binaries cannot run on this host tonight.

## 1. What changed

| File | Change |
|---|---|
| `apps/sandbox/src/seccomp.rs` | `SeccompPlan::verify_encoding()`: structural walk of every emitted instruction and both targets of every conditional jump; `plan()` now runs it and returns `SeccompFailed` on any inconsistency, so a bad program never reaches `prctl`. `jump_target`/`instruction` helpers shared by the verifier. New tests (see §2). |
| `apps/sandbox/tests/syscall_table.rs` (new) | Cross-check of every `DENIED` entry against `libc::SYS_*`, once per architecture, compiled only for a Linux host of that architecture. |
| `apps/sandbox/src/landlock.rs` | `normalize()` extracted (collapse duplicates, read-write wins). `from_profile` **and** `parse_shim_args` use it, so the launcher and shim agree; `shim_arguments` normalizes too. Fixed the shim parser's order-dependent dedup (§3). `PathBeneathAttr` now initializes its padding; `LandlockRule` errno is read before `close()`. |
| `apps/sandbox/src/enforce.rs` | `require_strict_layers()` gate (bwrap → user namespaces → Landlock → seccomp, in that order) called by `command_strict`; previously bwrap/userns were never re-checked and seccomp *availability* was never consulted. Tests per missing layer. |
| `apps/sandbox/tests/landlock_enforce.rs` | Kernel-level test that a path passed `--rw` and `--ro` stays writable (both orders), and that a rule for a missing path refuses the launch. |
| `apps/sandbox/README.md` | Seccomp verification story, syscall-table cross-check, rule normalization and the missing-path decision. |

No public API was removed or renamed. `Enforcer::command_strict` keeps its
signature; it simply refuses more cases (correctly).

## 2. Test inventory — what each would have caught

Unit tests in `src/seccomp.rs` (`cargo test -p aether-sandbox --lib`):

| Test | Proves | Would have caught |
|---|---|---|
| `every_denied_syscall_is_denied_and_innocuous_ones_are_allowed` | For x86_64 **and** aarch64: `plan.denied` equals the table resolved per arch (nothing dropped or invented), every denial evaluates to `ret_errno(EPERM)`, and 30+ syscalls an app cannot live without (`read`, `write`, `openat`, `mmap`, `clone`, `execve`, `exit`, `futex`, …) evaluate to `RET_ALLOW`. | The arch inversion (every syscall killed), a deny entry whose number is wrong on one arch, a silently dropped denial (`iopl`/`ioperm` on aarch64). |
| `a_foreign_architecture_kills_every_syscall` | `AUDIT_ARCH_*` of the other arch, `AUDIT_ARCH_I386` and `0` all evaluate to `RET_KILL_PROCESS` for every denial, every innocuous syscall and `u32::MAX`. | A "fix" that turns the kill into an allow, or an arch comparison that matches the wrong constant. |
| `pseudo_random_syscall_numbers_agree_with_the_deny_list` | 2 000 deterministic pseudo-random numbers per arch: never KILL on a matching arch, `EPERM` exactly for the listed numbers, `ALLOW` otherwise. | Any jump offset or duplicate-table error that makes a comparison unreachable or applied to the wrong entry (the old `program_checks_arch_then_denies_then_allows` test could not). |
| `malformed_instruction_streams_fail_closed` | Empty program, truncated tail (no final `ALLOW`), unknown opcode, load from a bogus `seccomp_data` offset, jump past the end, jump that skips a denial — all evaluate to `KILL_PROCESS` and all fail `verify_encoding`. | An evaluator that "helpfully" defaults to `ALLOW` when the walk runs off the program, which would make the evaluation tests pass for the wrong reason. |
| `verification_catches_the_historical_inverted_arch_check` | With `jt=0, jf=1` (the shipped bug) evaluation kills syscall 0 and `verify_encoding()` errors. | The exact bug: the plan gate now refuses to build the filter. |
| `verification_rejects_a_wrong_number_or_a_wrong_default` | Mutated comparison constant, mutated final `ALLOW`, mutated denial action are each rejected; both shipped plans verify. | A table edit that lands in the program but not in `plan.denied`, and a "deny" that returns `ALLOW`. |
| `deny_table_has_unique_names_and_numbers` | No duplicate name, no duplicate number per arch. | A paste error that makes one entry unreachable. |

Encoding audit, by construction (`verify_encoding` runs inside `plan()`):

- instruction 0 must be `LD W ABS` from `seccomp_data.arch`;
- instruction 1 must compare against `arch.audit_arch()`, its **taken** branch
  must land on the `seccomp_data.nr` load, its **fall-through** on
  `RET_KILL_PROCESS`;
- each denial's comparison must carry the table's number, its taken branch must
  land on its own `ret_errno(EPERM)`, and its fall-through must be exactly the
  next instruction (so no denial can be skipped);
- the last fall-through must be `RET_ALLOW`, every opcode must be one this
  crate emits, every target must be in bounds, and the instruction count must be
  `5 + 2 × denials`.

Syscall numbers (`tests/syscall_table.rs`): every entry of `DENIED` is compared
with `libc::SYS_*` for the host architecture, and entries marked absent (`-1`,
`iopl`/`ioperm` on aarch64) must be absent from the comparison list. The test is
gated to `target_os = "linux"` because `libc` only publishes the numbers of the
architecture it was built for; on macOS it compiles to zero tests.
`ci.yml` runs `cargo test --workspace --all-targets` on Linux x86_64, so the
x86_64 half runs on every push.

## 3. Landlock audit — one real bug found

The shim's parser deduplicated rules with `sort_by(path)` followed by
`dedup_by(|a, b| a.path == b.path)`, which **keeps the first occurrence and
discards the later one** — and `sort_by` is stable, so the outcome depended on
argument order. `aether-landlock --ro /data/app --rw /data/app` produced a
read-only rule while `--rw … --ro …` produced a writable one. The launcher's own
policy builder widened correctly, so this only bit a hand-written or foreign
invocation, but it is exactly the class of order-dependent sandbox bug the CI
parity work is trying to eliminate.

Fixed by one shared `normalize()`: sort, then on a duplicate keep the wider
grant (read-write sets `READ_WRITE`). Used by `LandlockPolicy::from_profile`,
`parse_shim_args`, and `shim_arguments`, so the emitted argv and the applied
ruleset cannot disagree.

Tests:

| Test | Proves |
|---|---|
| `normalize_keeps_the_widest_grant_in_any_order` (unit) | read-only + read-write in either order, plus triple duplicates → one read-write rule. |
| `shim_parser_widens_and_collapses_like_the_profile_builder` (unit) | same through the real `parse_shim_args` for `--ro`/`--rw`, `--rw`/`--ro`, and a three-argument duplicate list; duplicate `/usr` rules collapse. |
| `shim_arguments_are_normalized_even_for_a_hand_built_policy` (unit) | a hand-built `LandlockPolicy` still emits exactly one canonical rule per path. |
| `read_write_wins_over_read_only_for_the_same_path` (integration, kernel) | the shim, on a real Landlock kernel, lets `/bin/sh` **write** into a directory passed both `--rw` and `--ro`, in both orders. |
| `a_rule_for_a_missing_path_is_still_emitted` (unit) + `a_missing_rule_path_refuses_the_launch` (integration) | decision below. |

**Non-existent paths: emitted, not skipped.** Neither `from_profile` nor
`shim_arguments` filters on existence. `Enforcer::command` skips a missing source
for bwrap's `--ro-bind`/`--bind` (bwrap rejects a missing source outright), but
the Landlock rule is kept because the launcher and the shim must apply one rule
set: silently dropping it would start the app with a ruleset nobody reviewed and
would hide profile drift. The shim fails closed instead — `open_path` fails,
`LandlockRule` is returned, exit 126, program never run — which the integration
test asserts on every host (exit 126) and, on a Landlock kernel, also names the
path in stderr. That path is exercised by the `landlock` command in
`aether-sandbox` too, so a stale profile is a loud launch failure, not a quiet
narrowing.

Two more Linux-side fixes found while auditing:

- `PathBeneathAttr` (`u64 allowed_access; i32 parent_fd`) was built from a struct
  literal, leaving its 4 bytes of tail padding uninitialized. The kernel copies
  the whole struct, so the padding is now an explicit zeroed `_pad` field (ABI
  layout unchanged).
- The `open_path` failure path read `io::Error::last_os_error()` *after*
  `close(ruleset)`, so a failing `close` could replace the reason reported for
  the rule. errno is now captured first.

## 4. Strict launch — every layer gated, nothing spawned on refusal

`command_strict` previously checked only Landlock availability, then called
`landlock::plan` and `seccomp::plan`. It never re-checked bwrap/user namespaces
(the `with_bwrap` seam and `Enforcer::system()` could disagree) and never asked
whether the kernel could actually **install** a seccomp filter. New
`require_strict_layers()` checks, in order: bwrap present, user namespaces
enabled, Landlock available, seccomp available — each returning the layer-specific
error that names the layer. Since the return type is `Result<Command, _>`, an
`Err` means no `Command` exists to spawn.

| Test | Proves |
|---|---|
| `strict_launch_refuses_each_missing_layer_by_name` | Four cases (no bwrap, no user namespaces, no Landlock, no seccomp) each return an error whose message names that layer; all four present → `Ok`. The seccomp status is injected, so the seccomp branch is proven on a host that cannot install a filter. |
| `command_strict_is_all_or_nothing_on_this_host` | With Landlock removed: `LandlockUnavailable`, and a marker file the entry point would create stays absent. With the live probe, `command_strict` succeeds **iff** bwrap + userns + Landlock + seccomp are all available — no namespace-only third outcome. |

## 5. Other `#[cfg(target_os = "linux")]` paths reviewed

| Path | Risk | Verdict |
|---|---|---|
| `seccomp::install` (`prctl` `PR_SET_NO_NEW_PRIVS`, `PR_SET_SECCOMP`) | A `sock_fprog`/instruction mismatch, or `len` truncation. | Reviewed against `linux/filter.h`: `sock_filter` fields map 1:1, `len` is 77 (max 4096), the vector outlives the call. No change. Failures return errors and the shim exits 126. |
| `seccomp::probe` (Linux branch) | `/proc/self/status` always has a `Seccomp:` line, so the fallback is optimistic on old kernels. | Accepted and documented in code: a false positive only means the shim attempts the install and refuses on failure (fail-closed); a false negative would block launches the kernel can enforce. |
| `landlock::probe` / `apply_plan` / `sys::apply` | Hand-written syscall numbers (444–446), struct layout vs `landlock_*_attr`, ABI masking. | Numbers are the generic table values and match `libc 0.2.191`; kept literal because the dependency allows older 0.2.x. Struct layout: `RulesetAttr` is one `u64`; `PathBeneathAttr` padding fixed (§3). ABI masking already tested (`handled_rights_are_masked_by_abi`). |
| `landlock::apply` ordering | `no_new_privs` must precede `restrict_self`; rules must be added before restricting. | Order verified by reading the kernel contract: create → add rules → `PR_SET_NO_NEW_PRIVS` → `restrict_self`. Errors close the ruleset fd and return. No change. |
| `seccomp::apply` | Filter installed *after* Landlock in the shim; `no_new_privs` set twice. | Idempotent and intended; a seccomp failure cannot leave the app running because the shim exits before `exec`. No change. |
| `Enforcer::describe` / `aether-sandbox doctor` | Reported capabilities could drift from the strict gate. | `doctor`'s `strict_launch` is `usable() && landlock && seccomp.available`, the same four inputs as `require_strict_layers`. No change. |

Additionally, cross-compilation was used as a compile-time check of the
Linux-only code (which macOS never compiles): `cargo check --all-targets
--target x86_64-unknown-linux-gnu` and `--target aarch64-unknown-linux-gnu` both
pass with no warnings, which is what validates the `libc::SYS_*` names in the new
table test on both architectures.

## 6. Commands and results

```
$ LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-sandbox
   lib            50 passed
   landlock_enforce 5 passed
   seccomp_enforce  2 passed
   syscall_table    0 tests (host-gated: Linux only)
   → 57 passed, 0 failed

$ LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-runner
   41 lib + 4 integration = 45 passed, 0 failed
   (strict_layers_are_fail_closed still passes: the refusal now names Landlock
    first, which its assertion accepts)

$ LIBRARY_PATH=/opt/homebrew/lib cargo clippy -p aether-sandbox --all-targets
   Finished; no warnings from aether-sandbox (only pre-existing aether-pkg ones)

$ cargo check -p aether-sandbox --all-targets --target x86_64-unknown-linux-gnu
   Finished; no warnings
$ cargo check -p aether-sandbox --all-targets --target aarch64-unknown-linux-gnu
   Finished; no warnings

$ cargo run -q -p aether-sandbox --bin aether-sandbox -- seccomp | tail -6
   "available": false, "reason": "seccomp is a Linux kernel feature and this host is macos"
   SECCOMP_PLAN_OK

$ cargo run -q -p aether-sandbox --bin aether-landlock -- --seccomp --ro /usr --ro /usr --rw /usr -- /bin/true
   aether-landlock: refusing to start the app: Landlock enforcement is unavailable: ... macos
   exit 126   (argument normalization accepted the duplicate/conflicting rules; the
               refusal is the Landlock layer, not the parser)
```

Offline audit performed in addition to the test suite: the 36 table entries were
extracted from the kernel-UAPI values in `libc 0.2.191`
(`…/linux/gnu/b64/x86_64/not_x32.rs`, `…/aarch64/mod.rs`) and diffed against
`DENIED` — 36 entries, 0 mismatches for x86_64, 0 for aarch64, and
`iopl`/`ioperm` correctly absent on aarch64.

## 7. Residual checks that need a Linux kernel (not run tonight)

Nothing below is claimed as verified. Each is either covered by CI on push or has
an exact command to run on the Dell/AetherOS image:

1. **Kernel execution of the denials** — `aether-landlock --seccomp … -- setarch
   --addr-no-randomize /bin/true` must fail (personality denied) while the
   unfiltered run succeeds. Already in `tests/seccomp_enforce.rs`; skipped on
   macOS, runs in CI.
2. **`libc::SYS_*` comparison test** — `cargo test -p aether-sandbox --test
   syscall_table` on Linux x86_64 (CI) and on an aarch64 Linux host (not covered
   by any CI job; the crate compiles for it but the test has never executed).
3. **Landlock read-write-wins end to end** — `read_write_wins_over_read_only_for_the_same_path`
   writes a file through the shim on a real Landlock kernel; skipped here.
4. **Landlock missing-path refusal names the path** — the Linux-only stderr
   assertion in `a_missing_rule_path_refuses_the_launch`; the exit-126 half runs
   on macOS.
5. **Real seccomp install of the verified program** — `aether-landlock --seccomp`
   on the image, then `grep Seccomp /proc/<pid>/status` for mode 2, plus a denied
   syscall check. The evaluator proves the program's semantics; only the kernel
   can prove it accepts the encoding (`verify_encoding` mirrors the kernel's
   classic-BPF rules, it does not replace the loader's validation).
6. **`command_strict` end-to-end** — a strict launch in CI: `aether-runner launch
   … --strict-sandbox` where Landlock and seccomp exist, asserting the app runs
   and the shim reported ABI + denial count.

## 8. Not claimed

- No AP-AC-05 / D8 sign-off: the deny list still must be validated against the
  shipped kernel and the image's installed shim.
- The `SeccompPolicy::without` exception path still has no manifest surface, so an
  app that legitimately needs `personality` (an emulator) cannot be launched.
- Network mediation below Landlock ABI 4 remains namespace-only, unchanged.
