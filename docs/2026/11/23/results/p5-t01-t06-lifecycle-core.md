# P5-T01..T06 + P5-T08 — lifecycle core, protocol and benchmark harness

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run
- Scope: `lifecycled/`
- Design: D3 `process-manager-design.md`; gates in D6/04 §4

## What was verified

### Unit tests

Command (run in the repository workspace):

```bash
cargo test -p aether-lifecycled
```

Result: **62 unit tests pass, 2 integration tests pass, 0 failed**
(`2026-11-23`).

Coverage by task:

| Task | Tests |
|---|---|
| P5-T01 identity/scopes | accepts/rejects desktop IDs; deterministic scope path; distinct IDs never share a scope (`a.b` vs `a-b`); one scope per app; idempotent removal; leaf scope creation in a fake cgroup tree; dry-run needs no tree |
| P5-T02 state machine | all 17 allowed D3 §3 rows; 6 forbidden rows; same-state and terminated errors; termination is terminal; bands; protection reasons; inactive accounting |
| P5-T03 protocol | socket name stability/uniqueness; real `will-suspend` round trip acknowledged; no-client reported; unresponsive client times out at 500 ms; informational messages do not wait |
| P5-T04 mechanisms | freeze/thaw write `cgroup.freeze`; reclaim writes requested bytes; missing `memory.reclaim` fails cleanly; `memory.current`/`cgroup.procs` parsing; killer refuses to work outside a scope (dry-run); zram tune sets `disksize` and `swappiness=180`; no zram → 100 |
| P5-T05 pressure/killer | PSI parse; thresholds; missing PSI → Normal; hibernated killed first; largest memory within band; oldest focus on ties; foreground never killed; 2 s kill-storm guard; Elevated hibernates + reclaims without killing |
| P5-T06 exceptions/budget | Audio/Download protection semantics; protection added while frozen thaws to Background; per-app budget, window reset, `cpu.weight` lowered on exhaustion |
| Daemon wiring | one scope per app; hidden → grace → suspend → hibernate with real action calls; focus cancels the grace timer; critical pressure kills a background app only; metrics inactive share; registry restore after restart; illegal transition logged and ignored |

### Integration test (real binaries, real Unix socket)

`lifecycled/tests/lifecycle_protocol.rs` spawns
`aether-lifecycle-testapp`, waits for its ready marker, sends `WillSuspend`
through `SocketNotifier`, and asserts the app wrote its state file
(`open_documents=2`, cursor) and acknowledged.

Result: `test_app_saves_state_on_will_suspend ... ok`,
`test_app_gets_did_resume_without_waiting_for_ack ... ok`.

### End-to-end simulation

```bash
cargo run -p aether-lifecycled -- --simulate
```

Output (2026-11-23):

```text
after grace: mail=Suspended music=Background
after hibernate: mail=Hibernated music=Background
after critical pressure: fg=Foreground mail=None music=Background
metrics: total=62914560 inactive=0 share=0.00 kills=1
... state Background->Suspended reason=grace_elapsed
... suspend skipped: protected by Audio
... state Suspended->Hibernated reason=hibernation_timeout
... kill reason=MemoryPressure band=Hibernated
... state Hibernated->Terminated reason=killer
```

The audio app stayed alive and unfrozen; the foreground app was never selected.

### 50-app benchmark (P5-T08)

```bash
cargo run -p aether-lifecycled --bin aether-bench -- --apps 50 --no-spawn \
  --report docs/2026/11/23/results/p5-t08-bench-50.md
```

Result (`p5-t08-bench-50.md`): 50 apps, 1 cycle.

| Check | Result | Notes |
|---|---|---|
| B1 inactive memory ≤ 25% | PASS (model, 10.8%) | 20 MB/app model + 1.5 GB baseline |
| B3 foreground killed 0 times | PASS | 0 |
| B4 audio + download exceptions | PASS | both alive and unfrozen |
| B5 killer priority order | PASS | 6 kills, all band 6 (hibernated) first |
| G-PM2 inactive ≤ 25% | PASS (model) | |
| G-PM3 hibernated ≤ 5 MB | model PASS (4.2 MB) | cgroup number needs hardware |
| G-PM4 foreground safety | PASS | 0 |
| B2 restore ≤ 500 ms | not measured | needs AetherOS + cgroup + real apps |
| B6 full-pressure share < 5% | not measured | needs PSI on reference machines |

## Honest limits

- All mechanism tests use a fake cgroup tree or dry-run mode. The real
  freeze/thaw-on-a-process test, G-PM1 memory figures, G-PM3 cgroup values and
  B2/B6 require the 8 GB and 4 GB reference machines (04 §10); they are not
  claimed here.
- The benchmark harness runs real client processes but models per-app memory
  because macOS has no cgroup accounting. The report labels model results.
- Activity Manager UI (P5-T07) and the control-center battery view (P5-T14)
  are separate crates/tasks; the daemon exposes the metrics they consume.

## Reproduce

```bash
cargo test -p aether-lifecycled
cargo run -p aether-lifecycled -- --simulate
cargo run -p aether-lifecycled --bin aether-bench -- --apps 50 --no-spawn \
  --report /tmp/p5-t08.md
```
