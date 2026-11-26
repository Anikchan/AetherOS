# F15 — Optional Features and Packs (Like Windows Optional Features)

Owner: `nexa-features` (install, remove, and cost display), part of Settings. Costs are checked by the gates in section 5.

## 1. Principle

The core OS stays light. Heavier features are optional packs. Each pack is installed or removed from Settings → Features, the same way Windows lets users turn optional features on or off. Before a pack is installed, the screen shows:

- Download size and disk size.
- Memory used while running, and whether it runs at all when idle.
- CPU use while idle.
- Estimated battery effect.
- Whether the pack can be removed completely.

A pack that is not installed costs nothing: no process, no file, no memory.

## 2. Core (Always Installed, Always Light)

| Feature | Notes |
| --- | --- |
| Activity Manager (basic) | CPU, memory, disk, network, GPU (basic), sensors, battery summary (F13) |
| Battery status and basic history | Cycles where reported, daily health, sessions for 30 days (F14) |
| Screen time (basic) | Screen-on time and time per app (F14 section 2.2) |
| Process management | F2 and D3 |
| Default apps | F8 |
| Power management | F7 |
| Optional feature manager | This document |

The core is measured as a whole. Its idle cost must stay inside the budgets in F6 (gate G-OPT1).

## 3. Optional Packs

| Pack | Contents | Disk (estimate) | Memory when running | Idle cost when not in use | Priority |
| --- | --- | --- | --- | --- | --- |
| A. Android apps | F12 container and tools | 1 to 2 GB (to be measured) | 0.7 to 1.2 GB while an Android app is open (estimate); 150 MB when frozen | 0 when not running; keep-ready adds frozen memory | After M5 (Q22) |
| B. Windows apps | F9 Windows engine (Wine) | 0.5 to 1.5 GB | Per Windows app (measured per app) | 0 when not running | v1; scope option A (03 section 2.1) |
| C. AI assistant | F10 runtime and model | Up to 4 to 8 GB total with the OS (owner answer Q14) | About the model size when loaded (roughly 2.5 to 3 GB for the chosen 4B model, estimate; see Q23) | 0 when unloaded | After Android (P7-AI) |
| D. Advanced monitoring | Per-app network, per-process GPU, one-second sensor logs | 20 MB | About 30 MB | Target ≤ 0.5% of one core | After M5 |
| E. Screen Time limits | Daily limits and downtime (ST-05, ST-06) | 10 MB | About 10 MB | ≤ 0.1% of one core | After M5 |
| F. Battery reports | Full text reports and long session history (F14 BAT-10) | 10 MB | About 10 MB while generating | 0 | After M5 |
| G. Storage analyzer | Disk usage map and large file finder (macOS-style Storage view) | 15 MB | About 40 MB while scanning | 0 (scans run only when the user opens the view) | After M5 |

Packs D to G are in the plan as P7-PK (03 section 3). Their hours are listed there.

### Pack D contract status (2026-11-27)

The Pack D side of the Activity Manager is implemented as a data contract and a
pair of daemon recorders (`aether-taskmanager --pack-d-contract`, documented in
D10 §5.2):

- Per-app network: strict parser for the socket-accounting snapshot at
  `/run/aether/pack-d/net-accounting`, delta rates, hourly totals for the 7-day
  App history window, and a "Not installed"/"Not loaded" state instead of zero.
  `aether-powerd` runs the recorder on its own tick and persists the buckets
  (`pack-d-network.json`), so history accumulates while the Activity Manager is
  closed; `aether-taskmanager --pack-d-history <file>` reads them into the tab.
- Per-process GPU: DRM `fdinfo` engine/memory parsing, engine-busy deltas and
  per-app attribution through the process cgroup. `aether-lifecycled` runs the
  recorder every 30 s and persists its hourly buckets (`pack-d-gpu.json`).
- Refresh 2 s visible / 30 s hidden, ≤ 0.5% of one core target (D10 §3).

Still hardware-gated: attaching the eBPF accounting program and dumping its map,
and validating GPU numbers on the reference Intel/AMD machines (G-OPT2). The tab
shows the honest null label until then.

## 4. Settings Screen

Settings → Features shows a list. Each row has:

| Column | Example |
| --- | --- |
| Name | Android apps |
| Status | Not installed / Installed / Installing / Removing |
| Disk used | 1.4 GB |
| Idle cost | 0% CPU, 0 MB |
| Battery effect | None when idle |
| Action | Install / Remove |

A removed pack returns the disk space and removes all its processes and data (after a confirmation).

## 5. Gates

| Gate | Pass value | Method |
| --- | --- | --- |
| G-OPT1 Core idle budget | With all core features on and idle, the system stays inside RES-03 and RES-04 (F6) | Idle measurement for 5 minutes |
| G-OPT2 Pack idle cost | Each pack's idle cost matches its listed value (within 20%) | Measurement per pack |
| G-OPT3 Clean removal | After removal, no pack file, process, or registry entry remains | File and process comparison |
| G-OPT4 Cost display | Estimated sizes match the installed sizes within 20% | Compare with disk usage |

## 6. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| OPT-AC-01 | Settings → Features lists every pack with status, disk use, and idle cost | Not tested |
| OPT-AC-02 | Installing a pack shows the cost information before the install starts | Not tested |
| OPT-AC-03 | A removed pack leaves no process, file, or registry entry (G-OPT3) | Not tested |
| OPT-AC-04 | With no packs installed, the system runs the core within the F6 budgets (G-OPT1) | Not tested |
| OPT-AC-05 | Disabling a pack that is running stops it within 10 seconds | Not tested |

## 7. Open Questions

- Q21: Which packs should be on by default after install? Proposed: none; the owner chooses in Settings.
- Whether Screen Time limits (Pack E) should be included in the core. Proposed: no, to keep the core light.
