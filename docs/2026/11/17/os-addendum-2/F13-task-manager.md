# F13 — Activity Manager (Windows-Style, with Full Metrics)

Owner: `nexa-taskmgr` (shell app). Replaces the earlier "Activity" app (F2 PRM-10, F8 DA-10). Metric sources and sampling: D10.

## 1. Goal

A task manager that Windows users find familiar on the first use, and that also shows what macOS Activity Monitor shows: CPU, memory, GPU, temperature, battery, energy, and history. It must not make the system slower. Its cost while closed is zero; its cost while open is capped (section 6).

Name: "Activity Manager" is a plain functional term. Owner decision Q20 confirms the name.

## 2. Layout (Windows-Like)

```
+---------------------------------------------------------------------------+
| Activity Manager                                                    _  □  X   |
+---------------------------------------------------------------------------+
| Processes | Performance | App history | Startup apps | Users | Details |   |
|           | Services | Battery | Sensors | Android (if pack installed)  |
+---------------------------------------------------------------------------+
| Name           | Status    | CPU  | Memory | Disk   | Network | GPU | Energy |
|--------------- | ----------|------|--------|--------|---------|-----|--------|
| > Browser (4)  | Running   | 12%  | 1.4 GB | 2 MB/s | 3 Mbps  | 4%  | High   |
|   Text         | Foreground| 0.2% | 60 MB  | 0 MB/s | 0 Mbps  | 0%  | Low    |
| > Android Mail | Frozen    | 0%   | 140 MB | 0 MB/s | 0 Mbps  | 0%  | None   |
+---------------------------------------------------------------------------+
| Fewer details                                    [End task]  [Efficiency] |
+---------------------------------------------------------------------------+
```

The "Fewer details / More details" toggle switches between this compact view and the full table. Keyboard shortcut Ctrl+Shift+Esc (Windows profile, F9 AP-14); the default shortcut is Super+Escape.

## 3. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| TM-01 | Processes tab | MVP | One row per app, expandable to processes. Columns: name, status (F2 state), CPU, memory, disk, network, GPU, energy impact, and wakeups. Sort by any column. Color for high use. |
| TM-02 | End task and End process tree | MVP | End task asks for confirmation for unsaved apps. Force Quit dialog (macOS-style) for hung apps. |
| TM-03 | Efficiency mode | MVP | Lowers CPU weight and I/O priority of one app (F2 PRM-09). Shown as a leaf icon in the row. |
| TM-04 | Performance tab | MVP | Sections: CPU, Memory, Disk, Network, GPU, and Sensors. Each has a 60-second graph and a live value. Click a section for detail. |
| TM-05 | CPU detail | MVP | Overall and per-core use, current clock, temperature (where reported), energy preference (EPP), governor, uptime, and processes count. |
| TM-06 | Memory detail | MVP | Used, available, cached, compressed (zram), swap, and memory pressure level (PSI). Shows which apps hold compressed memory. |
| TM-07 | Disk detail | MVP | Each disk: read and write rates, active time, SMART temperature and health (where the disk reports them). |
| TM-08 | Network detail | MVP | Each interface: sent and received rates. Per-app network (TM-15) when the advanced pack is installed. |
| TM-09 | GPU detail | MVP (basic), Pack D (per process) | Engine use (render, video decode, video encode, copy), memory in use, temperature where the driver reports it. Per-process GPU use uses kernel data (D10 section 4). |
| TM-10 | Sensors | MVP | Temperatures (CPU, GPU, SSD, battery, board), fan speeds (where present). Thresholds with optional alerts. |
| TM-11 | Battery tab | MVP | Charge, state, power draw, time remaining, cycle count, health, temperature. Details and history in F14. |
| TM-12 | App history | MVP | Per-app CPU time, network total, and energy for the last 7 days. Long history in F14. |
| TM-13 | Startup apps | MVP | Autostart items with their startup impact estimate. Enable or disable. |
| TM-14 | Users and sessions | MVP | Logged-in users (from systemd-logind). |
| TM-15 | Per-app network | Pack D | Per-app upload and download rates using kernel socket accounting. Off by default. |
| TM-16 | Details tab | MVP | Windows-style process table: process ID, parent, user, status, memory (private and shared), CPU time, command line, threads, open files count, sandbox level. |
| TM-17 | Services tab | MVP | systemd units with status; start, stop, and restart (permission prompt). |
| TM-18 | Android tab | v1 (only with Pack A) | Android apps with their container memory and CPU share (F12). |
| TM-19 | Export report | MVP | Save current view or a time range as text or CSV for support. |
| TM-20 | Alerts | v1 | Notification when a sensor or battery threshold is crossed. Thresholds set in config (F4). |
| TM-21 | Resource summary in the menu bar | v1 | Optional small CPU and memory indicator (off by default). |

## 4. Metric Definitions

| Metric | Definition | Source (D10) |
| --- | --- | --- |
| CPU % (process) | Share of one core, sampled over the interval | `/proc/<pid>/stat` |
| Memory (process) | Private memory (not shared) in use | `/proc/<pid>/smaps_rollup` for visible rows; cgroup `memory.current` for apps |
| Memory (system) | Used = total minus available | `/proc/meminfo`; zram statistics |
| Disk (process) | Bytes read and written | `/proc/<pid>/io` |
| Network (app) | Bytes sent and received by the app's sockets | Kernel socket accounting (Pack D) |
| GPU (process) | Engine busy time per process | DRM usage statistics in `/proc/<pid>/fdinfo` |
| Temperature | Degrees Celsius from each sensor | hwmon and thermal sysfs |
| Energy impact | Relative score from CPU time, wakeups, GPU use, and network | Computed (F7 PWR-12) |

## 5. Windows and macOS Behavior Mapping

| Windows Activity Manager | macOS Activity Monitor | This design |
| --- | --- | --- |
| Processes, Performance, App history | CPU, Memory, Energy, Disk, Network tabs | Processes, Performance, App history |
| End task | Force Quit | End task (TM-02) and Force Quit for unresponsive apps |
| Efficiency mode | (none; similar to Energy setting) | Efficiency mode (TM-03) |
| Startup apps | Login Items (in Settings) | Startup apps (TM-13) |
| Details | (process list) | Details tab (TM-16) |
| Services | (launchd services) | Services tab (TM-17) |
| (none) | Energy tab with history | Battery and App history (F14) |

## 6. Overhead Rules

- Activity Manager open and visible: sampling every 1 second. Target: ≤ 1% of one CPU core and ≤ 40 MB memory.
- Activity Manager closed: no sampling from the window. Background sampling every 30 seconds for battery and app history (D10 section 3). Target: ≤ 0.2% of one CPU core average and ≤ 10 MB memory.
- Expensive reads (per-process memory detail) only for visible rows.
- Per-app network and deep GPU reads only when Pack D is installed.

## 7. Dependencies

- P5 phase (process and power management), where it replaces the earlier Activity app (P5-T07).
- F2 process states, F7 battery data, F14 battery history, D10 metric sources.
- F12 (Android tab) only when Pack A is installed.

## 8. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| TM-AC-01 | Ctrl+Shift+Esc opens Activity Manager within 500 ms | Not tested |
| TM-AC-02 | Processes tab shows every app with CPU, memory, disk, network, GPU, and energy columns | Not tested |
| TM-AC-03 | End task closes an app and all its processes; an unsaved-work warning appears when the app supports it | Not tested |
| TM-AC-04 | Each performance value matches the reference tool within 5% (G-TM2) | Not tested |
| TM-AC-05 | Temperatures match the system sensor tool within 2 °C | Not tested |
| TM-AC-06 | GPU engine use shows activity during a GPU test; shows zero when idle | Not tested |
| TM-AC-07 | Overhead with Activity Manager open ≤ 1% of one core (G-TM1) | Not tested |
| TM-AC-08 | Export report produces readable text and CSV | Not tested |
| TM-AC-09 | Efficiency mode lowers an app's CPU share during a CPU test | Not tested |
| TM-AC-10 | Startup apps can be disabled and stay disabled after reboot | Not tested |

## 9. Open Questions

- Q20: Use the name "Activity Manager" and replace "Activity" with it?
- Whether the menu bar indicator (TM-21) should be on by default. Proposed: off.
