# F6 — Lightweight, Fast, and Efficient Design (Size, RAM, CPU, Speed)

Owner: whole system. Measured with the tests in 04 and the gates in addendum D6. Battery targets are in F7. Boot and app-launch design is in addendum F11.

## 1. Targets

| ID | Resource | Target | Hard limit | Notes |
| --- | --- | --- | --- | --- |
| RES-01 | Live ISO size | 400 MB | 512 MB | Includes installer, base system, and default apps; no Windows engine, no AI model |
| RES-02 | Installed base size | 2 GB | 3 GB | Without Windows engine or AI model |
| RES-03 | Idle RAM (desktop, no apps) | 250 MB | 300 MB | On a 4 GB machine |
| RES-04 | Idle CPU (average, 5 min) | < 1% | 2% | Event-driven; no polling |
| RES-05 | Boot to desktop (bootloader to desktop ready) | 10 s | 15 s | On a reference SSD machine; stretch goal 8 s (F11 section 1) |
| RES-06 | RAM per hibernated app | 2 MB | 5 MB | Simple GUI app |
| RES-07 | Lite profile RAM use | 1.5 GB idle | 2 GB | Reduced effects, aggressive hibernation |
| RES-08 | Idle battery drain | 4% per hour | 6% per hour | Reference laptop; see F7 |
| RES-09 | Cold launch of a native app to first frame | 500 ms | 1 s | Built-in default apps (F8) |
| RES-10 | Input-to-screen latency added by the compositor | ≤ 1 frame | 2 frames | At 60 Hz, one frame is about 16.7 ms; direct scanout for full-screen apps |
| RES-11 | Menu bar, launcher, or control center open | ≤ 100 ms | 200 ms | Measured from input event |
| RES-12 | Hibernated app restore | ≤ 500 ms | 1 s | Apps hibernated less than 10 minutes (F2) |
| RES-13 | Built-in default apps when closed | 0 MB, 0% CPU | 0 MB, 0% CPU | Apps are not running when closed (F8) |
| RES-14 | Optional AI assistant when not installed | 0 MB, no files | 0 MB, no files | F10 AI-01 |
| RES-15 | Optional AI assistant when installed but unloaded | 0 MB model memory | 0 MB model memory | Unloads after 2 minutes idle (F10 AI-06) |
| RES-16 | Activity Manager overhead, open and visible | ≤ 1% of one core; ≤ 40 MB | 1% of one core; 40 MB | Addendum 2 D10 (G-TM1) |
| RES-17 | Activity Manager overhead, closed (background sampling) | ≤ 0.2% of one core average; ≤ 10 MB | 0.5% of one core average; 20 MB | Addendum 2 D10 (G-TM1) |
| RES-18 | Optional Android pack when not running | 0 MB, 0% CPU | 0 MB, 0% CPU | Addendum 2 F12 AND-09 |

## 2. Size Budget (Compressed ISO)

The detailed build breakdown is in D4 section 3. Summary:

| Component | Target (MB) | Limit (MB) |
| --- | --- | --- |
| Kernel and initramfs | 15 | 25 |
| Base userland (glibc, core tools, systemd minimal) | 60 | 90 |
| Graphics stack (Mesa for Intel and AMD, Vulkan) | 80 | 120 |
| Firmware (selected only) | 40 | 60 |
| Compositor, shell (Slint), fonts | 35 | 50 |
| Installer | 35 | 50 |
| Default apps (Text, Images, Documents, Player, Terminal, and others) | 20 | 40 |
| Networking (NetworkManager, wpa_supplicant, Bluetooth minimal) | 25 | 30 |
| Audio (PipeWire minimal) | 12 | 20 |
| **Total** | **322** | **485** |

Optional add-ons are not in the base image: the Windows engine (F9) and the AI model (F10). They are installed on demand.

## 3. RAM Budget (Idle Desktop, 4 GB Machine)

| Component | Budget (MB) |
| --- | --- |
| Kernel and base services | 60 |
| Compositor | 40 |
| Shell (all panels) | 60 |
| Lifecycle, power, and configuration daemons | 20 |
| Audio and network services | 40 |
| Other services and headroom | 25 |
| **Total** | **≤ 245 (target), ≤ 300 (limit)** |

These are planning figures. Measure in P1, P3, and P4 and update this table.

## 4. CPU Budget

- Idle CPU is driven by events, not polling. Timers are coalesced (larger timer slack for background services).
- The shell updates the clock once per minute when no panel is open.
- The compositor draws only damaged areas and sleeps when nothing changes.
- The lifecycle daemon and power daemon react to events and kernel notifications. They do not poll per app.
- Direct scanout for full-screen apps skips composition (RES-10).

## 5. Speed Techniques

Detailed in addendum F11 (boot stages and launch techniques). Summary:

| ID | Technique | Effect |
| --- | --- | --- |
| SPD-01 | Read-only squashfs root with fast decompression (zstd) | Fast boot and app start |
| SPD-02 | Services start on demand (socket activation) | Shorter boot |
| SPD-03 | Shell and compositor start before other services | Earlier desktop |
| SPD-04 | Hibernated apps restore from compressed memory (zram) | RES-12 |
| SPD-05 | Direct scanout for full-screen apps | RES-10 and lower power |
| SPD-06 | Shell panels are created once and shown and hidden, not rebuilt | RES-11 |
| SPD-07 | Launcher index is built in the background and kept in memory | RES-11 |
| SPD-08 | Foreground app gets a higher CPU weight through cgroups (D3) | Snappy foreground |
| SPD-09 | Prewarm of the most-used apps, pre-initialized UI helper, compiled interface files (F11 section 2) | RES-09 |

## 6. Lightweight Techniques

| ID | Technique | Effect |
| --- | --- | --- |
| LW-01 | Read-only squashfs root with compression | Smaller image, less disk I/O |
| LW-02 | Selected firmware and drivers only | Smaller image |
| LW-03 | zram compressed swap | More apps fit in RAM with less disk I/O |
| LW-04 | Process hibernation (F2) | Inactive apps use little RAM |
| LW-05 | Glass effect tiers (D1) | Lower GPU load on weak hardware |
| LW-06 | Lite profile | Lower defaults on 2 GB machines |
| LW-07 | No embedded web browser in the shell or default apps | Avoids large memory use |
| LW-08 | Rust services and Slint shell and apps (small runtimes, one shared runtime library) | Low memory per component |
| LW-09 | Coalesced timers and damage-based drawing | Lower idle CPU |
| LW-10 | Font subset (Latin and common scripts; more on demand) | Smaller image |
| LW-11 | Optional add-ons (Windows engine, AI model) installed only on request | Smaller base image and zero idle cost |

## 7. Effect Tiers and Automatic Selection

| Tier | Trigger | Effects |
| --- | --- | --- |
| Full | Integrated GPU from the last six years or newer; 8 GB RAM or more | All glass effects |
| Reduced | Older GPU, or 4 GB RAM | Blur at half resolution; no refraction |
| Minimal | No GPU shaders, 2 GB RAM, or reduce transparency turned on | Solid panels with borders |

The tier is chosen at login. The user can change it in the settings app (F4).

## 8. Measurement Plan

| Measure | Tool | When |
| --- | --- | --- |
| Image size | Build output and budgets file (D6) | Every build |
| Boot time | `systemd-analyze` and timed boot script | Every release candidate |
| Idle RAM | `free`, `smem` | Weekly on test machines |
| Idle CPU | `top`, `pidstat` | Weekly |
| Hibernated app RAM and restore time | cgroup `memory.current`; timed switch | Every process manager change |
| Frame time and input latency | Compositor counters | Every compositor change |
| Launch time | Timed launch to first frame | Every shell or app change |
| Installed size | Disk usage after install | Each release candidate |

## 9. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| RES-AC-01 | Live ISO ≤ 512 MB (RES-01) | Not tested |
| RES-AC-02 | Idle RAM ≤ 300 MB on 4 GB reference machine (RES-03) | Not tested |
| RES-AC-03 | Idle CPU average < 2% over 5 minutes (RES-04) | Not tested |
| RES-AC-04 | Boot to desktop ≤ 15 s limit, 10 s target, on reference SSD machine (RES-05) | Not tested |
| RES-AC-05 | Lite profile idle RAM ≤ 2 GB on 2 GB machine (RES-07) | Not tested |
| RES-AC-06 | Full tier holds 60 fps on reference integrated GPU; Reduced holds 30 fps | Not tested |
| RES-AC-07 | Menu bar, launcher, and control center open within 100 ms (RES-11) | Not tested |
| RES-AC-08 | Compositor adds at most one frame of latency at 60 Hz (RES-10) | Not tested |
| RES-AC-09 | Cold launch to first frame ≤ 500 ms for built-in default apps (RES-09) | Not tested |
| RES-AC-10 | Default apps and AI assistant use 0 MB when closed or not installed (RES-13, RES-14) | Not tested |

## 10. Open Questions

- Whether the 512 MB limit includes the installer. Proposed: yes; the installer is inside the ISO.
- Whether a server image without the desktop is wanted later. Out of scope in Personal Edition.
