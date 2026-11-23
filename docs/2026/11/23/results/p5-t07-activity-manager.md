# P5-T07 — Activity Manager core + spike S7 (sensors and GPU)

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run
- Scope: `apps/taskmanager/`
- Design: F13, D10, 04 §4; gates G-TM1/G-TM2

## Verified

### Tests

```bash
cargo test -p aether-taskmanager
```

Result: **26 unit tests pass, 0 failed** (5 consecutive runs).

| Area | Tests |
|---|---|
| Process parsing | `/proc/<pid>/stat` (name with spaces, state, ppid, utime/stime, threads); status (uid, VmRSS, wakeups); cmdline with NULs; io counters |
| Sampling | real fixture process row with CPU delta, memory, disk, user; previous-PID pruning |
| System | meminfo → bytes; net/dev skips loopback/veth; CPU tick deltas → busy % (75% case); per-core tick parse |
| Sensors | hwmon temperature + label, fan RPM, thermal zones; battery view (percent, health, cycles, temp) |
| GPU | `gpu_busy_percent` and `mem_info_vram_used` from DRM |
| Model | sorting by every column; energy classification; Android tab visibility; 60 s history window |
| Manager | end-task safety (PID 1, self), dry-run, efficiency on/off, startup list + persistent disable, CSV/JSON/text export, app-scope ID extraction, lifecycle state merge by PID, energy-impact column from analytics, systemctl/loginctl parsing, idempotent `.desktop` toggling |
| Spike S7 | Pass (sensors + GPU), Partial (sensors only), Fail (nothing), fan-only fallback |

### Spike S7 on this host

```bash
cargo run -p aether-taskmanager -- --spike-s7
```

Result: **Fail**, correctly:
- no hwmon/thermal sensors (`/sys` is not a Linux sysfs on macOS),
- GPU busy/VRAM not exposed,
- fallbacks printed: Sensors shows "Not reported by this system", GPU section
  "Not reported", per-process GPU stays in Pack D.

Parser validation used fixture sysfs trees (Pass/Partial/Fail/fan-only), so the
probe is ready for the hardware run; the verdict itself must be re-run on an
AetherOS machine. No hardware finding is claimed.

## Acceptance criteria (F13 §8)

| ID | Status | Notes |
|---|---|---|
| TM-AC-01 Ctrl+Shift+Esc opens in 500 ms | **pending UI** | core start is a single CLI run; the Slint window and shortcut are not wired |
| TM-AC-02 Processes tab shows every column | **core done** | CPU/memory/disk/GPU/energy/wakeups columns; per-app network stays Pack D |
| TM-AC-03 End task closes app + processes | **core done** | SIGTERM/SIGKILL on pid; process-tree kill uses the app scope; unsaved warning comes from the lifecycle SDK |
| TM-AC-04 values within 5 % of reference (G-TM2) | **not measured** | needs the Dell run |
| TM-AC-05 temperatures within 2 °C | **not measured** | needs real sensors |
| TM-AC-06 GPU activity during GPU test | **not measured** | needs real GPU |
| TM-AC-07 overhead ≤ 1 % while open (G-TM1) | **not measured** | sampling is 1 s and detail reads are opt-in; measure on hardware |
| TM-AC-08 export readable text + CSV | **pass** | tested |
| TM-AC-09 efficiency lowers CPU share | **core done** | nice + `cpu.weight`; measured effect needs a CPU test on hardware |
| TM-AC-10 startup disable persists | **pass (file level)** | `.desktop` flags persist; reboot check on hardware |

## Honest limits

- The Slint window (S3 `ui/` design system), the keyboard shortcut, the
  unsaved-work confirmation dialog and the force-quit flow are not built here;
  they are thin calls into this core.
- `/proc`/`/sys` are Linux-only; the macOS result above is "no data", not a
  passing sensor probe.
- G-TM1/G-TM2 and TM-AC-04/05/06/07 require the reference machines (04 §10).

## Reproduce

```bash
cargo test -p aether-taskmanager
cargo run -p aether-taskmanager -- --spike-s7
cargo run -p aether-taskmanager -- --once --json --limit 5
```
