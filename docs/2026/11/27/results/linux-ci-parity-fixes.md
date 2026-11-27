# Linux/CI parity — four bugs the macOS dev host could not see

- Date: 2026-11-27
- Session: S1 (`session - 1 (Nov27)`)
- Why: `cargo test --workspace` was green on the macOS dev host and red on the
  Ubuntu CI runner. Instead of re-running CI every two hours, Rust 1.100 was
  installed on the Dell (`ubuntu@192.168.68.112`) and the workspace was copied
  to `/home/anik/aether-build`, giving a real Linux loop of about two minutes.

## Bugs found and fixed

| # | Test | Symptom | Root cause | Fix |
| --- | --- | --- | --- | --- |
| 1 | `aether-sandbox` `enforce::tests::landlock_launch_wraps_the_entry_with_the_shim` | `assert_eq!(args[shim + 1], "--ro")` failed | The test assumed read-only rules come first. `LandlockPolicy::from_profile` sorts by path, and on Linux the writable data folder (`/tmp/...`) sorts before `/usr`. On macOS the test returned early (no Landlock ABI), so it never ran. | Assert the rule *shape* and that both kinds are present (`--ro` and `--rw`), never an order the policy does not promise. |
| 2 | `aether-compositor` `p3_t08_no_hang::hung_app_is_marked_not_responding_and_recovers` | Waited for `APP_STATE: client=0 state=not_responding` for 5 s while the log showed pongs | The watchdog numbers clients by connect order; on the Linux runner XWayland is also a client, so the test's own connection was not client 0. | `AETHER_COMPOSITOR_XDG_TOPLEVEL` now carries `client=<id>`; the test reads its own id from that marker (and the responsive case does too). |
| 3 | `aether-powerd` `telemetry::tests::sysfs_energy_battery_is_read` | `status.source` was `Ac`, expected `Battery` | `SysfsBattery::at()` hard-coded the host's `/sys/class/power_supply/AC`, so a fixture battery read the machine's real AC state. The Dell was plugged in; the runner was not. | `at()` carries no AC adapter; `system()` sets `/sys/class/power_supply/AC`. New test `fixture_battery_ignores_the_host_ac_adapter` pins both behaviours. |
| 4 | `aether-sandbox` `tests/seccomp_enforce` | `a filtered program must still run` failed; shim exited 159 (SIGSYS) at the first syscall after installation | **Real product bug.** The classic-BPF arch check used `jt = 0, jf = 1`. Offsets are relative to the *next* instruction, so a **matching** architecture fell straight into `RET_KILL_PROCESS`: every sandboxed app would have been killed on its first syscall on AetherOS. | `jt = 1, jf = 0`; added `SeccompPlan::evaluate()` (runs the emitted program the way the kernel does) plus tests for allow / deny / foreign-arch, and corrected the existing encoding test that had pinned the inverted values. |

## Evidence

```
# Dell, after the fixes
cd /home/anik/aether-build && cargo test --workspace
→ no FAILED line; WORKSPACE_DONE

# seccomp shim, real kernel (Landlock ABI 8)
./target/debug/aether-landlock --seccomp --ro /usr --ro /bin --ro /lib \
    --ro /lib64 --ro /etc -- /bin/cat /etc/hostname
aether-landlock: ABI 8 enforced, 5 rules, handled 0xd
aether-landlock: seccomp filter installed (36 denials, 77 instructions, x86_64)
anik-Inspiron-5402
exit=0

# before the fix, same command: exit 159, audit code 0x80000000 on syscall=1
```

## Commits

- `3e43953` sandbox Landlock test: rule shape instead of order (first attempt).
- `00eb1f5` compositor: client id in `XDG_TOPLEVEL`, no-hang test follows it.
- `f013ab7` powerd: fixture battery no longer reads the host AC adapter.
- `300b43a` sandbox: assert both rule kinds anywhere in the shim arguments.
- `a22f52e` sandbox: seccomp arch check skipped the kill; BPF evaluator in tests.

## Notes for the CI job

The seccomp test needs `kernels_ready()` (Landlock ABI + seccomp); it now also
proves the filter's semantics through `SeccompPlan::evaluate()`, so a broken
encoding fails even on a host that cannot install a filter.
