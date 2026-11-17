# F2 — Process Management (iOS-Style)

Owner component: `nexa-lifecycled`. Detailed design: D3.

## 1. Goal

Keep many apps open at once while inactive apps use almost no memory and no CPU. Protect the foreground app. Under memory pressure, end background apps in a fixed priority order, the way iOS does with jetsam.

How each claim in this feature is proven (gates and results): `docs/2026/11/17/os-addendum-1/D6-verification-framework.md`.

## 2. App Lifecycle States

| State | Meaning | Memory | CPU |
| --- | --- | --- | --- |
| Foreground | Focused window | Full | Full |
| Visible | Shown but not focused (for example, a side-by-side window) | Full | Full |
| Background | Not visible, still running | Full (may be reclaimed) | Limited by weight |
| Suspended | Frozen with `cgroup.freeze` | Full in RAM until reclaimed | None |
| Hibernated | Frozen, and most memory is compressed in zram or moved to swap | Small resident set | None |
| Terminated | Ended by the system or by the user | None | None |

State rules are in D3 section 3.

## 3. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| PRM-01 | Lifecycle states | MVP | Six states above, tracked for every app. |
| PRM-02 | Per-app isolation | MVP | Each app runs in its own cgroup scope. |
| PRM-03 | Auto-suspend | MVP | Background apps suspend after a grace period. Default 30 s on AC power, 10 s on battery. Both are configurable. |
| PRM-04 | Hibernation | MVP | Suspended apps reclaim memory after 10 minutes by default, using compressed memory (zram). |
| PRM-05 | Pressure killer | MVP | When memory pressure rises, the lowest-priority app is terminated first (D3 section 6). |
| PRM-06 | Lifecycle notifications | MVP | Apps receive "will suspend", "did resume", and "memory warning" messages. Apps can save state. |
| PRM-07 | Background exceptions | MVP | Audio playback, file downloads, VPN, calls, and navigation are not suspended. Apps request these through portals. |
| PRM-08 | Background task budget | v1 | Apps request scheduled wake windows (for example, mail sync every 15 minutes). The budget limits total background time. |
| PRM-09 | Per-app limits | v1 | User can set CPU weight and memory ceiling for an app. |
| PRM-10 | Activity monitor (now Activity Manager, addendum 2 F13) | MVP (basic), v1 (full) | Lists apps, state, memory, CPU, and reason for any exception. Allows manual suspend, hibernate, and quit. |
| PRM-11 | State restoration | v1 | Apps that support it restore their window and document state after termination. |
| PRM-12 | Power-aware behavior | v1 | On battery, the grace period is shorter and hibernation starts sooner. Set by `nexa-powerd` (F7 PWR-05, D5). |
| PRM-13 | Pinned apps | MVP | Apps the user pins are never terminated by the pressure killer (they can still be hibernated). |

## 4. Rules

1. The foreground app is never suspended, hibernated, or terminated by the system.
2. An app with an active exception (PRM-07) is not suspended. Its processes keep running. Its memory can be reclaimed under pressure, except for audio playback and calls.
3. System services (compositor, shell, lifecycle daemon, audio server, network) are never terminated by the killer.
4. The user can always force-quit an app from the activity monitor.
5. Every system action is logged with its reason (for example, "hibernated: idle 10 min").

## 5. Dependencies

- Linux cgroup v2, PSI, zram (kernel configuration in D4).
- Wayland focus events from P3 compositor.
- Portals for exceptions (Background portal and audio stream state).

## 6. Known Risks

| Risk | Mitigation |
| --- | --- |
| Frozen apps lose network connections | Exceptions (PRM-07). Apps get a "will suspend" message to close or pause connections. Connection test in P5-T06. |
| Apps that ignore the lifecycle protocol lose unsaved data when terminated | Warning message before the killer acts when possible. Killer prefers hibernated apps first. |
| Hibernated app restore is slow | Restore is a page-in from zram. Target PT-09 (≤ 500 ms). Measured in the benchmark (04). |
| Browsers and large apps keep high memory | Ask apps to release caches on "memory warning". Document per-app results in F3. |

## 7. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| PRM-AC-01 | Background app moves to suspended after the grace period (30 s on AC) | Not tested |
| PRM-AC-02 | Hibernated test app resident memory ≤ 5 MB (PT-10) | Not tested |
| PRM-AC-03 | Hibernated app restores in ≤ 500 ms if hibernated less than 10 minutes (PT-09) | Not tested |
| PRM-AC-04 | Foreground app is never killed in a memory stress test | Not tested |
| PRM-AC-05 | Audio keeps playing in background for 10 minutes | Not tested |
| PRM-AC-06 | Download continues in background for 10 minutes | Not tested |
| PRM-AC-07 | Pressure killer ends background apps in band order and logs the reason | Not tested |
| PRM-AC-08 | 50-app benchmark passes (04 section 4) | Not tested |
| PRM-AC-09 | Activity monitor shows the correct state for each app within 2 s of a change | Not tested |
| PRM-AC-10 | Force-quit from the activity monitor ends the app and all its child processes | Not tested |

## 8. Open Questions

- Default hibernation timing: 10 minutes is the proposal. Test on the 4 GB reference machine before finalizing.
- Whether swap on disk is allowed for hibernation on machines with an SSD. Proposed: optional swap file, off by default.
