# Addendum 1 — Answers and New Requirements (2026-11-17)

Parent plan: `docs/2026/11/16/os-project/` (main index: `docs/2026/11/16/os-project/README.md`). Hours and dates in the "Plan Impact" section below are superseded by addendum 2 and `03-roadmap-plan.md`.

This folder answers the owner's five questions and adds three new requirements. The main plan has been updated to match (phases, hours, scope, and open questions).

## Short Answers

### Q1. iOS-style process management: are we achieving it, and how do we know?

- **Yes, by design.** Inactive apps are kept open in one of two states: frozen (suspended, no CPU use) or hibernated (memory compressed, small footprint). Apps are closed only under real memory crunch, in priority order. This is the same idea as iOS. Closing an app is not used to save battery.
- **How we make sure:** a 50-app benchmark, a 72-hour soak test, a hibernated-app size test, and a foreground-safety gate, each with pass values (D6 gates G-PM1 to G-PM5). Results are compared with stock Linux, which has no such management.
- **What we cannot promise:** an identical result to iOS. iOS apps are built for suspension. Linux and Windows programs are not always built that way, so some apps (especially browsers and Windows programs) will keep more memory. We will report those numbers, not hide them.
- Details: main plan feature F2 and `D6-verification-framework.md`.

### Q2. Battery life, fast app opening, and no hangs

- **Battery:** The target is "within 15% of Windows 11 on the same laptop and workload", plus an idle drain limit. "Longest possible" cannot be proven in absolute terms, so it is measured against Windows (G-BAT1, G-BAT2). Some laptops may fall short because of missing vendor power features; that is recorded, not hidden.
- **Fast app opening:** Built-in apps are targeted at 500 ms to first frame. Techniques: prewarming apps used most often, a pre-loaded UI helper that new apps are forked from, compiled interface files, lazy loading, and restore from compressed memory (F11 section 2).
- **Fast boot:** Target 10 s from bootloader to desktop on the reference SSD machine, with a stage budget (F11 section 1). Firmware time is not ours and is reported separately.
- **No hangs unless there is a full crunch:** The compositor never waits for an app. Unresponsive apps are detected with the Wayland ping and marked "Not responding" with Force Quit. The shell has a watchdog. Foreground priority is protected. Hangs are measured by a frame-gap logger, and gaps are counted only outside the Critical pressure level (F11 section 3, D6 G-NH1, G-NH2).

### Q3. Install and run apps from Windows, Linux, and macOS with no third-party tool

- **Yes for Windows and Linux, with one honest correction.** The user never needs a separate program to install or run apps. One install path, one launcher, one update path, and one uninstall path handle all supported kinds (F9).
- **The correction:** Running Windows programs needs a Windows compatibility engine. Writing one ourselves would take many years, so the OS includes Wine as a hidden, versioned engine (LGPL). The same applies to open-source libraries for images, media, and PDF. These are included inside the OS, not as user-visible apps. Owner question Q11 asks for confirmation.
- **macOS:** Not possible in this plan. The OS can install cross-platform apps (apps that ship Windows or Linux builds). macOS packages are detected and explained, not installed (F9 section 5, D7).
- **No Flatpak or Flathub tool is required.** Native bundles use our own format and sandbox. AppImage files need no tool at all (Q13 asks whether to also read Flathub later).

### Q4. Size, CPU, and RAM

Planning estimates. Measured values come after P1 and P4 and are recorded in the D6 results log.

| Item | Estimate | Limit or note |
| --- | --- | --- |
| Live ISO (installer, base, default apps; no AI, no Windows engine) | 322 to 485 MB | Must stay under 512 MB (D4 budget) |
| Installed base system (with default apps) | About 2 GB | Limit 3 GB |
| Windows engine (optional add-on) | About 0.5 to 1.5 GB disk | To be measured in P6 |
| AI model (optional add-on) | Up to 4 to 8 GB disk, counted with the OS size; chosen model Qwen3.5 4B (4-bit) | Per model, Q14 (answered 2026-11-17) |
| Idle RAM (desktop, no apps open) | About 250 MB | Limit 300 MB on 4 GB |
| RAM during typical use | Depends on apps; the OS adds about 250 MB on top | Browser usually dominates |
| RAM with AI loaded | Additional 0.6 to 1.5 GB | Unloaded after 2 minutes idle |
| Minimum CPU | x86-64 with two cores at about 2 GHz (SSE4.2 support) | Target for "lite" profile |
| Recommended CPU | Four or more cores, Intel 8th generation or AMD Ryzen or newer | |
| Minimum RAM | 2 GB (lite profile) | Recommended 8 GB |
| Idle CPU | Under 1% average | Limit 2% |
| Idle disk activity | Near zero | Event-driven design |

### Q5. Separate kernel, and what macOS apps would need

- **Keep the option open.** All Linux-specific code goes into one crate, so a different kernel later affects only that crate (D7 section 3).
- **Closest kernel now: Linux.** It is the only option that covers driver support, process control, power management, and Windows-program support within this plan.
- **macOS apps would need:** a macOS program loader, a system library and linker equivalent, Apple-style kernel services (Mach and BSD), a service manager, Objective-C and Swift runtimes, core libraries, and a clean-room rewrite of Apple's graphics and UI frameworks. The last one is the largest and is proprietary.
- **Reference:** Darling is an open-source project that runs some macOS programs on Linux. It is partly working and uses a modified XNU kernel. It is a research reference only.
- **Verdict:** Not planned for Personal Edition. A research track for macOS command-line tools can be opened after M5, if wanted (D7 section 6). Owner decision needed.

## New Requirements

### R1. Default apps such as Notepad (as original apps)

- Built-in apps: Text (plain text and code editor, equivalent functions to a classic Notepad), Calculator, Images, Documents (PDF), Player, Terminal, Calendar and Clock, Archive, and the existing Files, Activity, Settings, and Screenshot.
- Names and code are original. "Notepad", "Paint", and "Explorer" are Microsoft names and are not used (F8 section 2). Owner question Q12.
- Estimated effort: about 400 hours (F8, P4-T10 to P4-T17).

### R2. Lightweight and fast boot

- Boot to desktop target 10 s; stretch 8 s; limit 15 s (F11 section 1, F6 RES-05).
- Built-in apps idle at 0 MB and 0% CPU when closed (F6 RES-13, F8 section 4).

### R3. Optional basic AI model

- Off by default, installed only if the user chooses, local only, removable in one step (F10).
- Basic model of about 0.5 to 1.5 billion parameters, 4-bit quantized. Owner question Q14.
- Runs only when asked, unloads after 2 minutes idle, and follows battery rules (F10 section 3, D8 section 6).
- Estimated effort: about 220 hours, optional, after the Personal Edition core (P7-AI).

## Plan Impact

| Change | Hours |
| --- | --- |
| Default apps (P4-T10 to P4-T17) | +400 |
| Internal app platform, replacing the earlier basic compatibility work (P6, from 300 to 610 hours) | +310 |
| Boot speed and app launch work (P1-T08, P3-T08) | +80 |
| Verification setup (P0-T11) and Windows engine spike (P0-T12) | +40 |
| AI assistant, optional (P7-AI) | +220 (after core) |

The Personal Edition core is now about 3,820 hours (about 147 weeks at 26 hours per week). Details and dates: `docs/2026/11/16/os-project/03-roadmap-plan.md`.

## Documents in This Folder

| Document | Purpose |
| --- | --- |
| [F8-default-apps.md](F8-default-apps.md) | Built-in default apps |
| [F9-app-platform.md](F9-app-platform.md) | Internal install and run platform; no user-visible third-party tools |
| [F10-ai-assistant.md](F10-ai-assistant.md) | Optional basic AI assistant |
| [F11-speed-and-responsiveness.md](F11-speed-and-responsiveness.md) | Boot speed, app launch, no-hang design |
| [D6-verification-framework.md](D6-verification-framework.md) | How each claim is proven: gates, telemetry, process |
| [D7-kernel-and-macos-strategy.md](D7-kernel-and-macos-strategy.md) | Kernel choice, separate-kernel option, macOS requirements |
| [D8-app-platform-and-ai-runtime-design.md](D8-app-platform-and-ai-runtime-design.md) | Contracts for the app platform and AI runtime |

## Open Questions (New)

All questions Q8 to Q17 were answered by the owner on 2026-11-17 and are recorded in the main plan's Owner Decisions table (`docs/2026/11/16/os-project/README.md`). No open questions remain at this time.
