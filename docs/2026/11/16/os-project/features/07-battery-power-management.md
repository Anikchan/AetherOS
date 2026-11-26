# F7 — Battery and Power Management

Owner component: `aether-powerd` (power policy). Mechanisms come from the Linux kernel, UPower, and power-profiles-daemon. Design: D5.

Battery management is a top priority (03 and 01 section 2). The goal is the longest battery life possible without manual tuning, with battery health protected and no loss of responsiveness when plugged in.

## 1. Goals

1. Battery life close to the best the hardware can deliver, measured against Windows on the same machine and workload.
2. Automatic behavior that the user never has to manage.
3. Battery health protected by charge limits where the hardware allows.
4. Accurate time-remaining estimates.
5. No loss of responsiveness on AC power.
6. Every power decision is visible and explained in the power view.

## 2. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| PWR-01 | Power profiles | MVP | Three profiles: Performance, Balanced, Saver. Set through the platform profile and CPU energy preference (EPP). |
| PWR-02 | Automatic profile switching | MVP | On battery, the default is Saver. On AC, the default is Balanced. Both are configurable. |
| PWR-03 | Device runtime power management | MVP | Turn on power saving for PCIe links (ASPM), USB autosuspend, Wi-Fi power save, audio codec power save, SATA link power, and GPU runtime power management where supported. Uses an exclusion list. |
| PWR-04 | Idle CPU efficiency | MVP | Tickless idle and deep CPU idle states enabled in the kernel. Timer coalescing for background services. |
| PWR-05 | Background activity limits on battery | MVP | Shorter grace period before suspend; earlier hibernation; lower CPU weight for background apps (F2 PRM-12). |
| PWR-06 | Display power | MVP (dim and off), v1 (refresh rate) | Dim after idle, turn off after longer idle. Variable refresh rate or lower refresh rate on battery where the panel supports it. |
| PWR-07 | Battery status | MVP | Percentage, charging state, time to empty or full (smoothed), power draw in watts, cycle count, and health (full capacity vs design capacity) where the kernel reports them. |
| PWR-08 | Charge limit | MVP where supported | Stop charging at 80% or 100%, if the hardware exposes the control. Hidden on machines without support. |
| PWR-09 | Low-battery actions | MVP | Notify at 20% and 10%. Switch to Saver at 10%. At 5%, suspend or hibernate (configurable). Hibernate at 3% if not charging. |
| PWR-10 | Sleep policy | MVP (suspend-to-idle), v1 (suspend-then-hibernate) | Suspend-to-idle by default. Optional: suspend first, then hibernate after a set time, to save battery during long sleep. Needs a disk swap file (D5 section 8). |
| PWR-11 | Thermal management | MVP (basic), v1 (fan curves) | Keeps temperatures in range while saving power. Intel thermal daemon where used; platform fan control where supported. |
| PWR-12 | Energy impact per app | v1 | Relative energy score in the activity monitor, from measured energy per app (F14) when available, else CPU time, wakeups, and GPU use. The column contract (labels, thresholds, tones, 60 s refresh) is printed by `aether-taskmanager --energy-contract`. |
| PWR-13 | Wakeup control | v1 | Flags apps that wake the CPU often; limits their timers in background. |
| PWR-14 | Power report | v1 | `aetherctl power report` writes a text report: drain sources, profile history, sleep events, and battery health. |
| PWR-15 | Low-power mode switch | MVP | One toggle in the control center. Sets Saver and reduces effects (F6 tiers). |
| PWR-16 | Battery and power view | MVP | Shows status, profile, time remaining, and why the current profile was chosen. |
| PWR-17 | Optimized charging | Later | Delay full charge until shortly before typical unplug time. Needs usage history. |
| PWR-18 | Battery analytics and screen time | MVP (basic), Pack F (reports) | Cycles, health trend, sessions, energy by app, standby drain, and screen time. Details in addendum 2 F14. |

## 3. Profile Behavior

| Setting | Performance | Balanced | Saver |
| --- | --- | --- | --- |
| CPU energy preference | Performance | Balance performance | Power saving |
| Platform profile | Performance | Balanced | Low power |
| Background grace period | 60 s | 30 s | 10 s |
| Hibernation timeout for suspended apps | 30 min | 10 min | 3 min |
| Display dim after idle | 5 min | 3 min | 1 min |
| Effects tier (F6) | Full | Full or Reduced by GPU | Reduced |
| Runtime power management for devices | Off for PCIe and USB; on for Wi-Fi | On | On, all listed devices |

Exact values are proposed. They are tuned on the owner's machines with the battery benchmark (04 section 11.2).

## 4. Battery Time Estimate

- Show "Estimating..." for the first 2 minutes after a change of power source.
- Smooth the power draw with a moving average over 2 to 5 minutes.
- Do not show a value that changes by more than 10% between readings unless the change is a real load change (for example, video started).
- Use the full charge capacity reported by the kernel, not the design capacity, for estimates.

## 5. Sleep and Resume

- Default: suspend-to-idle. Lid close sleeps. Lid open resumes to the lock screen.
- Optional: suspend-then-hibernate after a set time (PWR-10). Requires a disk swap file at least as large as RAM. zram cannot be used for hibernation.
- Resume target: desktop ready in 3 seconds (PT-08 in 04).
- Wi-Fi reconnects automatically after resume.
- A failed resume is logged, and the next boot reports the reason.

## 5.1 Implementation note (spike S4, P0-T10, 2026-11-18)

Battery telemetry through UPower/sysfs (cycle count, charge limits) is **blocked
on real hardware** (a machine with a battery; the spare Dell or a laptop VM
passthrough). The design retains UPower-first with a sysfs fallback (D5
section 10) and a "not reported" display for missing values (risk R15). No
battery numbers are recorded yet; spike S4 lands in P2 bring-up.

## 6. Dependencies

- P5 (process and power management phase).
- F2 process manager (background limits and hibernation).
- F6 effect tiers (Saver uses Reduced).
- D4 partition plan (swap file for hibernation).
- Hardware checklist items in 04 section 6 (battery, sleep, fan).

## 7. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| PWR-AC-01 | Profile switches to Saver within 5 s of unplugging, and back to Balanced within 5 s of plugging in | Not tested |
| PWR-AC-02 | Battery percentage matches the system's own battery tool within 2% | Not tested |
| PWR-AC-03 | Time-remaining estimate is within 10% of the actual remaining time after 10 minutes of steady use | Not tested |
| PWR-AC-04 | Charge limit set to 80% stops charging at 80% (where supported) | Not tested |
| PWR-AC-05 | At 10% battery, the system switches to Saver and notifies; at 5%, it runs the configured action | Not tested |
| PWR-AC-06 | Idle drain ≤ 6% per hour on the reference laptop (target 4%) (04 section 11.2) | Not tested |
| PWR-AC-07 | Battery benchmark within 15% of Windows 11 on the same laptop and workload (04 section 11.2) | Not tested |
| PWR-AC-08 | Device tuning reverts automatically when a device reports errors after tuning | Not tested |
| PWR-AC-09 | Suspend and resume succeed for 10 of 10 cycles | Not tested |
| PWR-AC-10 | Power view shows the reason for the current profile | Not tested |

## 8. Open Questions

- Whether suspend-then-hibernate is needed in Personal Edition or can wait. Proposed: Later unless the owner's laptop drains too quickly in sleep.
- The default low-battery action at 5%: suspend or hibernate. Proposed: hibernate if a swap file exists; suspend otherwise.

## 9. Implementation note (2026-11-26)

PWR-12 is wired end to end: `powerd` records measured energy per app (F14
`energy_by_app`), `aether-taskmanager` ranks rows by that share and falls back
to the CPU/wakeup/GPU score, and the window binds to the published contract
(High ≥ 66 %, Medium ≥ 33 %, Low > 0 Wh, None; tones `danger`/`accent`/
`success`/`text_muted`; 60 s refresh). Hardware energy numbers still need the
owner's laptop (G-BAT1/G-BAT2).

