# D6 — Verification Framework: How We Prove the Claims

This document answers: "How are we making sure?" for process management, battery life, speed, and hang prevention. Rule: no claim is made until a gate with a number, a method, a machine, and a date has passed.

## 1. Principles

1. **Every claim has a gate.** A gate is a measurable condition with a pass value, a test method, and a record.
2. **Compare with a baseline.** Results are shown against stock Linux with no lifecycle management and against Windows 11 on the same machine. macOS cannot be measured on our hardware, so it is not used as a baseline.
3. **Measure on real hardware.** Virtual machines are used for regression checks. Claims about battery, speed, and memory use real machines.
4. **Record everything.** Each run records the command, machine, kernel version, date, and values. Results go into the results log (section 7).
5. **Estimates are labeled.** A value measured with a tool is labeled Measured. A value taken from a model or a calculation is labeled Estimate.
6. **No self-certification.** A result counts only after the owner or a test script records it. Code review alone does not count.

## 2. Claims and How They Are Proven

| Question | Claim we make | Gate(s) | Method | Evidence |
| --- | --- | --- | --- | --- |
| Q1: Inactive apps use little memory and CPU, like iOS | Inactive apps are suspended or hibernated and use little memory and no CPU | G-PM1, G-PM2, G-PM3 | 50-app benchmark (04 section 4); 72-hour soak test; hibernated app RAM test | Results log |
| Q1: Apps are kept open instead of closed | Apps are closed only under Critical pressure | G-PM4 | Pressure log during benchmark; kill count and reasons | Kill log |
| Q2: Longest possible battery life | Battery life within 15% of Windows 11 on the same laptop; idle drain ≤ 6%/h | G-BAT1, G-BAT2 | Battery benchmark (04 section 11.2) | Battery log |
| Q2: Fastest app opening | Launch targets in F11 section 2 | G-SPD1, G-SPD2 | Timed launches (20 per app, median) | Launch log |
| Q2: Fast boot | Boot targets in F11 section 1 | G-SPD0 | Timed boots (10 per target, median) | Boot log |
| Q2: No hangs unless full crunch | No UI gap over 1 s outside Critical pressure | G-NH1, G-NH2 | 24-hour daily-use test with frame-gap logger | Frame-gap log |
| Q4: Size and resource use | Image size, idle RAM, idle CPU, AI footprint | G-RES1 to G-RES4 | Build check; idle measurement | Build and idle logs |
| Q3: No user-visible third-party app | Default image contains no user-visible third-party app | G-APP1 | File-list check in build (F9 AP-AC-07) | Build log |
| New: default apps | Launch and idle targets in F8 | G-DA1 | Timed launches; idle check | Launch log |
| New: optional AI | Zero footprint when not installed; unloads when idle; no network traffic | G-AI1 to G-AI3 | Process, memory, and network checks | AI log |

## 3. Gate Definitions

| Gate | Pass value | Method | Frequency |
| --- | --- | --- | --- |
| G-SPD0 Boot | Bootloader to desktop median ≤ 10 s; limit 15 s | 10 timed boots on reference SSD machine | Each release candidate; monthly |
| G-SPD1 App launch | Default apps median ≤ 500 ms cold | 20 timed launches per app | Each shell or app change |
| G-SPD2 Restore | Hibernated app restore ≤ 500 ms (under 10 minutes hibernated) | 20 timed restores | Each process manager change |
| G-PM1 50-app benchmark | Pass criteria B1 to B6 (04 section 4.3) | Benchmark script `lifecycled/bench/` | Each process manager change; before M4 |
| G-PM2 Inactive memory | Inactive apps hold ≤ 25% of used memory after 5 minutes (B1); stretch goal 10% for simple test apps | Benchmark measurement | Same as G-PM1 |
| G-PM3 Hibernated size | Hibernated test app ≤ 5 MB resident | cgroup `memory.current` | Each change |
| G-PM4 Foreground safety | Foreground app killed 0 times in all tests | Kill log | Every run |
| G-PM5 Soak test | 72 hours with 50 apps, random switching; no foreground kill; memory returns to baseline after each cycle | Soak script | Before M4 and before M5 |
| G-BAT1 Battery versus Windows | W2 and W3 life within 15% of Windows 11 (median of 3 runs) | Battery benchmark (04 section 11.2) | Before M5; after each power change |
| G-BAT2 Idle drain | ≤ 6%/h limit; ≤ 4%/h target | W1 (1 hour idle) | Before M5 |
| G-BAT3 Profile switching | Within 5 s of AC change (PWR-AC-01) | Timed event log | Each power change |
| G-NH1 Hang frequency | Zero gaps over 1 s outside Critical pressure in a 24-hour daily-use test | Frame-gap logger (NH-07) | Before M3 and before M5 |
| G-NH2 Hang recovery | "Not responding" shown within 6 s; shell restart within 5 s without losing windows | Test app and kill tests | Each shell or compositor change |
| G-RES1 Image size | ≤ 512 MB; target ≤ 400 MB, total budget in D4 | Build output and budgets file | Every build |
| G-RES2 Idle RAM | ≤ 300 MB on 4 GB machine (target 250 MB) | `free`, `smem` after 5 minutes idle | Weekly |
| G-RES3 Idle CPU | < 2% average over 5 minutes (target < 1%) | `top` sampling | Weekly |
| G-RES4 Installed size | Base install ≤ 2 GB (limit 3 GB), without optional engines or AI | Disk usage after install | Each release candidate |
| G-APP1 No third-party app | Default image contains no user-visible third-party app | File-list check in build | Every build |
| G-AI1 Zero when absent | No AI process, file, or memory when not installed | Process and memory check | Before M5 |
| G-AI2 Unload when idle | Memory returns to the unloaded level within 2 minutes after the last answer | Memory sampling | Each AI change |
| G-AI3 Local only | No outgoing network connection during an answer | Network monitor during answer | Each AI change |
| G-DA1 Default apps idle | 0 MB and 0% CPU when closed | Process check | Each app change |

## 4. Telemetry (Local Only)

- The lifecycle, power, and shell services write a metrics log to `/var/log/nexa/metrics/` (system) or `~/.local/state/nexa/metrics/` (user).
- Events recorded: app state changes, memory per app, restore times, kill events with reasons, power source and profile changes, battery percentage, frame gaps, launch times, boot stage times.
- Storage: ring buffer capped at 50 MB. Oldest records are removed first.
- Nothing is sent off the machine. Export is manual, through `nexactl metrics export`.
- Metrics use a fixed text format, so the owner can read them without tools.

## 5. Automation

| Item | Where | When it runs |
| --- | --- | --- |
| Budgets file (image size, boot time, idle RAM) | `os-build/budgets.toml` | Every build in CI; fails the build when a limit is exceeded |
| Boot test in a virtual machine | `os-build/tests/boot.sh` | Every build |
| Frame-gap and launch timing | Built into the shell (metrics log) | Always on; sampled |
| 50-app benchmark | `lifecycled/bench/` | Manual on the reference machine, then nightly when set up |
| Battery benchmark | `powerd/bench/` | Manual; runs 3 hours per full set |
| Soak test | `lifecycled/bench/soak.sh` | Manual before M4 and M5 |

## 6. Limits of the Claims

- A parity claim with iOS cannot be proven in the strict sense, because iOS and Linux differ. We prove measured results against defined gates.
- Battery life depends heavily on hardware firmware. Some laptops may get less battery life on Linux than on Windows. If G-BAT1 fails on a machine, the result is recorded and the cause is investigated (power features missing, driver issue, or app behavior). It is not hidden.
- Windows program performance depends on the Windows program. Each app is measured separately.
- Results from virtual machines do not count for battery, speed, or hang claims.

## 7. Results Log Format

Each run is written as one entry in `docs/<year>/<month>/<day>/results/<gate-id>.md`:

```
Gate: G-SPD1 App launch
Date: 2026-MM-DD
Machine: <model, CPU, RAM, SSD>
Kernel: <uname -r>
Command: <exact command>
Values: <list of measured numbers>
Result: Pass | Fail | Estimate only
Notes: <cause of any failure, next step>
```

## 8. Process

1. Before a milestone, the owner runs the gates marked for that milestone (03 section 2).
2. Each failed gate becomes a task in the plan with the gate ID.
3. A milestone is not marked Done until its gates show Pass in the results log.
4. Retries: a gate may be re-run at most twice after a failure before the failure is reported as blocked (team retry rule).
