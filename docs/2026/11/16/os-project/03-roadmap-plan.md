# 03 — Roadmap Plan

Updated 2026-11-17 for addendum 2 (`docs/2026/11/17/os-addendum-2/`). Phase numbers are unchanged. Hours, tasks, and dates have grown.

## 1. Planning Assumptions

- One engineer (the owner) at 26 hours per week (Q4): 5 hours × 2 weekdays + 8 hours × 2 weekend days.
- Hours per task are planning estimates for one person learning as they go. Add a 30% buffer for risk (section 2).
- Week numbers count from Week 1 = the first full week after this plan is approved (approximately 2026-11-17).
- Calendar dates are approximate. They move when the actual hours differ from the estimate.
- Re-estimate after P1 and P3. These are the two highest-risk early phases.
- AI assistants may help write code, but every result must be tested by the owner before it counts as done (addendum 1, D6 section 1).

## 2. Phase Overview and Estimates

Cumulative weeks = total estimated hours up to the end of the phase, divided by 26 hours per week (rounded).

| Phase | Name | Hours (estimate) | Cumulative weeks | Needed for Personal Edition | Milestone |
| --- | --- | --- | --- | --- | --- |
| P0 | Foundation, spikes, verification setup | 120 | 5 | Yes | — |
| P1 | Base system boots and boot speed | 160 | 11 | Yes | M1 |
| P2 | Hardware bring-up | 250 | 20 | Yes | M2 |
| P3 | Compositor, window system, no-hang design | 540 | 41 | Yes | — |
| P4 | Desktop shell, default apps, and default browser | 1,170 | 86 | Yes | M3 |
| P5 | Process, power, Activity Manager, battery analytics, screen time | 900 | 121 | Yes | M4 |
| P6 | Internal app platform (no user-visible third-party tools) | 610 | 144 | Yes | — |
| P7 | Customization, developer tools, optional feature manager (basic) | 240 | 153 | Yes | — |
| P8 | Installer and dual boot | 260 | 163 | Yes | M5 |
| P6-AND | Android apps (Pack A) | 620 | 187 | Optional (after M5 by default) | — |
| P7-PK | Optional packs D to G | 160 | 193 | Optional (after M5) | — |
| P7-AI | Optional AI assistant (Pack C) | 220 | 202 | Optional (after M5) | — |
| P9 | Updates, security, hardening | 220 | 210 | Deferred parts | — |
| P10 | Beta and public release | 150 | 216 | Deferred | M6 |

Totals:
- Personal Edition core (P0 to P8): 4,250 hours, about 163 weeks. Approximately January 2030 without buffer; about December 2030 with 30% buffer.
- Core plus Android (P6-AND, if placed before M5 per Q22): 4,870 hours, about 187 weeks. Approximately June 2030 without buffer.
- Full plan including all optional packs and public release (P0 to P10): 5,620 hours, about 216 weeks. Approximately January 2031 without buffer.

Why the hours grew from addendum 1 (3,820 to 4,250 for the core):
- Activity Manager replaces the earlier Activity app (P5-T07 grows from 20 to 150 hours): +130.
- Battery analytics and screen time (P5-T16, P5-T17): +120.
- Optional feature manager (P7-T07): +60.
- Default browser (owner answer Q16; P4-T18 family): +120.

Calendar estimates (approximate, from 2026-11-17, no buffer):

| Milestone | End of phase | Week | Approximate date |
| --- | --- | --- | --- |
| M1 Boots in a Linux virtual machine, both UEFI and legacy BIOS | P1 | 11 | February 2027 |
| M2 Boots on owner's real hardware (checklist passes) | P2 | 20 | April 2027 |
| M3 Daily-driver desktop (basic shell, default apps, browser) | P4 | 86 | July 2028 |
| M4 Process, power, and Activity Manager proven (gates G-PM, G-BAT, G-TM) | P5 | 121 | March 2029 |
| M5 Personal Edition ready (installable beside Windows) | P8 | 163 | January 2030 |
| M6 Public beta (all packs) | P10 | 216 | January 2031 |

### 2.1 Scope Options (owner decision)

The core is large for one person. These options shorten it. Each is a decision for the owner.

| Option | Change | Hours | Effect on M5 | Trade-off |
| --- | --- | --- | --- | --- |
| A | Move the Windows engine (P6-T05, P6-T06) to after M5 | 230 saved | About 9 weeks earlier | Not chosen: owner answer Q15 keeps the Windows engine in Personal Edition |
| B | Limit default apps to Text, Calculator, Images, Terminal, and Player | 150 saved | About 6 weeks earlier | Fewer built-in apps at M5 |
| C | Hire outside help for the Windows engine packaging, media libraries, or the Android bridges | Varies | Depends on help | Cost; coordination |
| D | Keep AI after M5 (current plan) | None | None | None |
| E | Android before M5 (Q22) | Adds 620 | M5 moves to about June 2030 | Larger core at M5; more to test |
| F | Android after M5 (current plan) | None | None | Android arrives later |

Recommended: keep the plan as written (options D and F), and review options A and B after the P1 re-estimate.

## 3. Step-by-Step Tasks

Each task has an ID. Use the ID in progress logs and commit messages.

### P0 — Foundation, Spikes, and Verification Setup (120 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P0-T01 | Confirm requirements (owner answers Q1 to Q7) | README owner decisions table | Done (2026-11-16) |
| P0-T02 | Review each decision in 01 section 6 and addendum 1 D2 section 9 (ADR-001 to ADR-020); owner accepts or changes each one | ADR statuses | Done (2026-11-17, implementation-1): owner accepted ADR-001 to ADR-023 (spike safeguards noted on ADR-003, 004, 009, 021); statuses in D2 section 9 |
| P0-T03 | Legal notes for Personal Edition: licenses of each component (02 section 3.1), including the Android container engine and Wine | Notes in 02 | Done (2026-11-17, implementation-1): verified against project sources, recorded in 02 section 3.1; public-release re-check stays in P0-T03b |
| P0-T03b | Deferred to public release: trademark search; Slint license terms; Secure Boot key policy; Android container license review | Notes in 02 | Deferred |
| P0-T04 | Hardware list: owner's own PCs and one spare test machine (Q9) | Hardware list in 04 section 6 | Done (2026-11-18, implementation-2): Reference 1 = spare Dell laptop (i5, integrated graphics, 24 GB RAM, Windows installed; primary OS test machine); dev host recorded; remaining machine details filled by P2-T08 coverage report |
| P0-T05 | Repository layout and CI skeleton: `os-build/`, `compositor/`, `shell/`, `lifecycled/`, `powerd/`, `installer/`, `apps/`, `docs/` | Repository with empty build | Done (2026-11-17, implementation-1): workspace builds and tests pass; CI runs on first push |
| P0-T06 | Linux test environment on the Mac (Q9): Linux x86-64 virtual machine with UEFI and BIOS, plus the spare PC | Boot test script | Done (2026-11-17, implementation-1): Alpine x86-64 placeholder boots in QEMU in both BIOS and UEFI modes (results log `docs/2026/11/17/results/p0-t06-boot-test.md`); spare-PC validation moves to P2 |
| P0-T07 | Spike S1: Slint draws a layer-shell surface and requests a background blur region | Spike report | In progress (2026-11-18, implementation-3): owner approved VM route; headless-first approach (runner VM). Research finding: winit has no layer-shell (winit#2582), so the spike uses Slint custom-platform + software renderer over zwlr_layer_shell_v1, blur via org-kde-blur if the host compositor exposes it |
| P0-T08 | Spike S2: minimal Smithay compositor shows one client window | Spike report | In progress (2026-11-18, implementation-3): owner approved VM route; headless-first — Smithay dummy backend + Wayland socket, small client draws one window; CI workflow to be added |
| P0-T09 | Spike S3: `cgroup.freeze`, `memory.reclaim`, and PSI all work on the test kernel | Spike report with numbers | Done (2026-11-18, implementation-2, partial result): freeze Pass, memory.reclaim Pass (~40 MB reclaimed on demand); PSI absent from the Alpine virt kernel (no `/proc/pressure`, even with `psi=1`) — `CONFIG_PSI=y` required in the P1-T01 kernel fragment, PSI re-test folded into P1. Report: `docs/2026/11/18/results/p0-t09-spike-s3.md` |
| P0-T10 | Spike S4: battery state through UPower and sysfs; charge-limit controls | Spike report | Readings match system tools |
| P0-T11 | Verification setup: budgets file (`os-build/budgets.toml`), results log folder, metrics format (addendum 1 D6; addendum 2 D10) | Budgets file; results folder | Done (2026-11-17, implementation-1): budgets file, metrics format (`os-build/metrics/FORMAT.md`), and `os-build/check-budgets.py` wired into CI; verified pass and over-limit fail cases locally |
| P0-T12 | Spike S5: one Windows test program through the Wine engine in a Linux virtual machine | Spike report with startup time | In progress (2026-11-18, implementation-3): headless-first in the CI Linux VM — mingw-w64-built test exe through Wine, startup time recorded; GUI-app run under the graphical VM follows |

Exit criteria: all ADRs decided; spikes S1 to S5 reported; test environment and budgets working.

### P1 — Base System Boots and Boot Speed (160 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P1-T01 | Buildroot configuration and kernel fragment: UEFI EFI stub, legacy BIOS, cgroup v2, PSI, zram, squashfs, ext4, Btrfs, DRM for Intel and AMD, tickless idle, runtime power management, virtio, **binder IPC support (for Android, P6-AND)**; built-in drivers for owner's hardware | `os-build/` config | Done (2026-11-18, implementation-2): fragment + defconfig verified green in CI (`kernel-config` and `buildroot-build` confirmed by owner); owner-hardware built-in driver list is refined in P2 from the coverage report |
| P1-T02 | Init: systemd minimal profile. Compare with runit or s6 | ADR-003 decision | In progress (2026-11-18, implementation-3): CI boot test added (boots the built image to login on serial); busybox-init comparison job added (dispatch-only). Done when the owner confirms the CI boot test green |
| P1-T03 | Read-only squashfs root with overlay for `/var` and `/home` | Mount plan (D4) | In progress (2026-11-18, implementation-3): rootfs overlay with tmpfs `/var` and `/home` added; CI boot test checks mounts. Done when CI confirms mounts on the read-only root |
| P1-T04 | Networking with NetworkManager (or systemd-networkd with iwd) | Working DHCP in the virtual machine | In progress (2026-11-18, implementation-3): NetworkManager in the defconfig; CI boot test checks DHCP on eth0. Done when CI confirms DHCP lease in the VM |
| P1-T05 | UEFI boot chain: systemd-boot with a unified kernel image (UKI) | Bootable EFI image | In progress (2026-11-18, implementation-3): systemd-boot + EFI-stub UKI with embedded cmdline; genimage ESP+rootfs disk (`aetheros-efi.img`); CI OVMF boot test added. Done when CI confirms the UEFI chain boots |
| P1-T06 | Measure boot time and base image size | Numbers in D6 results log | Base image ≤ 150 MB compressed |
| P1-T07 | Legacy BIOS boot path with GRUB for MBR disks | Bootable BIOS image | In progress (2026-11-18, implementation-3): GRUB2 i386-pc, ext4 /boot + MBR-dos layout (`aetheros-bios.img` via make-bios-image.sh); CI SeaBIOS boot test added. Done when CI confirms the BIOS chain boots |
| P1-T08 | Boot speed: stage timing (addendum 1 F11 section 1.2), service start order, boot-time report (BT-09), trimming of unused kernel parts | Boot-time report | Stage budget met in the virtual machine; report produced |

Exit criteria (M1): boots in a virtual machine to login in ≤ 10 s; base image ≤ 150 MB compressed; UEFI and legacy BIOS boot. Re-estimate P2 to P4 here.

### P2 — Hardware Bring-Up (250 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P2-T01 | Firmware for the owner's machines (linux-firmware subset) | Firmware list in D4 | Hardware detected |
| P2-T02 | Graphics: kernel mode setting, Mesa for Intel and AMD, Vulkan (ANV, RADV) | Working graphics | Native resolution displayed |
| P2-T03 | Wi-Fi and Bluetooth: connect, pair, reconnect after sleep | Working | 04 section 6 items pass |
| P2-T04 | Audio with PipeWire: output, input, HDMI audio | Working audio | Checklist passes |
| P2-T05 | Input: touchpad, keyboard backlight, brightness keys | Working input | Checklist passes |
| P2-T06 | Sleep and resume: suspend-to-idle, lid switch | Working sleep | 10 suspend cycles without failure |
| P2-T07 | Update the hardware matrix | Table in 04 section 6 | Recorded |
| P2-T08 | Hardware coverage report (Working, Partly, Needs firmware, No driver) | Report tool and sample | Lists all devices on test machines |

Exit criteria (M2): owner's real machine passes the checklist (04 section 6). Coverage report works.

### P3 — Compositor, Window System, and No-Hang Design (540 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P3-T01 | Smithay compositor skeleton: DRM/KMS, input, Wayland core protocol (from spike S2) | `compositor/` runs | Test client displayed |
| P3-T02 | XDG shell, layer shell (menu bar and dock), multi-monitor, HiDPI | Multi-monitor works | Two monitors at different scales |
| P3-T03 | XWayland for X11 apps | X11 test app runs | Runs |
| P3-T04 | Window management: floating, snap, tiling, workspaces (F1) | Features in F1 | F1 acceptance passes |
| P3-T05 | Glass material pipeline and effect tiers (D1) | Tier switch | 60 fps with 3 windows at Full tier |
| P3-T06 | Frame scheduling and damage tracking | Lower idle GPU use | Idle GPU use measured |
| P3-T07 | Direct scanout for full-screen apps | Full-screen bypasses composition | Measured in 04 section 11.3 |
| P3-T08 | No-hang design: asynchronous app updates, Wayland ping and "Not responding", shell watchdog, frame-gap logger (addendum 1 F11 section 3) | Hang handling | G-NH2 passes with a test app that stops responding |

Exit criteria: compositor targets met; G-NH2 passes.

### P4 — Desktop Shell, Default Apps, and Default Browser (1,170 hours)

Desktop shell block (650 hours):

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P4-T01 | Menu bar with system tray and clock | `shell/menubar` | F1 acceptance |
| P4-T02 | Dock with running indicators and auto-hide | `shell/dock` | F1 acceptance |
| P4-T03 | Launcher and search | `shell/launcher` | F1 acceptance |
| P4-T04 | Control center (Wi-Fi, Bluetooth, sound, brightness, Focus, battery and power mode) | `shell/controlcenter` | F1 acceptance |
| P4-T05 | Notification center and Focus modes | `shell/notifications` | F1 acceptance |
| P4-T06 | Lock screen and login (greetd with a custom greeter) | `shell/lockscreen` | F1 acceptance |
| P4-T07 | Settings app generated from the configuration schema (F4) | `shell/settings` | Every setting maps to a file key |
| P4-T08 | Files app | `apps/files` | F1 acceptance |
| P4-T09 | Overview (all windows and workspaces) | `shell/overview` | F1 acceptance |

Default apps and browser block (520 hours, addendum 1 F8):

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P4-T10 | Text app (DA-01) | `apps/text` | DA-AC-01 and DA-AC-02 pass |
| P4-T11 | Calculator (DA-02) | `apps/calculator` | DA-AC-03 passes |
| P4-T12 | Images (DA-03) | `apps/images` | DA-AC-04 passes |
| P4-T13 | Documents, PDF viewer (DA-04) | `apps/documents` | DA-AC-05 passes |
| P4-T14 | Player (DA-05) and codec decision | `apps/player` | DA-AC-06 passes |
| P4-T15 | Terminal (DA-06) | `apps/terminal` | DA-AC-07 passes |
| P4-T16 | Calendar and Clock (DA-07) | `apps/calendar` | Basic checks pass (v1) |
| P4-T17 | Archive (DA-08) | `apps/archive` | DA-AC-08 passes |
| P4-T18 | Browser core (DA-13): lightweight engine chosen in this task; tabs, bookmarks, history, downloads, private window (60 h) | `apps/browser` | DA-AC-12 passes |
| P4-T18a | Browser media and streaming: hardware-accelerated playback; streaming sites; codec and DRM policy (Q24) (40 h) | Browser media support | DA-AC-11 passes |
| P4-T18b | Browser bookmarks, history, downloads, and security update channel (20 h) | Browser data features | Basic checks pass |

Exit criteria (M3): daily-driver checklist (04 section 5) passes for 7 consecutive days; DA-AC-09 and DA-AC-10 pass.

### P5 — Process, Power, Activity Manager, Battery Analytics, and Screen Time (900 hours)

Process management block (530 hours):

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P5-T01 | App identity and one cgroup scope per app | `lifecycled/` identity module | Each app has its own scope |
| P5-T02 | Lifecycle state machine (addendum 1 D3) | State machine and unit tests | Every transition tested |
| P5-T03 | Lifecycle protocol for apps (will and did suspend or resume) | SDK note and test app | Test app saves state on suspend |
| P5-T04 | Freeze and hibernate using `cgroup.freeze` and `memory.reclaim`; zram tuning | Working hibernation | G-PM3 passes |
| P5-T05 | Pressure-driven killer with priority bands (D3 section 6) | Killer module | G-PM4 passes |
| P5-T06 | Exceptions (audio, downloads, VPN, calls) and background task budget | Exception rules | Audio plays in background for 10 minutes |
| P5-T07 | Activity Manager (F13): Windows-style tabs, full metrics, end task, efficiency mode, export. Includes spike S7 (sensor and GPU readings) at the start. Replaces "Activity". | `apps/taskmanager` | TM-AC-01 to TM-AC-10 pass; G-TM1 and G-TM2 pass |
| P5-T08 | 50-app benchmark and 72-hour soak test harness | `lifecycled/bench/` | G-PM1 and G-PM5 pass |

Battery management block (250 hours):

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P5-T09 | `nexa-powerd` policy engine on UPower and power-profiles-daemon (D5) | Policy engine and tests | Profile changes with AC and battery (G-BAT3) |
| P5-T10 | Runtime power management tuner with exclusion list and automatic revert (D5 section 9) | Tuner module | Tuned state reverts on device error |
| P5-T11 | Battery telemetry and time-remaining estimate with smoothing (D5 section 6) | Estimator and tests | Estimate within 10% after 10 minutes |
| P5-T12 | Charge limit control where supported (F7 PWR-08) | Limit control | Limit applied and reported |
| P5-T13 | Low-battery actions and sleep policy, including suspend-then-hibernate option (D5 sections 7 and 8) | Actions and sleep config | Each threshold action works in test |
| P5-T14 | Battery benchmark (04 section 11.2) and battery view in the control center | Benchmark script and UI | G-BAT1 and G-BAT2 pass |
| P5-T15 | Energy-impact column (F7 PWR-12) | Activity Manager column | Shows relative energy per app |

Battery analytics and screen time block (120 hours, addendum 2 F14):

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P5-T16 | Battery analytics: cycle count (spike S8 at the start), health trend, estimated cycles, sessions, temperature history, standby drain, energy by app (F14 section 2.1) | Battery history store and Battery tab | BAT-AC-01 to BAT-AC-05 pass; G-BAT4 and G-BAT5 pass |
| P5-T17 | Screen time (basic): screen-on time, time per app, pickups, notifications count (F14 section 2.2) | Screen Time view | ST-AC-01 to ST-AC-03 pass; G-ST1 passes |

Exit criteria (M4): G-PM1 to G-PM5 pass on two machines (one 8 GB, one 4 GB); G-BAT1 and G-BAT2 pass on the owner's laptop; G-TM1 and G-TM2 pass.

### P6 — Internal App Platform (610 hours, addendum 1 F9 and D8)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P6-T01 | Package format and manifest (D8 section 2) | Format spec and inspector | Inspects sample bundles |
| P6-T02 | Native bundle install and run (D8 sections 4 and 5) | `apps/pkg`, `apps/runner` | AP-AC-01 passes |
| P6-T03 | AppImage runner | Runner support | AP-AC-02 passes |
| P6-T04 | Sandbox: Landlock, namespaces, seccomp; permission broker and prompts | `apps/sandbox`, permission service | AP-AC-05 passes |
| P6-T05 | Windows engine packaging, versioning, prefix manager (Pack B; scope option A) | Engine package and prefix tool | AP-AC-04 passes |
| P6-T06 | Windows installer flow and uninstall | Install flow | AP-AC-03 passes |
| P6-T07 | Cross-platform bundle support and samples | Samples | Each sample launches |
| P6-T08 | macOS package detection and explanation message | Detector | AP-AC-06 passes |
| P6-T09 | Software app (basic list: install, update, remove, search) | `apps/store` | AP-AC-08 passes |
| P6-T10 | Compatibility database and the 40-app Windows test (04 section 7) | Results in F3 | 30 of 40 in Tier A or B |

### P6-AND — Android Apps, Pack A (620 hours, after M5 by default; addendum 2 F12 and D9)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P6-AND-T01 | Spike S6: container runs on the host kernel; per-app windows possible; binder support; container memory baseline; warm-start time (40 h) | Spike report | Pass confirms AND-04; fail sets the one-window fallback |
| P6-AND-T02 | Container image build and integration; kernel binder config checked (60 h) | Pack image | Container starts |
| P6-AND-T03 | Lifecycle manager: states, lazy start with stage messages, keep-ready, freeze, caps (D9 sections 3 and 5) (90 h) | `nexa-android` | AND-AC-03, AND-AC-05, AND-AC-06 pass |
| P6-AND-T04 | Window bridge: per-app windows with our decorations, launcher and dock entries (100 h) | Window bridge | AND-AC-01, AND-AC-02 pass |
| P6-AND-T05 | Input bridge and Android key mapping (back, long press) (40 h) | Input bridge | AND-17 mapping works |
| P6-AND-T06 | Audio through PipeWire and clipboard bridge (60 h) | Bridges | Audio plays; text clipboard works |
| P6-AND-T07 | Notification bridge (50 h) | Bridge | Notifications appear in our center |
| P6-AND-T08 | File access through the picker; optional shared folder (50 h) | Picker integration | Files open only through picker |
| P6-AND-T09 | Store integration: open repository index, APK install, sideload, updates, uninstall (60 h) | Store changes | AND-AC-08 passes |
| P6-AND-T10 | Camera, microphone, and location through the permission broker (40 h) | Permissions | Each permission asked once |
| P6-AND-T11 | microG option install and verification (20 h) | Option | Listed apps work |
| P6-AND-T12 | Performance gates G-AND1 to G-AND5 and Activity Manager Android tab (10 h) | Gate results | Gates pass or limits documented |

Exit criteria: AND-AC-01 to AND-AC-10 pass (or limits documented and accepted by owner).

### P7 — Customization, Developer Tools, and Optional Feature Manager, Basic (240 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P7-T01 | Configuration schema and file layout (F4) | Schema files | Validation passes |
| P7-T02 | Theme token engine with live reload | Theme engine | Change applies without restart |
| P7-T03 | `nexactl` command-line tool (F4 section 6, plus `nexactl metrics export`) | Binary | Commands work |
| P7-T04 | Lua hooks for shell and compositor (basic) | Scripting runtime | Sample script runs |
| P7-T05 | Extension API | Deferred to Later | — |
| P7-T06 | Keybinding file and editing | Keybinding file | Keys remap |
| P7-T07 | Optional feature manager (F15): install and remove packs, cost display, idle budget check (G-OPT1 to G-OPT4) (60 h) | Settings → Features | OPT-AC-01 to OPT-AC-05 pass |

### P7-PK — Optional Packs D to G (160 hours, after M5)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P7-PK-T01 | Pack D: advanced monitoring (per-app network through kernel socket accounting, per-process GPU detail) (50 h) | Pack D | G-TM1 still passes with pack on; G-OPT2 passes |
| P7-PK-T02 | Pack E: Screen Time limits and downtime (40 h) | Pack E | ST-05 and ST-06 work |
| P7-PK-T03 | Pack F: battery reports (text reports, long history) (30 h) | Pack F | BAT-10 works |
| P7-PK-T04 | Pack G: storage analyzer (disk map, large file finder) (40 h) | Pack G | Scan results match `du` within 1% |

### P7-AI — Optional AI Assistant, Pack C (220 hours, after M5)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P7-AI-T01 | Runtime integration of the internal inference engine (addendum 1 D8 section 6) (80 h) | Runtime service | Model loads and generates in test |
| P7-AI-T02 | Model package and opt-in install and remove (addendum 1 F10 AI-01, AI-02, AI-10) (50 h) | Install flow | AI-AC-01, AI-AC-02, AI-AC-07 pass |
| P7-AI-T03 | Text assistant actions (AI-03) (40 h) | Actions in Text | AI-AC-08 passes |
| P7-AI-T04 | Launcher ask bar (AI-04) (30 h) | Ask bar | One-turn answers work |
| P7-AI-T05 | Unload when idle, battery rule, stop button, hardware gate (AI-05 to AI-09) (20 h) | Lifecycle rules | AI-AC-03 to AI-AC-06 pass; G-AI1 to G-AI3 pass |

### P8 — Installer and Dual Boot (260 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P8-T01 | Live image that boots from USB | ISO | Boots from USB |
| P8-T02 | Disk, Windows, EFI, and BitLocker detection | Detection report | Correct on the owner's Windows machines |
| P8-T03 | Partition planner with free-space use and NTFS shrink checks | Planner | Dry run matches result |
| P8-T04 | Bootloader install with Windows entry (UEFI and legacy BIOS) | Boot menu with both systems | INS-AC-04 and INS-AC-09 pass |
| P8-T05 | Secure Boot path | Deferred to public release | — |
| P8-T06 | Uninstall and repair | Uninstall mode | Windows boots after uninstall |
| P8-T07 | Backup prompt and pre-checks (BitLocker, Fast Startup, battery) | Pre-check screens (F5) | Warnings appear |

Exit criteria (M5): 10 successful install and uninstall cycles on 3 Windows 11 machines (owner's machines); G-RES1, G-RES4, G-APP1, and G-OPT1 pass. Personal Edition is ready.

### P9 — Updates, Security, Hardening (220 hours, mostly deferred)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P9-T01 | A/B updates with RAUC | Update system | Update installs |
| P9-T02 | Rollback and health checks | Health check service | Failed update rolls back |
| P9-T03 | Optional disk encryption (LUKS2) in the installer | Option | Encrypted install boots |
| P9-T04 | Firewall (default deny inbound) and sandbox defaults | Firewall rules | Test ports closed |
| P9-T05 | Checksums for Personal Edition images; signing pipeline deferred | Checksums | Checksum verified |
| P9-T06 | Security review and fuzzing of installer, package service, Android container bridge, and `nexa-lifecycled` | Report | No critical findings |

### P10 — Beta and Public Release (150 hours, deferred)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P10-T01 | Hardware matrix with 10 or more machines (HW-COV-03) | Matrix | Recorded |
| P10-T02 | Performance and battery tuning against gates | Tuning notes | Gates met or documented |
| P10-T03 | User guide and developer guide | Documents | Reviewed |
| P10-T04 | 30-day dogfooding on the owner's daily driver | Log | 30 days completed |
| P10-T05 | Public release image, signatures, release notes | Release | Beta criteria (04 section 9) met |

## 4. Progress Tracker

Update this table whenever a task changes state. Allowed statuses: Not started, In progress (session and date), Blocked (reason), Done (date), Deferred (reason).

| Phase | Status | Owner | Last updated |
| --- | --- | --- | --- |
| P0 Foundation, spikes, verification setup | In progress (Done: T01, T02, T03, T04, T05, T06, T09 (partial: PSI re-test in P1), T11. T07/T08/T12 In progress via CI Linux VMs (implementation-3). T10 blocked on real battery) | Owner | 2026-11-18 |
| P1 Base system boots and boot speed | In progress (T01 Done, owner-confirmed green CI; T02/T03/T04/T05/T07 In progress, CI boot-test evidence pending owner confirmation; T06/T08 numbers from CI logs next) | Owner | 2026-11-18 |
| P2 Hardware bring-up | Not started | Owner | 2026-11-17 |
| P3 Compositor, window system, no-hang design | Not started | Owner | 2026-11-17 |
| P4 Desktop shell and default apps | Not started | Owner | 2026-11-17 |
| P5 Process, power, Activity Manager, battery analytics | Not started | Owner | 2026-11-17 |
| P6 Internal app platform | Not started | Owner | 2026-11-17 |
| P7 Customization and optional feature manager (basic) | Not started | Owner | 2026-11-17 |
| P8 Installer and dual boot | Not started | Owner | 2026-11-17 |
| P6-AND Android apps (Pack A) | Not started (after M5 by default; Q22) | Owner | 2026-11-17 |
| P7-PK Optional packs D to G | Not started (after M5) | Owner | 2026-11-17 |
| P7-AI Optional AI assistant (Pack C) | Not started (after M5) | Owner | 2026-11-17 |
| P9 Updates, security, hardening | Deferred parts; not started | Owner | 2026-11-17 |
| P10 Beta and public release | Deferred | Owner | 2026-11-17 |

### Progress Log

| Date | Session | Change |
| --- | --- | --- |
| 2026-11-16 | Planning session 1 | Created documentation set (README, 01 to 04, features F1 to F6, design D1 to D4). |
| 2026-11-16 | Planning session 2 | Applied owner answers Q1 to Q7. Language Rust (ADR-013), shell Slint pending spike S1 (ADR-004), battery management added, speed targets, legacy BIOS boot, hardware coverage goal, hour-based estimates for one person, personal-first scope. P0-T01 marked Done. |
| 2026-11-17 | Planning session 3 (addendum 1) | Answered five questions and added three requirements. Added F8 to F11 and D6 to D8. Added default apps, app platform, optional AI, boot speed, and no-hang design to the plan. |
| 2026-11-17 | Planning session 4 (addendum 2) | Added Android app support (F12, D9), Activity Manager (F13, D10 monitoring), battery analytics and screen time (F14), and optional packs (F15). Activity Manager replaces Activity. Android and optional packs placed after M5. Added spikes S6 to S8, Q18 to Q22, and risks R23 to R28. |
| 2026-11-17 | Planning session 5 | Owner answers Q8 to Q17 recorded in the main README (AetherOS name; AetherPad text app; AI pack up to 4 to 8 GB; Windows engine stays in Personal Edition; lightweight default browser). Browser added as P4-T18 family (+120 hours). Totals and dates updated. Open questions Q18 to Q24 listed in the README. |
| 2026-11-17 | Planning session 6 | Owner answers Q18 to Q24 recorded: wants Android limits removed (investigate options), lazy Android start, monitoring app named "Activity Manager", no optional packs on by default, Android after M5, lighter AI default with user choice of larger models, lighter browser engine with HD-or-reduced playback. All open questions resolved. |
| 2026-11-17 | implementation-1 | P0-T05 Done: Cargo workspace created (`aether-compositor`, `aether-shell`, `aether-lifecycled`, `aether-installer` placeholder crates; `apps/`, `os-build/` placeholders; `.github/workflows/ci.yml`). Git repo initialized on `main`. Verified on host: `cargo build --workspace` and `cargo test --workspace` pass (4/4 tests). Rust 1.100.0 installed via Homebrew. CI workflow will run on first push once a remote is added. Package prefix is `aether-` per ADR-012. P0-T02 review prep written to `docs/2026/11/17/os-implementation/p0-t02-adr-review-prep.md` (owner acceptance still required). |
| 2026-11-17 | implementation-1 | P0-T03 Done: licenses verified via cited web research and recorded in 02 section 3.1 (kernel GPL-2.0-only, systemd LGPL-2.1+, Slint GPL-3.0/royalty-free-with-attribution, Wine DLL rule, Waydroid GPL-3.0, and others). P0-T06 Done: QEMU 11.1.3 + sevenzip installed; `os-build/test/boot-test.sh` boots the Alpine 3.24.4 x86-64 placeholder ISO to the login prompt in both BIOS (SeaBIOS) and UEFI (EDK2 via pflash) modes; results in D6 format at `docs/2026/11/17/results/p0-t06-boot-test.md`. Remaining P0: T04 (owner hardware list), T02 (owner ADR acceptance), T07–T12 (spikes, need the VM and component setup). |
| 2026-11-17 | implementation-1 | P0-T11 Done: `os-build/budgets.toml` (limits and targets for G-RES1–4, G-SPD0–2, G-BAT2, G-TM1), `os-build/metrics/FORMAT.md` (fixed text format, aether/ paths per ADR-012), `os-build/check-budgets.py` (reads budgets.toml, compares a metrics file, fails the build over limit), and a CI step running it. Verified locally: pass case exit 0 with expected warnings; over-limit case exits 1; full build + test + budget check green. Remaining P0: T02 (owner), T04 (owner), T07–T10 and T12 (spikes needing a graphical/Linux session). |
| 2026-11-17 | implementation-1 | P0-T02 Done: owner accepted ADR-001 through ADR-023 in D2 section 9 (Groups 1 and 2 accepted as proposed; ADR-004, 009, 021 accepted with spike safeguards — S1/S5/S6 still validate, fallbacks unchanged). D7 kernel note updated. ADR decision log statuses are now all Accepted. Remaining P0: T04 (owner hardware list), T07–T10 and T12 (spikes). |
| 2026-11-18 | implementation-2 | P0-T09 Done (partial result): spike S3 run in the QEMU live-ISO environment via `os-build/test/spike-s3.sh` + `spike-s3.exp` (expect-driven serial console). cgroup.freeze Pass (`frozen 1`, progress stopped); memory.reclaim Pass (67.5 MB → 24.3 MB after a 40 MB reclaim request, rc 0); PSI Not available on the Alpine virt kernel (no `/proc/pressure`, even with `psi=1`) — `CONFIG_PSI=y` goes into the P1-T01 kernel fragment and the PSI check is re-run there. Report: `docs/2026/11/18/results/p0-t09-spike-s3.md`. Two harness failures (frozen-cgroup reuse; TCG-too-slow anonymous-memory hog) recorded per the retry rule; marker hygiene fixed so echoed command text cannot fake a pass. |

## 5. Risk Register

| ID | Risk | Likelihood | Impact | Mitigation | Phase |
| --- | --- | --- | --- | --- | --- |
| R1 | Hardware without a Linux driver (Q2 "any system") | High | High | Hardware coverage report (P2-T08); owner confirms Q10 | P2 |
| R2 | Scope creep (too many features for one person) | High | High | Personal-first scope; optional packs (F15); scope options (2.1); re-plan at each milestone | All |
| R3 | Glass effects too heavy on weak GPUs | Medium | High | Effect tiers (D1); benchmark (D6) | P3 |
| R4 | Windows app compatibility lower than expected | High | Medium | Tiers; dual boot fallback; spike S5 | P6 |
| R5 | Data loss during dual-boot partition resize | Low | Very high | Backup prompt; dry run; BitLocker and Fast Startup checks; 10-cycle test | P8 |
| R6 | Trademark or licensing problem with the name or design | Low now, Medium at public release | High | Deferred task P0-T03b; original assets | Public release |
| R7 | Firmware, graphics drivers, and default apps push the image past 512 MB | Medium | Medium | Firmware subset; budgets file (D6) | P1, P2, P4 |
| R8 | Freezing apps breaks connections or app state | Medium | High | Exceptions (F2); lifecycle protocol; tests | P5 |
| R9 | Single engineer capacity lower than estimated | High | High | Hour-based plan; 30% buffer; scope options; re-estimate at M1 and M3 | All |
| R10 | Security flaws in custom code (installer, services, sandbox, Android bridges) | Medium | Very high | Fuzzing; security review (P9-T06); minimal privileges | P6, P9 |
| R11 | Slint cannot draw layer-shell surfaces or blur regions | Medium | High | Spike S1; iced fallback | P0, P4 |
| R12 | Slint license terms unsuitable for distribution | Low | Medium | Review before public release (P0-T03b) | Public release |
| R13 | Learning curve for Rust compositor (Smithay) and kernel interfaces | Medium | High | Spikes S2 and S3 | P0, P3 |
| R14 | Mac development host cannot run the Linux features under test | Certain | Medium | Linux virtual machine on the Mac; spare PC (Q9) | P0 onward |
| R15 | Battery information missing or wrong on some laptops | High | Medium | Standard sources; "not reported" display; spike S8 | P5 |
| R16 | Hibernation to disk needs a swap file as large as RAM | Medium | Medium | Optional swap file; off by default on small disks | P5, P8 |
| R17 | Hour estimates too optimistic for a first OS project | High | High | Re-estimate after P1 and P3; scope options | All |
| R18 | Windows engine (Wine) integration harder than estimated | High | High | Spike S5; scope option A; Q15 | P0, P6 |
| R19 | Sandbox features differ between kernels or distributions | Medium | High | Fail closed (D8 section 7); test on the chosen kernel | P6 |
| R20 | Default apps take longer than 400 hours | Medium | Medium | Scope option B | P4 |
| R21 | Small AI model quality too low, or model license unclear | Medium | Low | Optional pack; quality test; license check | P7-AI |
| R22 | Boot-time target missed because of slow firmware on some machines | Medium | Low | Firmware time reported separately (addendum 1 F11) | P1, P2 |
| R23 | Android ARM-only apps cannot run without proprietary translators we cannot ship | Certain | High | Stated as a hard limit (F12 section 2); owner-supplied translator for personal use only; store marks them | P6-AND |
| R24 | Android per-app windows not possible with the chosen container | Medium | High | Spike S6 first; one-window fallback documented as a major limit | P6-AND |
| R25 | Android container memory and warm-start too high for the "no impact" requirement | Medium | High | Hard caps (D9 section 5); lazy start; freeze; measure with G-AND2 and G-AND3 | P6-AND |
| R26 | Android container shares the host kernel (weaker isolation than a virtual machine) | Certain | Medium | Same sandbox rules as apps; virtual-machine mode as later option (D9 section 9) | P6-AND |
| R27 | Monitoring overhead makes the Activity Manager or the core heavy | Medium | Medium | Sampling rates (D10 section 3); gate G-TM1; packs for heavy features (F15) | P5, P7 |
| R28 | Battery cycle count, GPU, or sensor values missing on owner's hardware | Medium | Low | Spikes S7 and S8; "not reported" display; no zeros for missing data | P5 |

## 6. Working Rules for Parallel Sessions

1. Before starting a task, change its status in section 4 to "In progress (session and date)".
2. Do not start a task that another session has marked "In progress".
3. When a task finishes, mark it Done with the date and add a progress log line.
4. If a task is blocked, record the reason and the next step.
5. Any change to a decision in D1 to D10 must update its ADR entry and the progress log in the same session.
6. A milestone is Done only when its gates show Pass in the results log (addendum 1 D6 section 7).
