# 01 — Blueprint

## 1. Vision

A personal desktop operating system for ordinary x86-64 PCs and laptops. It runs alongside Windows, looks like a modern macOS-style desktop (macOS 27 is the owner's visual reference, built with original assets), behaves like a customizable Unix system, is approachable for Windows users, and runs Linux, Windows (through compatibility layers), and cross-platform apps. Its top priorities are process management, battery management, speed, and resource efficiency.

Scope starts as a Personal Edition for one owner. Public release is a later goal.

## 2. Priorities

The owner ranked these areas as most important. The order below is the build priority. Where two areas compete, the higher one wins.

| Rank | Pillar | Meaning | Measurable goal (full details in 04 and F-documents) |
| --- | --- | --- | --- |
| 1 | Process management | iOS-style. Inactive apps are suspended, then hibernated, with memory compressed. | 50 apps open; apps not in use use very little memory; the foreground app is never killed |
| 2 | Battery management | Long battery life with no manual tuning. Battery health is protected. | Idle drain ≤ 4% per hour (target), ≤ 6% (limit). Within 15% of Windows battery life on the same workload |
| 3 | Speed | Fast boot, fast app switching, instant menus, low input latency | Boot to desktop ≤ 10 s target (15 s limit); hibernated app restore ≤ 500 ms; menus open ≤ 100 ms; compositor adds at most one frame |
| 4 | Resource optimization | Small image, low idle RAM and CPU | Image ≤ 512 MB; idle RAM ≤ 300 MB on 4 GB; idle CPU < 2% |
| 5 | Look and feel | macOS 27-style glass desktop with original assets | 60 fps with full effects on a reference integrated GPU; automatic fallback |
| 6 | Unix core | Everything is a file, configuration is text, command line and scripting are first-class | Every setting in the settings app has a file key and a command-line equivalent |
| 7 | Windows-friendly usability | Installer wizard, search-first launcher, app store, no terminal for daily use | A new user installs an app in 3 clicks or fewer |
| 8 | Apps from everywhere | Linux apps, Windows apps through Wine and Proton, cross-platform apps | 30 of 40 test Windows apps in Tier A or B |
| 9 | Hardware coverage | Runs on any x86-64 PC whose devices have Linux drivers | Coverage goal in 04 section 11.1 |

## 3. Scope

### Personal Edition (required)
- x86-64 PCs and laptops. UEFI boot and legacy BIOS boot (see ADR-016).
- Intel and AMD graphics. Common Wi-Fi, Bluetooth, audio, and input hardware with Linux drivers.
- Side-by-side install with Windows 10 and Windows 11 (dual boot).
- Desktop shell, window manager, process manager, power manager, settings, installer, and updates (basic).
- Built-in original default apps: Text, Calculator, Images, Documents, Player, Terminal, Calendar and Clock, Archive (addendum F8).
- Internal app platform that installs and runs native, portable, and Windows apps with no user-visible third-party tool (addendum F9).
- Optional basic local AI assistant, off by default and removable (addendum F10).
- Activity Manager with full metrics (CPU, memory, GPU, temperature, fans, disk, network, battery, energy), battery analytics (cycles, health trend, sessions), and screen time (addendum 2 F13, F14).
- Optional feature packs, installed like Windows Optional Features; heavier features are packs, not core (addendum 2 F15).
- Flatpak apps, AppImage apps, and Windows apps through Wine (basic).
- Unix command line, configuration files, and a command-line control tool.

### Public release (Deferred until the owner decides to release)
- Secure Boot signing (shim review), signed release pipeline, and published signing keys.
- Trademark search and final product name. Slint license review for distribution.
- Full software store, extension API, public documentation, and wide hardware certification.
- Public beta and support processes.

### Out of scope (for now)
- Running macOS applications (see 02-feasibility-and-legal.md).
- Apple Silicon Macs (Q5). Windows-on-ARM laptops (open question Q8).
- Running Windows inside AetherOS. A virtual machine is not needed for dual boot.
- Android apps are not part of the core. They are an optional pack after Personal Edition core (addendum 2 F12; Q6 and Q18 to Q22).
- Server and enterprise editions.
- Writing our own kernel. We use the Linux kernel.

## 4. Target Hardware

| Tier | CPU | RAM | Storage | Notes |
| --- | --- | --- | --- | --- |
| Minimum | x86-64 | 4 GB | 32 GB SSD | Full desktop; fewer background apps |
| Recommended | Intel 8th generation or newer, or AMD Ryzen | 8 GB or more | 128 GB SSD | Full effects; 50-app target |
| Lite (stretch) | Any x86-64 | 2 GB | 16 GB | Reduced effects; aggressive hibernation |

Hardware coverage (what "any system" means in practice) is defined in 04 section 11.1 and depends on the answer to Q10. 512 MB of RAM cannot keep 50 modern GUI apps awake. The process manager keeps inactive apps near zero, but active apps still need real memory.

## 5. Architecture Summary

```
+------------------------------------------------------------------+
|  Apps: Linux native | Flatpak | Windows (Wine) | Java,          |
|        Electron, Flutter, Qt, GTK, web apps                      |
+------------------------------------------------------------------+
|  Desktop shell (Rust + Slint): menu bar, dock, launcher,         |
|  control center, notifications, settings, lock screen            |
+------------------------------------------------------------------+
|  Compositor (Rust, Smithay): windows, glass effects, workspaces, |
|  input, XWayland, direct scanout for full-screen apps            |
+------------------------------------------------------------------+
|  Services (Rust): nexa-lifecycled (process manager),             |
|  nexa-powerd (power policy), nexa-confd (configuration),         |
|  nexa-pkg, nexa-compat (Wine), nexa-updater                      |
+------------------------------------------------------------------+
|  Base: glibc, systemd (minimal), UPower, power-profiles-daemon,  |
|  PipeWire, NetworkManager, Mesa, selected firmware               |
+------------------------------------------------------------------+
|  Linux kernel (custom config: cgroup v2, PSI, zram, DRM, EFI,    |
|  tickless idle, runtime power management)                        |
+------------------------------------------------------------------+
|  Hardware (x86-64; UEFI or legacy BIOS)                          |
+------------------------------------------------------------------+
```

Details: D2 (internal architecture), D3 (process manager), D5 (power management).

## 6. Key Decisions (summary)

Each decision is recorded as an ADR in D2, section 9. Status is Proposed until the owner accepts it in P0-T02.

| Area | Proposed decision | Reason | Alternative considered |
| --- | --- | --- | --- |
| Language | Rust for all custom code (ADR-013) | Memory safety without a garbage collector, small binaries, low resource use, strong open-source ecosystem, one language to maintain | C (more bugs to find alone); C++ (larger, harder to maintain); Go (garbage collector pauses) |
| Kernel | Mainline Linux LTS with custom config (ADR-001) | Drivers, cgroups, PSI, zram, power management | Own kernel (rejected) |
| Base image | Buildroot, read-only squashfs root (ADR-002) | Small, reproducible images | Debian minimal, Arch |
| Init | systemd minimal profile (ADR-003) | cgroup v2 and unit management | runit or s6 (test size in P1) |
| Compositor | Smithay (ADR-005) | Rust, modern Wayland | wlroots (C) |
| Shell UI | Slint, pending spike S1 (ADR-004) | Rust-native, small runtime, one language | Qt 6 (larger runtime, C++ and QML); iced; GTK4 |
| Process manager | nexa-lifecycled in Rust (ADR-006) | Safe, low overhead | C |
| Power management | nexa-powerd policy on top of UPower and power-profiles-daemon (ADR-014) | Reuses standard battery data and profile mechanisms; our policy is the differentiator | Writing our own battery and profile stack (more work, no gain) |
| Configuration | TOML files, Lua for scripting (ADR-007) | Readable, small, embeddable | JSON, Python (heavier) |
| Linux apps | Flatpak first, AppImage second (ADR-008) | Sandboxing and Flathub | Snap |
| Windows apps | Wine and Proton, managed by nexa-compat (ADR-009) | Legal, no Windows license needed | Virtual machine (heavier) |
| Boot | systemd-boot with signed UKI for UEFI; GRUB for legacy BIOS (ADR-010, ADR-016) | Detects Windows Boot Manager; covers old PCs | Secure Boot path deferred to public release |
| Updates | A/B image updates with RAUC (ADR-011) | Safe rollback | OSTree (evaluate later) |

## 7. Success Metrics

1. Personal Edition: the owner installs AetherOS beside Windows 11, boots either OS, and returns to Windows without data loss (10 of 10 cycles).
2. Boot to desktop ≤ 10 s target (15 s limit) on a reference 4 GB SSD machine.
3. The 50-app benchmark passes (04 section 4).
4. The battery benchmark passes (04 section 11.2).
5. Image ≤ 512 MB; target ≤ 400 MB.
6. The owner uses AetherOS as the daily driver for 30 consecutive days.

## 8. Risks

Full register with mitigations: 03-roadmap-plan.md, section 5.

## 9. Related Documents

- Feasibility and legal: 02-feasibility-and-legal.md
- Plan: 03-roadmap-plan.md
- Tests and targets: 04-test-and-acceptance.md
- Features: features/
- Design: design/
