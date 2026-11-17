# F14 — Battery Analytics, Battery History, and Screen Time

Owner: `nexa-powerd` (battery data and history), shown in Activity Manager (F13) and the Battery settings page. Design: D5 and D10.

## 1. Goal

Show the battery's health and how it is used over days, weeks, and years. Show how long the screen is on and which apps use the time. Keep everything local. The basic version is part of the core OS. Detailed reports are an optional pack (F15).

## 2. Feature List

### 2.1 Battery (core)

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| BAT-01 | Cycle count | MVP | Shows the cycle count the battery reports. If the hardware does not report it, the screen says "Not reported by this battery" and shows the estimate in BAT-03 labeled as an estimate. |
| BAT-02 | Health trend | MVP | Full charge capacity compared with design capacity, as a percentage, with a chart over months and years. Each point is recorded once per day. |
| BAT-03 | Estimated cycles from use | MVP | Total energy discharged divided by full capacity, labeled "Estimate". Used when BAT-01 is missing. |
| BAT-04 | Charge and discharge sessions | MVP | Each session: start and end time, start and end percentage, average power, and energy used. Last 30 days in core; longer history in the reports pack (F15 Pack F). |
| BAT-05 | Battery temperature history | MVP | Daily minimum, maximum, and average (where the battery reports temperature). |
| BAT-06 | Charging habits | v1 | Time spent above 80%, number of times charged to 100%, and charge-limit use. Shows the facts; does not give medical advice. |
| BAT-07 | Time-remaining accuracy | v1 | Compares the estimate from F7 with the real time left, per session. |
| BAT-08 | Energy by app | MVP | Energy impact per app for the last 24 hours and 7 days (uses F7 PWR-12). |
| BAT-09 | Standby drain | MVP | Battery used while the lid is closed or the screen is off (per night and per day). |
| BAT-10 | Power report export | v1 | Text report of BAT-01 to BAT-09 for a chosen period (F7 PWR-14). |

### 2.2 Screen Time (core, basic)

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| ST-01 | Screen-on time | MVP | Total time the screen was on, today and for the last 7 days, and per day. |
| ST-02 | Time per app | MVP | Focused time for each app (the time it was the active window with the screen on). |
| ST-03 | Pickups | v1 | Number of times the screen was turned on or unlocked per day. |
| ST-04 | Notifications count | v1 | Number of notifications received per app per day. |
| ST-05 | Limits | Later (Pack E) | Daily time limits per app, with a notice and an option to extend. |
| ST-06 | Downtime | Later (Pack E) | Scheduled quiet hours that close chosen apps. |

Screen Time data comes from the compositor (focus and screen events) and the power daemon (screen state). It is stored locally. Nothing is sent off the machine.

## 3. Data Storage

| Data | Resolution | Retention | Approximate size per year |
| --- | --- | --- | --- |
| Battery health and cycles | One record per day | 5 years | Under 100 KB |
| Battery percentage and power | 1 minute | 30 days | About 40 MB, then rolled up |
| Battery percentage and power after rollup | 15 minutes | 2 years | About 5 MB |
| Sessions | One record per session | 2 years | Under 1 MB |
| Screen time per app | One record per app per day | 2 years | Under 5 MB |
| Energy per app | 1 minute, rolled up to 1 day | 90 days | About 10 MB |

Storage and sampling are defined in D10. Every chart can be exported (TM-19).

## 4. Accuracy and Honesty Rules

1. Values come from the battery and the kernel. When a value is missing, the screen says so; it never shows zero for missing data.
2. Estimates are always labeled "Estimate".
3. Health and cycle values can differ between firmware. The screen shows the source of each value.
4. History gaps (for example, while the machine was off) are shown as gaps, not as zero use.

## 5. Dependencies

- F7 battery policy and telemetry (P5-T11, P5-T14).
- F13 Activity Manager (the Battery tab and App history tab).
- D10 storage and sampling.

## 6. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| BAT-AC-01 | Cycle count matches the value in the system battery report when the hardware reports it (G-BAT4) | Not tested |
| BAT-AC-02 | Health trend shows one point per day with no gaps longer than one day while the machine is on (G-BAT5) | Not tested |
| BAT-AC-03 | Sessions list matches the battery percentage changes within 1% | Not tested |
| BAT-AC-04 | Energy by app shows the top app during a known test workload | Not tested |
| BAT-AC-05 | Standby drain matches the battery percentage change over 8 hours of closed lid within 1% | Not tested |
| ST-AC-01 | Screen-on time matches the compositor focus log within 1 minute per day (G-ST1) | Not tested |
| ST-AC-02 | Time per app adds up to the screen-on time within 1 minute | Not tested |
| ST-AC-03 | Data for a missing period shows as a gap, not as zero | Not tested |

## 7. Open Questions

- Whether the 30-day minute-level history (about 40 MB per year before rollup) is acceptable on a 32 GB disk. Proposed: yes.
- Whether pickups (ST-03) and notification counts (ST-04) should be core or pack. Proposed: v1 core.
