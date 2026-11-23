# aether-lifecycled — process & app lifecycle (P5)

Implements `docs/2026/11/16/os-project/design/process-manager-design.md` (D3)
and the P5 process block of the roadmap. The policy layer has no I/O and is
fully unit-tested; kernel access is behind traits.

## Status

| Task | State | Evidence |
|---|---|---|
| P5-T01 App identity + one cgroup scope per app | Done (logic + tests) | `src/identity.rs`: validated desktop IDs, deterministic `aether-app-<id>-<hash>.scope`, one scope per app, cgroup v2 `aether.slice/aether-apps.slice/` layout |
| P5-T02 Lifecycle state machine | Done | `src/state.rs`: every D3 §3 transition encoded once; 17 allowed rows + forbidden rows tested |
| P5-T03 Lifecycle protocol | Done (logic + real-socket test) | `src/protocol.rs`, `src/bin/aether-lifecycle-testapp.rs`; integration test runs the real binaries and verifies the state file is written on `WillSuspend` |
| P5-T04 Freeze/hibernate + zram | Done (logic + fake-tree tests) | `src/cgroup.rs`: `cgroup.freeze`, `memory.reclaim`, zram `disksize`, `vm.swappiness`; real-hardware freeze test is in the D3 test table for the reference machine |
| P5-T05 Pressure-driven killer | Done (policy + tests) | `src/pressure.rs`, `src/policy.rs`: PSI parsing, thresholds, priority bands, kill-storm guard, foreground never killed |
| P5-T06 Exceptions + background budget | Done | `src/budget.rs`, protection reasons, `cpu.weight` lowering |
| P5-T08 50-app benchmark + soak harness | Done (harness) | `src/bin/aether-bench.rs`, `bench/bench-50.sh`, `bench/soak-72h.sh`; hardware gates stay pending |
| P5-T07 Activity Manager | see `apps/taskmanager` | — |
| P5-T09..T17 | see `powerd/` | — |

## Scope layout

```
/sys/fs/cgroup/aether.slice/               root slice (memory+cpu+pids delegated)
/sys/fs/cgroup/aether.slice/aether-apps.slice/
/sys/fs/cgroup/aether.slice/aether-apps.slice/aether-app-org.example.Mail-3f2a91c4.scope/
    cgroup.procs        one app = one scope; all its processes are attached here
    cgroup.freeze       1 while suspended/hibernated
    memory.current      accounting for metrics
    memory.reclaim      proactive hibernation (kernel 5.19+)
    cpu.weight          lowered for background/throttled apps
```

The 8-hex-digit FNV-1a suffix keeps the mapping one-to-one even when two app
IDs sanitize to the same text.

## Lifecycle protocol (SDK)

The app binds `$XDG_RUNTIME_DIR/aether/lifecycle/aether-app-<id>-<hash>.sock`.
Newline-delimited JSON both ways:

```text
daemon -> app  {"type":"will-suspend","app":"org.example.Mail"}
app -> daemon  {"ack":"accepted","app":"org.example.Mail"}
```

- `will-suspend`: app has up to 2 s to save state and acknowledge. On timeout
  the app is suspended anyway unless it holds an exception (Audio/Call).
- `did-resume`: informational, no acknowledgement.
- `memory-warning`: app has 500 ms to release caches before a possible kill.
- `background-budget-exceeded`: informational; the app's `cpu.weight` is lowered.

Apps without the SDK are still suspended; they just miss the messages.

## Configuration

`/etc/aether/lifecycle.toml`, overridden by
`$XDG_CONFIG_HOME/aether/lifecycle.toml`:

```toml
cgroup_root = "/sys/fs/cgroup"
socket_dir = "/run/user/1000/aether/lifecycle"
state_dir = "/var/lib/aether/lifecycled"
dry_run = false
grace_ac_seconds = 30
grace_battery_seconds = 10
hibernate_after_minutes = 10
```

Defaults match D3 §3. `dry_run = true` performs no kernel writes and keeps the
registry in the state directory; it is used by the benchmark and the macOS
smoke test.

## Commands

```bash
# Real daemon (AetherOS)
aether-lifecycled

# One-shot metrics snapshot (JSON) for Activity Manager / benchmarks
aether-lifecycled --once

# Reproducible end-to-end smoke on any host (no cgroups needed)
aether-lifecycled --simulate

# SDK test app
aether-lifecycle-testapp --id org.example.TestApp --socket-dir /tmp/aet --state-file /tmp/state.txt

# 50-app benchmark
../lifecycled/bench/bench-50.sh
```

## Tests

```bash
cargo test -p aether-lifecycled          # 62 unit + 2 integration tests
cargo run -p aether-lifecycled -- --simulate
```

`--simulate` demonstrates: launch → hide → grace → suspend → hibernate →
critical pressure → kill; an audio-protected app stays alive and unfrozen, the
foreground app is never selected.

## Safety rules

- The daemon never freezes or kills a process outside its own app scopes
  (`is_managed_scope` check, pids read from `cgroup.procs`).
- Freeze/reclaim failures are logged and not retried more than twice; a failed
  freeze leaves the app in Background (D3 §8).
- Missing PSI (`CONFIG_PSI` off) falls back to reclaim-only behavior and never
  crashes (spike S3 finding).
- Missing `memory.reclaim` (kernel < 5.19) disables hibernation but keeps
  suspension working.
