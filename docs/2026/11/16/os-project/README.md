# OS Project Documentation (AetherOS)

- Created: 2026-11-16
- Last updated: 2026-11-26 (S4: G-PM1/G-PM5 gate report + soak baseline sampling, Energy column contract, Windows engine packaging + installer flow, live USB build path, RAUC A/B wiring; see `docs/2026/11/26/session-s4-progress.md`)
- Status: Implementation in progress (2026-11-26). P0/P1 verification is largely complete (P1 UEFI fixed on real firmware; hardware re-validation paused). P5/P6/P7/P8/P9 core logic is implemented and tested (300+ tests across S4 crates): lifecycle, power/analytics, Activity Manager core with the energy column contract, app platform/sandbox/store, Windows engine packaging and installer flow, config+aetherctl, installer planner/pre-checks, live USB image path, A/B update state machine wired to a real RAUC config. Remaining work is hardware validation, the 72-hour soak and G-PM1 on the reference machines, UI bindings, and the 30-day dogfood. Active parallel sessions S1–S4; S4 owns design docs and this README, S3 owns feature docs.
- Scope: Personal Edition first. Public release is a distant future goal.
- Team: One engineer (the owner) at 26 hours per week.
- OS name: AetherOS (chosen by the owner on 2026-11-17). Component names use the "aether-" prefix (Cargo workspace: aether-compositor, aether-shell, aether-lifecycled, aether-installer). Trademark check is deferred until public release (task P0-T03b).

## Document Map

| # | Document | Purpose |
| --- | --- | --- |
| 01 | [01-blueprint.md](01-blueprint.md) | Vision, priorities, scope, architecture summary, key decisions, success metrics |
| 02 | [02-feasibility-and-legal.md](02-feasibility-and-legal.md) | What is possible, what is not, licensing |
| 03 | [03-roadmap-plan.md](03-roadmap-plan.md) | Phases P0 to P10, step-by-step tasks, hour and calendar estimates, milestones, risks, progress tracker |
| 04 | [04-test-and-acceptance.md](04-test-and-acceptance.md) | Test strategy, performance and battery targets, benchmarks, hardware coverage, release criteria |
| F1 | [features/01-desktop-shell.md](features/01-desktop-shell.md) | Menu bar, dock, launcher, control center, windows, lock screen |
| F2 | [features/02-process-management.md](features/02-process-management.md) | iOS-style app suspension, hibernation, memory pressure handling |
| F3 | [features/03-app-compatibility.md](features/03-app-compatibility.md) | Linux, Windows, and cross-platform app support |
| F4 | [features/04-customization.md](features/04-customization.md) | Unix-style configuration, theming, scripting, command-line tool |
| F5 | [features/05-installer-dual-boot.md](features/05-installer-dual-boot.md) | Installer, side-by-side install with Windows, legacy BIOS |
| F6 | [features/06-lightweight-resources.md](features/06-lightweight-resources.md) | Size, RAM, CPU, and speed budgets |
| F7 | [features/07-battery-power-management.md](features/07-battery-power-management.md) | Battery life, power profiles, charge limits, sleep, low-battery actions |
| D1 | [design/ui-design.md](design/ui-design.md) | Visual language, glass material spec, tokens, components |
| D2 | [design/internal-architecture.md](design/internal-architecture.md) | System layers, components, IPC, file layout, security, decision log (ADR-001 to ADR-017) |
| D3 | [design/process-manager-design.md](design/process-manager-design.md) | Lifecycle daemon contracts, state rules, killer bands |
| D4 | [design/image-build-design.md](design/image-build-design.md) | Image build pipeline, size budget, partition layout, boot chain |
| D5 | [design/power-management-design.md](design/power-management-design.md) | Power policy daemon contracts, battery estimation, device tuning |

## How to Resume Work (for any session, human or AI)

1. Read the progress tracker in [03-roadmap-plan.md](03-roadmap-plan.md), section 4.
2. Find the first task that is not marked Done and whose dependencies are Done.
3. Before starting a task, mark it "In progress" with the date and session name.
4. When the task ends, mark it Done or Blocked, add a line to the progress log, and update any affected document.

## Owner Decisions (Recorded 2026-11-16)

| ID | Question | Owner answer | How the plan applies it |
| --- | --- | --- | --- |
| Q1 | What is "Golden Gate"? | The latest macOS version, 27.x | D1 uses macOS 27 as the visual reference. Apple assets are still not copied. |
| Q2 | Which hardware? | No specific machine. Any system that can run Unix or Windows should be able to run AetherOS. | Hardware coverage goal in 04 section 11.1. x86-64 only (see Q10 below). |
| Q3 | Who is it for? | Personal use now. Public release may come much later. | Personal Edition scope first. Public-only tasks are marked Deferred. |
| Q4 | Team and hours | One person, 26 hours per week | All estimates are in hours, converted to calendar dates in 03 section 2. |
| Q5 | Apple Silicon Macs? | No | Out of scope. |
| Q6 | Android apps? | Maybe later. Not in scope now. | Listed as Later in F3. Not scheduled. |
| Q7 | Language choice | Owner leaves the choice to the plan. Priorities: robust, light, easy to maintain, open source, and best-in-class process management, battery management, speed, and resource use. | Rust for all custom code (ADR-013). Slint for the UI, to be proven by spike S1 (ADR-004). |
| Q8 | Are Windows-on-ARM laptops (Snapdragon-type) in scope? | No for now, but may be in future. Future scope: add an ARM64 target and image-build variant when needed (ADR-016). | x86-64 only in Personal Edition; ARM64 is a future-scope item, not scheduled. |
| Q9 | Which Mac do you develop on (Intel or Apple Silicon)? Do you have a spare x86-64 PC or laptop to use as the test machine? | Apple Silicon Mac; yes, a spare x86-64 PC/laptop is available. | Development stays on the Mac host; testing on Linux x86-64 VMs plus the spare PC (ADR-017). |
| Q10 | Confirm reading of Q2: "AetherOS runs on any x86-64 PC whose devices have Linux drivers, using UEFI or legacy BIOS boot. Devices that only have Windows drivers are not supported." | Prefer a broader range of hardware to be covered. | Keep the x86-64 plus Linux-driver baseline, but expand the hardware coverage goal and testing matrix toward more devices (04 section 11.1); Windows-only-driver devices remain unsupported. |
| Q11 | Confirm that "no third-party app" means no user-visible third-party app or tool. Open-source engines (Wine, media and PDF libraries) are included inside the OS. | Confirmed intent: Wine or any other engine used to run platform-specific apps must be handled entirely by the OS, so users feel no difference and apps appear to run natively. | App platform hides all engines; native-like launcher, install, update, and uninstall paths (F9, ADR-019). |
| Q12 | Name for the text app ("Text" proposed); "Notepad" is avoided. | Name it `<OSname>Pad` (AetherPad, since the OS is AetherOS). | Text app working name is AetherPad; functional name "Text" remains the generic label (F8). |
| Q13 | Read the Flathub catalog later, or use only our own store? | Own store first, Flathub catalog possibly later. | Own store and bundles first; Flathub catalog reading deferred as an optional later enhancement (F9, ADR-019). |
| Q14 | AI model size range. | Around 4 to 8 GB at most, counted together with the OS size; choose models that fit that budget. | AI model add-on target becomes up to 4-8 GB total; model shortlist and sizes updated in F10. |
| Q15 | Windows program support in Personal Edition, or after it? | In Personal Edition. | Windows engine (AP-05, AP-06) stays in Personal Edition scope (P6); largest cost accepted (F9). |
| Q16 | Include a browser by default? | Yes, a lightweight browser with high-quality media playback and streaming support. | Add a lightweight default browser to F8; heavier browsers remain optional via the store. |
| Q17 | Accept "no macOS apps" for Personal Edition, with a research track left open? | Accept no macOS apps, but the OS must run the owner's own apps (such as PowerGuard and AI monitoring) as native/cross-platform bundles. | Personal Edition supports owner-built native/cross-platform apps; macOS GUI apps remain unsupported; macOS CLI research track stays optional (D7). |
| Q18 | Accept the Android limits (ARM-only apps need proprietary translators we cannot ship; no Google Play Store; Widevine limits)? | Wants the limits removed. Plan to investigate user-supplied translators, alternative stores, and microG, but proprietary translators and Google Play Store cannot be shipped. Limits remain unless a legal/technical path is found. | Addendum 2 F12; AND-15 |
| Q19 | Android start mode: lazy (default) or keep-ready (about 150 MB frozen memory)? | Lazy by default. | Addendum 2 F12 |
| Q20 | Name the monitoring app "Activity Manager" and replace "Activity" (proposed: yes)? | Name it "Activity Manager". | Addendum 2 F13 |
| Q21 | Which optional packs are on by default after install (proposed: none)? | None for now. | Addendum 2 F15 |
| Q22 | Android before M5 (adds 620 hours; M5 about June 2030) or after M5 (proposed)? | Android later, after M5 as the first pack. | Addendum 2 README; 03 section 2.1 |
| Q23 | RAM for the AI pack: the chosen model (Qwen3.5 4B, 4-bit) needs roughly 2.5 to 3 GB when loaded, so 4 GB machines will struggle. Set the AI pack minimum to 8 GB (proposed), or choose a smaller model for 4 GB machines? | Use a lighter default model for 4 GB machines, but keep the option for users to install larger models (up to 8 GB) when their hardware allows. | Addendum 1 F10; addendum 2 F15 Pack C |
| Q24 | Browser engine family (Chromium-based or Firefox-based), and accept lower quality or no playback for streaming services requiring Widevine? | Choose whichever engine is lighter; target HD playback when feasible, otherwise reduced quality. | Addendum 1 F8 DA-13 (P4-T18) |

## Open Questions

Questions Q8 to Q24 were answered by the owner on 2026-11-17 (see the Owner Decisions table above). No open questions remain at this time.

## Glossary

| Term | Meaning |
| --- | --- |
| A/B update | Two system partitions. One runs while the other is updated. A failed update rolls back. |
| cgroup v2 | Linux mechanism to group processes and limit CPU, memory, and freezing per group. |
| PSI | Pressure Stall Information. Linux reports how long tasks wait for memory, CPU, or I/O. |
| zram | Compressed RAM block device used as swap. Pages are compressed in RAM. |
| UKI | Unified Kernel Image. Kernel, initramfs, and command line in one EFI file. |
| EPP | Energy Performance Preference. A CPU hint from performance to power saving. |
| s2idle | Suspend-to-idle. Low-power sleep that works on most modern laptops. |
| UPower | Standard Linux service that reports battery state. |
| Slint | A UI toolkit written in Rust with a small runtime. |
| Smithay | Rust library for writing Wayland compositors. |
| Flatpak | Sandboxed app packaging format for Linux, distributed through Flathub. |
| Flathub | Public "app store" and repository for Flatpak apps: discovery, metadata, screenshots, updates, and install files. Reading its catalog is optional and deferred to a later enhancement. |
| Wine / Proton | Compatibility layers that run Windows programs on Linux. |
| Jetsam | iOS mechanism that ends background apps when memory runs low. Used here as a design reference. |
