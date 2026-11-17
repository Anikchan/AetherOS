# Addendum 2 — Android Apps, Activity Manager, Battery Analytics, and Optional Packs (2026-11-17)

Parent plan: `docs/2026/11/16/os-project/` (index: `docs/2026/11/16/os-project/README.md`). Addendum 1: `docs/2026/11/17/os-addendum-1/`.

This folder adds Android app support, a Windows-style Activity Manager with full metrics, battery analytics and screen time, and the optional-feature model that keeps heavier features out of the core.

## Short Answers

### Android apps: can the three requirements be met?

| Requirement | Answer |
| --- | --- |
| 1. The OS is natively running the app | Yes, in design. The container runs on the host kernel (no emulator). Each app gets its own window with our title bar and appears in the launcher, dock, and Activity Manager. The Android home screen is hidden. Per-app windows must be proven in spike S6 first. If that fails, the fallback is one Android window, which is a clear limit. |
| 2. No delay or performance problems | Partly. Warm launch target 1.5 s for x86 apps. Cold start of the container takes seconds and is hidden behind stage messages. Keep-ready mode removes the cold start but keeps about 150 MB of memory. ARM-only apps are slower because they need translation (below). Targets are measured in gates G-AND1 to G-AND4. |
| 3. The OS performance is not impacted | Yes, by design and measured. Memory is capped (default 1.5 GB), CPU and disk get low priority, and the container freezes when unfocused. Native apps keep priority. Gate G-AND5 checks that native apps do not stutter while an Android app runs in the background. |

Two hard limits must be accepted before Android is installed (F12 section 2):
1. Most Google Play apps are ARM-only. Intel and AMD PCs need a translation layer. The available ones (libhoudini, libndk) are proprietary, so the OS cannot ship them. For personal use, the owner may supply one (AND-15). Public images will not include one.
2. The Google Play Store and Google Play Services are not available on a generic Android system. microG (open source) covers many features but not all. Streaming apps that need Widevine L1 may not work well.

### Activity Manager with full metrics

- Yes. It has Windows-style tabs (Processes, Performance, App history, Startup apps, Users, Details, Services) and a Battery and Sensors view for macOS-style energy and temperature data.
- It covers CPU, memory, disk, network, GPU, temperature, fans, battery, and energy impact.
- Ctrl+Shift+Esc opens it. End task, Efficiency mode, and Force Quit are included.
- Overhead: about 1% of one core while open; under 0.2% average while closed (G-TM1). Accuracy is checked against reference tools within 5% (G-TM2).
- Per-process GPU use comes from the kernel's DRM usage data. It works for Intel and AMD drivers that publish that data. Missing values show as "not reported".
- The old "Activity" app is replaced by Activity Manager (owner question Q20).

### Battery cycles, trends, and screen time

- **Cycles:** Shown when the battery reports a cycle count. If not, the screen says so and shows an estimate labeled as an estimate.
- **Trends:** Daily health history for years, sessions for 30 days (longer in the reports pack), daily temperature range, charging habits, and standby drain.
- **Screen time:** Screen-on time, time per app, and pickups. Stored locally. Limits and downtime are an optional pack.
- **Storage:** About 40 MB per year before rollup, about 5 MB per year after rollup.

### macOS-style and other features: will they make the OS heavy?

Not if they are optional. Heavier features are optional packs (F15), installed from Settings → Features, like Windows Optional Features. Each pack shows its disk size, memory, idle cost, and battery effect before installing. A pack that is not installed costs nothing.

| Always in core (light) | Optional packs (heavier) |
| --- | --- |
| Activity Manager (basic), battery status and history, screen time (basic), process management, default apps, power management | A. Android apps, B. Windows apps, C. AI assistant, D. Advanced monitoring (per-app network, per-process GPU), E. Screen Time limits, F. Battery reports, G. Storage analyzer |

Gate G-OPT1 checks that the core stays inside the idle budgets in F6.

## Decisions Needed (Q18 to Q22)

| ID | Question | Proposed answer |
| --- | --- | --- |
| Q18 | Accept the Android limits (ARM-only apps need translation; no Google Play Store; Widevine limits)? | Yes, with the limits shown in the store |
| Q19 | Android start mode: lazy (no memory cost until used) or keep-ready (faster, about 150 MB frozen memory)? | Lazy by default; keep-ready as a setting |
| Q20 | Name the monitoring app "Activity Manager" and replace "Activity"? | Yes |
| Q21 | Which optional packs should be on by default after install? | None; owner chooses |
| Q22 | Put Android in Personal Edition before M5 (adds about 620 hours; M5 moves to about June 2030) or after M5 as the first pack? | After M5 |

## Plan Impact

| Change | Hours |
| --- | --- |
| Activity Manager replaces Activity (P5-T07 expanded) | +130 |
| Battery analytics and screen time (P5-T16, P5-T17) | +120 |
| Optional feature manager (P7-T07) | +60 |
| Android pack (P6-AND, after M5) | +620 |
| Optional packs D to G (P7-PK, after M5) | +160 |
| Default browser (owner answer Q16; P4-T18 family) | +120 |

Personal Edition core (with Activity Manager, battery analytics, and screen time): 4,250 hours, about 163 weeks. Approximately January 2030 without buffer, about December 2030 with a 30% buffer. Android is added after M5 by default; if it is included before M5 (Q22), M5 moves to about June 2030 without buffer.

Full plan including Android, optional AI, default browser, and public release: 5,620 hours, about 216 weeks. Approximately January 2031 without buffer.

Details: `docs/2026/11/16/os-project/03-roadmap-plan.md`, which has been updated to match.

## Documents in This Folder

| Document | Purpose |
| --- | --- |
| [F12-android-app-support.md](F12-android-app-support.md) | Android app support: the three requirements, limits, features, targets |
| [F13-task-manager.md](F13-task-manager.md) | Windows-style Activity Manager with full metrics |
| [F14-battery-analytics-and-screen-time.md](F14-battery-analytics-and-screen-time.md) | Battery cycles, health trends, sessions, screen time |
| [F15-optional-features.md](F15-optional-features.md) | Optional packs and the core light set |
| [D9-android-runtime-design.md](D9-android-runtime-design.md) | Android container architecture, lifecycle, contracts, resource policy |
| [D10-monitoring-design.md](D10-monitoring-design.md) | Metric sources, sampling, storage, overhead, gates |

## Spikes Added

| Spike | Question | When |
| --- | --- | --- |
| S6 | Can the Android container show each app in its own window with our decorations? Kernel binder support present? Container memory baseline and warm-start time? | P6-AND-T01 |
| S7 | Are GPU, temperature, fan, and battery values readable on the owner's machines? | P5-T07 (start) |
| S8 | Does the owner's battery report cycle count and temperature? | P5-T16 (start) |

## New Gates (Summary)

| Gate | Area | Pass value |
| --- | --- | --- |
| G-AND1 | Android launch | Warm launch ≤ 3 s (typical x86 app); cold start ≤ 15 s with progress |
| G-AND2 | Android resource cap | Container never exceeds its cap; host stays at Normal pressure when only the container is at cap |
| G-AND3 | Android frozen state | Frozen memory ≤ 250 MB; restore ≤ 3 s |
| G-AND4 | Android frame rate | ≥ 45 fps (limit), 55 fps (target) for a typical x86 app |
| G-AND5 | Host isolation | Native foreground app keeps frame gaps under 1 s while an Android app runs |
| G-TM1 | Activity Manager overhead | See D10 section 8 |
| G-TM2 | Activity Manager accuracy | Within 5% of reference tools |
| G-BAT4 | Cycle count | Matches the system report when reported |
| G-BAT5 | History continuity | At least 99% of minutes recorded over 7 days |
| G-ST1 | Screen time | Within 1 minute per day of the focus log |
| G-OPT1 to G-OPT4 | Optional packs | See F15 section 5 |

## Open Questions (Carried)

Q8 to Q24 were answered by the owner on 2026-11-17 and are recorded in `docs/2026/11/16/os-project/README.md`. No open questions remain at this time.
