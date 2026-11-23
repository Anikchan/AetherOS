# P5-T08 — 50-app benchmark / soak result

- Date: 1795407038
- Host: macos aarch64
- Apps per cycle: 50
- Cycles: 1
- Method: real client processes + daemon/policy, modelled cgroup memory (cgroup accounting needs Linux)

## Checks

| Check | Result | Detail |
|---|---|---|
| B1 inactive memory ≤ 25% (cycle 1) | PASS | 10.8% (197.1 MB of 1832.9 MB) |
| B2 hibernated restore ≤ 500 ms (cycle 1) | not measured | Needs AetherOS hardware (cgroup + real apps) |
| B3 foreground killed 0 times (cycle 1) | PASS | 0 |
| B4 audio + download exceptions (cycle 1) | PASS | true |
| B5 killer priority order (cycle 1) | PASS | kills: org.aether.bench.App03:6, org.aether.bench.App04:6, org.aether.bench.App05:6, org.aether.bench.App08:6, org.aether.bench.App09:6, org.aether.bench.App10:6 |
| B6 full-pressure share < 5% (cycle 1) | not measured | Needs PSI on the reference machines |
| G-PM2 inactive ≤ 25% (cycle 1) | PASS | |
| G-PM3 hibernated ≤ 5 MB (cycle 1) | model PASS | 4.2 MB modelled |
| G-PM4 foreground safety (cycle 1) | PASS | 0 foreground kills |

## Evidence rules

- B1, B3, B4, B5, G-PM2 and G-PM4 are measured by the real policy/daemon against real client processes; memory accounting is modelled at 20 MB per app (04 §4.1).
- B2, B6, G-PM1 memory figures and G-PM3 cgroup figures must be re-measured on the 8 GB and 4 GB reference machines; results from this host do not count for the M4 exit criteria (04 §10, D6).

## Re-run with real client processes (2026-11-23)

The same harness with `--apps 50` (no `--no-spawn`) launches 50 real processes
and registers their PIDs with the daemon. Result:

```text
cycle 1: apps=50 inactive=10.8% b1=PASS b3=PASS b4=PASS b5=PASS gpm2=PASS gpm4=PASS kills=6
```

Harness fix found during the run: child processes inherited the harness
stdout/stderr, so a piped invocation waited for the children. Children now use
`Stdio::null()`. A compressed soak run (`SOAK_HOURS=0.01`) also completes and
writes `p5-t08-soak.md`; the full 72-hour run still needs the reference machine.
