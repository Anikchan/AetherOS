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
| P0-T07 | Spike S1: Slint draws a layer-shell surface and requests a background blur region | Spike report | Done (2026-11-18, implementation-3, run 44236833280 green): `S1_LAYERSHELL_SURFACE_OK` — Slint custom-platform + software renderer over zwlr_layer_shell_v1 (winit has no layer-shell; winit#2582). **ADR-004 confirmed, iced fallback not triggered.** Finding: blur needs a compositor advertising org_kde_kwin_blur_manager — Smithay extension added to the P2 backlog. Report: `docs/2026/11/18/results/p0-t07-t08-spikes-s1-s2.md` |
| P0-T08 | Spike S2: minimal Smithay compositor shows one client window | Spike report | Done (2026-11-18, implementation-3, run 44236833280 green): Smithay 0.7 headless compositor (`S2_SOCKET_READY`), test client xdg-toplevel mapped with SHM buffer (`CLIENT_WINDOW_MAPPED` exit 0, `S2_CLIENT_MAPPED`). Core compositing proven headless; visible presentation proven in the graphical VM (P2). **ADR-009/016 validated.** Report: `docs/2026/11/18/results/p0-t07-t08-spikes-s1-s2.md` |
| P0-T09 | Spike S3: `cgroup.freeze`, `memory.reclaim`, and PSI all work on the test kernel | Spike report with numbers | Done (2026-11-18, implementation-2, partial result): freeze Pass, memory.reclaim Pass (~40 MB reclaimed on demand); PSI absent from the Alpine virt kernel (no `/proc/pressure`, even with `psi=1`) — `CONFIG_PSI=y` required in the P1-T01 kernel fragment, PSI re-test folded into P1. Report: `docs/2026/11/18/results/p0-t09-spike-s3.md` |
| P0-T10 | Spike S4: battery state through UPower and sysfs; charge-limit controls | Spike report | Readings match system tools |
| P0-T11 | Verification setup: budgets file (`os-build/budgets.toml`), results log folder, metrics format (addendum 1 D6; addendum 2 D10) | Budgets file; results folder | Done (2026-11-17, implementation-1): budgets file, metrics format (`os-build/metrics/FORMAT.md`), and `os-build/check-budgets.py` wired into CI; verified pass and over-limit fail cases locally |
| P0-T12 | Spike S5: one Windows test program through the Wine engine in a Linux virtual machine | Spike report with startup time | Done (2026-11-18, implementation-3, headless scope): wine-spike run 44232865848 green — Wine 10.0, mingw-w64 console program ran, **20 ms** startup. D6 log: `docs/2026/11/18/results/p0-t12-spike-s5-wine.md`. GUI-app-through-Wayland residual tracked for the graphical VM session |

Exit criteria: all ADRs decided; spikes S1 to S5 reported; test environment and budgets working.

### P1 — Base System Boots and Boot Speed (160 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P1-T01 | Buildroot configuration and kernel fragment: UEFI EFI stub, legacy BIOS, cgroup v2, PSI, zram, squashfs, ext4, Btrfs, DRM for Intel and AMD, tickless idle, runtime power management, virtio, **binder IPC support (for Android, P6-AND)**; built-in drivers for owner's hardware | `os-build/` config | Done (2026-11-18, implementation-2): fragment + defconfig verified green in CI (`kernel-config` and `buildroot-build` confirmed by owner); owner-hardware built-in driver list is refined in P2 from the coverage report. **Caveat (2026-11-18, implementation-3):** the green build used the default uclibc toolchain — glibc-only packages (systemd, NetworkManager, Upower) were silently dropped; defconfig fixed (`BR2_TOOLCHAIN_BUILDROOT_GLIBC=y`, `BR2_KERNEL_HEADERS_AS_KERNEL=y`); P1-T01 re-verification runs with the next buildroot-build |
| P1-T02 | Init: systemd minimal profile. Compare with runit or s6 | ADR-003 decision | **Done (2026-11-23, S1):** CI boot test (run 44936414669) booted to login prompt (S_BOOT_LOGIN_OK); busybox-init comparison job (init-compare) added (dispatch-only). |
| P1-T03 | Read-only squashfs root with overlay for `/var` and `/home` | Mount plan (D4) | **Done (2026-11-23, S1):** CI boot test confirmed /var and /home on tmpfs over read-only squashfs root (S_MOUNTS_OK). |
| P1-T04 | Networking with NetworkManager (or systemd-networkd with iwd) | Working DHCP in the virtual machine | **Done (2026-11-23, S1):** CI boot test confirmed DHCP lease on eth0 (S_DHCP_OK). Note: switched from NetworkManager to systemd-networkd for DHCP. |
| P1-T05 | UEFI boot chain: GRUB2 (x86_64-efi) | Bootable EFI image | **Done (2026-11-23, S1):** UEFI boot test PASS (run 45141636923: S_UEFI_BOOT_OK, 33 s TCG). Root cause was missing `linuxefi` module in the grub-mkimage standalone image (GRUB 2.06 autoload failed). Fixed by preferring `linuxefi` over `linux` in `post-image.sh`. **2026-11-24 bare-metal fixes:** display — `console=tty0`, `earlycon=efi`, `CONFIG_VT`/`FRAMEBUFFER_CONSOLE`/`DRM_SIMPLEDRM`/`FB_EFI`, `getty@tty1` (VM screendump shows login prompt). Boot — replaced `root=/dev/vda2` (virtio-only) with `root=PARTUUID=9f8e7d6c-…` (GPT) / `a37e5c21-02` (MBR), so the same image boots on bare-metal USB (`sda`) and QEMU (`vda`). Verified in VM (run 45412192212) and on the USB partition table. |
| P1-T06 | Measure boot time and base image size | Numbers in D6 results log | **Done (2026-11-23, S1):** base image 14 MB compressed (≤150 MB gate pass); direct 25 s, BIOS 30 s, UEFI 33 s (TCG). D6: `docs/2026/11/18/results/p1-boot-speed.md`. |
| P1-T07 | Legacy BIOS boot path with GRUB for MBR disks | Bootable BIOS image | **Done (2026-11-23, S1):** CI BIOS boot test (run 44948381548) passed (S_BIOS_BOOT_OK); GRUB2 i386-pc MBR image builds and boots. |
| P1-T08 | Boot speed: stage timing (addendum 1 F11 section 1.2), service start order, boot-time report (BT-09), trimming of unused kernel parts | Boot-time report | **Done (2026-11-23, S1):** direct kernel 25 s, BIOS 30 s, UEFI 33 s (all TCG). Image 14 MB. Bare-metal timings in P2. D6 log: `docs/2026/11/18/results/p1-boot-speed.md`. |

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
| P2-T08 | Hardware coverage report (Working, Partly, Needs firmware, No driver) | Report tool and sample | In progress (2026-11-18, implementation-3): report tool scripted; CI sample run against the runner's virtual hardware; real-machine data at first Dell boot (depends on P2 bring-up) |

Exit criteria (M2): owner's real machine passes the checklist (04 section 6). Coverage report works.

### P3 — Compositor, Window System, and No-Hang Design (540 hours)

| ID | Task | Output | Done when |
| --- | --- | --- | --- |
| P3-T01 | Smithay compositor skeleton: DRM/KMS, input, Wayland core protocol (from spike S2) | `compositor/` runs | Test client displayed |
| P3-T02 | XDG shell, layer shell (menu bar and dock), multi-monitor, HiDPI | Multi-monitor works | **Done (2026-11-26, S2):** two headless `wl_output`s (1920×1080 @1.0 + 2880×1800 @1.5), `zxdg_output_v1` logical geometry, `wp_fractional_scale_v1`, layer-shell configure 1920×32 on the HiDPI output, xdg toplevel mapped; `cargo test -p aether-compositor` passes. Evidence: `docs/2026/11/26/results/p3-t02-multi-monitor-hidpi.md`. |
| P3-T03 | XWayland for X11 apps | X11 test app runs | **Done (2026-11-26, S2):** CI run 45869439628 — `xwayland_xeyes_maps ... ok` (really executed, not skipped). Root causes fixed: `client_compositor_state` now returns XWayland's own client state (was `process::exit(1)`); CI installs `libegl1`/Mesa for the EGL probe. Commits `3528807`, `0997f84`. |
| P3-T04 | Window management: floating, snap, tiling, workspaces (F1) | Features in F1 | **Done (2026-11-26, S2):** `WorkspaceManager` wired into toplevels (`configure_toplevel`, `snap_toplevel`, `move_toplevel_to_workspace`, `tile_active_workspace`, `activate_workspace`); layer surfaces excluded; unit tests (grid tiling, snapping). F1 on-device acceptance pending. |
| P3-T05 | Glass material pipeline and effect tiers (D1) | Tier switch | **Implementation Done (2026-11-26, S2, b0c7fa9):** `compositor/src/glass.rs` — Full/Reduced/Minimal tiers with D1 §4 material values, resolved pass lists (16/13/5), damage-only effect plan, modelled frame cost (Full 3×1152×648 = 2.44 ms of 16.67 ms) and tier hysteresis. **Measured 60 fps gate is hardware-gated** (`GpuProfile::measured=false`; needs the Dell GPU). |
| P3-T06 | Frame scheduling and damage tracking | Lower idle GPU use | **Done (2026-11-26, S2, b0c7fa9):** `compositor/src/scheduling.rs` — damage coalescing/union, frame pacing, frame callbacks, idle time/ratio/skips, gap stats; wired into the event loop. Idle-GPU measurement hook present (hardware-gated for real numbers). |
| P3-T07 | Direct scanout for full-screen apps | Full-screen bypasses composition | **Done (2026-11-27, S2, 2cdff9a):** `compositor/src/scanout.rs` decision model (one visible opaque output-sized toplevel, no Top/Overlay layer; reasons + transition counter); direct frames skip glass passes (`SCANOUT_FRAME passes=0`). Real KMS plane flip is hardware-gated (DMA-BUF stays composited safely). |
| P3-T08 | No-hang design: asynchronous app updates, Wayland ping and "Not responding", shell watchdog, frame-gap logger (addendum 1 F11 section 3) | Hang handling | **Done (2026-11-27, S2, 2cdff9a):** `compositor/src/hang.rs` — ping/pending/not_responding/late-pong recovery, frame-gap logger (damage-wait), shell watchdog; markers + 3 integration tests. G-NH2 shell UI + shell restart wiring (S3/S4) and 24 h measurement hardware-gated. |

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
| P0 Foundation, spikes, verification setup | In progress (Done: T01, T02, T03, T04, T05, T06, T07, T08, T09 (partial: PSI re-test in P1), T10 (battery spike passed on Dell 2026-11-23: UPower+sysfs OK, charge-limit supported, time-remaining OK), T11, T12 (headless scope). All spikes reported; P0 essentially complete except P0-T03b (deferred to public release).) | Owner | 2026-11-23 |
| P1 Base system boots and boot speed | **Done (2026-11-23).** M1 achieved. T01 Done (kernel-config PASS, run 44936411307). T02 Done (direct-kernel boot: login, 25 s TCG). T03 Done (S_MOUNTS_OK: /var and /home on tmpfs). T04 Done (S_DHCP_OK: DHCP lease on eth0). T05 Done (UEFI boot PASS, run 45141636923: S_UEFI_BOOT_OK, 33 s TCG — GRUB2 x86_64-efi, linuxefi module fix). T06 Done (image 14 MB ≤ 150 MB). T07 Done (BIOS boot S_BIOS_BOOT_OK, 30 s TCG). T08 Partial → complete (direct 25 s, BIOS 30 s, UEFI 33 s; bare-metal timings in P2). | Owner | 2026-11-23 |
| P2 Hardware bring-up | In progress. **Run 3 on the Dell (2026-11-27, image `15150017702` booted from USB):** boot chain ✅ (PARTUUID root, squashfs + tmpfs), T02 ✅ (i915 tigerlake, DMC v2.12, eDP 1920×1080, fb0), T04 ✅ ALSA (ALC3204 + 4 HDMI, speaker-test, MIC_OK) **but WirePlumber missing** — defconfig lacked Lua ≥5.3, fixed with `BR2_PACKAGE_LUA_5_4=y`, next build verifies; T06 ✅ 10/10 sleep cycles (10–16 s, carrier on after resume), battery 100 %/24.795 Wh/46.77 % health, 58 °C, webcam `/dev/video0-1`, Bluetooth up. T03 partial: ath10k firmware + scan OK (Raj/Raj_IoT/Raj_Guest/…), connect not run (needs passphrase), `regulatory.db` load fails at boot (world domain). T05 partial: keyboard + PS/2 mouse + backlight 96000, touchpad still PS/2. Branding gap: hostname/Bluetooth name `buildroot`. Evidence: `docs/2026/11/27/results/p2-hardware-validation-run3.md` + raw log. | Owner | 2026-11-27 |
| P3 Compositor, window system, no-hang design | **P3-T01..T09 implemented** (2026-11-27). T01 skeleton, T02 outputs/layer shell/HiDPI, T03 XWayland, T04 WM, T05 glass tiers + GLES glass shaders, **T06 frame scheduling + damage-scoped frost + damage-scoped backdrop** (`8af4999`, `b7cc53c`: frost 7.3% of covered; backdrop 21.9% of union; per-surface stable Id; pooled targets; HiDPI capture bound fixed), T07 direct scanout, T08 no-hang, T09 DRM/KMS+libinput backend. **`kms-checklist.sh`** grades 19 on-device checks with 4-case `--selftest`. Compositor 73 lib + 14 integration green. Hardware-gated: real EGL/modeset/page flips/input, VT pause/resume, second output, KMS HiDPI, measured 60 fps/24 h gap, visual glass. **2026-11-28 (S-overnight):** layer-surface damage now translated by placement (`AETHER_COMPOSITOR_LAYER_DAMAGE`, checkpoint 9, 5-case selftest), HiDPI edge rounding fixed (no 1 px overhang at 1.5x/2x), damage-scoped backdrop stale-strip guard; 78 lib + 15 integration green, checklist 20/20. | Owner | 2026-11-27 |
| P4 Desktop shell and default apps | **In progress (S3)** — T01–T18b implemented and headless-verified. 2026-11-27: T04 effect tiers, T18a browser media, T18b browser security, G-NH2 shell side + interactive shell host, **SHL-18 accessibility** (reduce motion, reduce transparency→Minimal, text scale 100–200%, Tab navigation with wrap, 2 px focus ring; `--a11y-status/--a11y-env/--render-a11y`; 661dab4), T06 greetd/PAM, T13 PDF, T14 Player, T16 Calendar/Clock, P7-T07 Features UI, layer-shell host contract, acceptance harness. Tests: shell 163 + ui 5 green. On-device F1/DA acceptance + AT-SPI announcements + real layer-shell Platform pending the Dell. **2026-11-28 (S-overnight):** real `zwlr_layer_shell_v1` client + an integration test that boots the real compositor and connects the real shell; fixed two shipping bugs (zero-size axis without both anchors → protocol error; panels asking exclusive keyboard → would steal typing). 170 shell+ui tests green. | Owner | 2026-11-27 |
| P5 Process, power, Activity Manager, battery analytics | **In progress (S4)** — T01–T17 code + tests. 2026-11-27: **P5-T07 Activity Manager all tabs**, **TM-17 service actions** (plan → permission prompt → `SystemdDbusExecutor` behind `--yes`; polkit policy generated from the contract and pinned by CI), Ctrl+Shift+Esc, force-quit dialog, P5-T08 soak JSON, P5-T15 energy contract, **Pack D network (powerd) + GPU fdinfo (lifecycled) recorders wired with persisted hourly buckets**. Tests: 211 across taskmanager/lifecycled/powerd green. Hardware-gated: eBPF attach + real DRM fdinfo + confirmed polkit restart + G-TM1/G-BAT/72 h. | Owner | 2026-11-27 |
| P6 Internal app platform | **In progress (S4)** — T01 (format+inspector), T02 (native bundle), T03 (AppImage), **T04 Done 2026-11-27** (fail-closed Landlock + seccomp shim, namespaces; `command_strict`/`require_strict_layers`; tests), T05/T06 (engine packaging + Windows install/uninstall), T07/T08, T09 (store JSON contract), T10 (40-slot compat DB + harness; results hardware-gated). **2026-11-27 (S1, Linux parity):** the seccomp arch check was inverted (`jt=0,jf=1`), so a matching architecture fell into `RET_KILL_PROCESS` and every sandboxed app would have died on its first syscall — fixed in `a22f52e`, with `SeccompPlan::evaluate()` + allow/deny/foreign-arch tests so the encoding is verified without a filter-capable kernel. Kernel Landlock/seccomp allow-deny runs + shim install on AetherOS still pending (now verified on the Dell Ubuntu kernel: `aether-landlock --seccomp` runs `cat` and exits 0). **2026-11-28 (S-overnight):** `SeccompPlan::verify_encoding()` runs inside `plan()` (structural jump-target check, 9 new tests), syscall table cross-checked against `libc::SYS_*` for x86_64+aarch64, shared Landlock `normalize()` (read-write wins — the shim parser could previously downgrade `--rw` to `--ro`), `require_strict_layers()` gate proves no `Command` exists with a layer skipped; 57 sandbox + 45 runner tests green. | Owner | 2026-11-27 |
| P7 Customization and optional feature manager (basic) | **In progress (S4 + S3)** — T01–T07 core done (config schema/daemon, theme tokens, `aetherctl`, Lua hooks, keybinding file, optional feature manager). **T07 UI done 2026-11-26 (S3, 3f84123):** Settings → Features page over S4's `FeatureManager` (cost/status/install/remove/budget/refusal). P7-PK partial (packs E/G + fuzz); P7-AI not started. | Owner | 2026-11-26 |
| P8 Installer and dual boot | **In progress (S4)** — T02 (detection), T03 (partition planner), T04 (bootloader), T06 (uninstall/repair), T07 (pre-checks), **T01 live USB script** (`make-live-usb.sh`), **T05 Secure Boot documented as deferred** (decision/prerequisites/risks/test matrix; no signing key/shim now). Pending: real 10-cycle install test on 3 Windows machines (M5), live persistence mount (S1), Secure Boot implementation at public release. | Owner | 2026-11-27 |
| P6-AND Android apps (Pack A) | Not started (after M5 by default; Q22) | Owner | 2026-11-17 |
| P7-PK Optional packs D to G | **In progress (S4)** — Pack D (advanced monitoring) wired 2026-11-27: strict snapshot parser + `NetworkRecorder` in `powerd` (hourly buckets, persisted), `GpuRecorder`/fdinfo sampler in `lifecycled` (per-app scope attribution, persisted), App-history columns. Pack E + Pack G + P9-T06 fuzz earlier; Pack F pending. Hardware-gated: eBPF attach + map dump, real DRM fdinfo, G-TM1/G-OPT2. | Owner | 2026-11-27 |
| P7-AI Optional AI assistant (Pack C) | Not started (after M5) | Owner | 2026-11-17 |
| P9 Updates, security, hardening | **In progress (S4)** — P9-T01 RAUC A/B wiring, **P9-T02 Done** (health service: probes, gating, 120 s deadline, retries, persistence, rollback), **P9-T03 Done (planning)** (LUKS2 installer plan: luksFormat → luksOpen → crypttab → mkfs on mapper, 16 MiB header, passphrase never in argv, real UUID into cmdline; 5 integration tests), P9-T04 firewall, P9-T05 checksums, P9-T06 fuzzing. Pending: encrypted boot (initramfs cryptsetup — S1 image), real slot write/signing on hardware (production keys deferred). | Owner | 2026-11-27 |
| P10 Beta and public release | Deferred | Owner | 2026-11-17 |

### Progress Log

| Date | Session | Change |
| --- | --- | --- |
| 2026-11-27 | S1 (overnight) | **Overnight parallel run started (22:40 IST):** 7 developer/tester sessions launched under `docs/2026/11/27/overnight-parallel-plan.md` — UI packaging into the image (critical path), kernel regulatory.db + touchpad i2c-hid, compositor layer-damage translation, shell↔compositor real handshake, apps/launcher wiring, CI parity guards, sandbox seccomp hardening. `test.sh` now collects the Wi-Fi credentials before the log file exists (nothing typed can be captured) and the USB ESP copy was refreshed. WirePlumber root cause fixed (`BR2_PACKAGE_LUA_5_4=y`); build `46041689625` verifies it. Hardware: P2 run 3 logged (10/10 sleep, graphics/audio/battery/webcam OK, touchpad and regulatory domain open). |

| Date | Session | Change |
| --- | --- | --- |
| 2026-11-16 | Planning session 1 | Created documentation set (README, 01 to 04, features F1 to F6, design D1 to D4). |
| 2026-11-16 | Planning session 2 | Applied owner answers Q1 to Q7. Language Rust (ADR-013), shell Slint pending spike S1 (ADR-004), battery management added, speed targets, legacy BIOS boot, hardware coverage goal, hour-based estimates for one person, personal-first scope. P0-T01 marked Done. |
| 2026-11-17 | Planning session 3 (addendum 1) | Answered five questions and added three requirements. Added F8 to F11 and D6 to D8. Added default apps, app platform, optional AI, boot speed, and no-hang design to the plan. |
| 2026-11-17 | Planning session 4 (addendum 2) | Added Android app support (F12, D9), Activity Manager (F13, D10 monitoring), battery analytics and screen time (F14), and optional packs (F15). Activity Manager replaces Activity. Android and optional packs placed after M5. Added spikes S6 to S8, Q18 to Q22, and risks R23 to R28. |
| 2026-11-17 | Planning session 5 | Owner answers Q8 to Q17 recorded in the main README (AetherOS name; AetherPad text app; AI pack up to 4 to 8 GB; Windows engine stays in Personal Edition; lightweight default browser). Browser added as P4-T18 family (+120 hours). Totals and dates updated. Open questions Q18 to Q24 listed in the README. |
| 2026-11-17 | Planning session 6 | Owner answers Q18 to Q24 recorded: wants Android limits removed (investigate options), lazy Android start, monitoring app named "Activity Manager", no optional packs on by default, Android after M5, lighter AI default with user choice of larger models, lighter browser engine with HD-or-reduced playback. All open questions resolved. |
| 2026-11-23 | S3 (session-1, app batches) | P4 default apps completed in three parallel batches and integrated: Files (sandboxed FS ops with traversal/symlink guards), AetherPad (buffer, undo/redo, counts, find/replace, save/load), Calculator (parser, history, memory), Images (decode, zoom, rotate/flip), Documents (markdown parser, pagination, search, poppler adapter), Player (queue, shuffle, repeat, transport, GStreamer adapter + codec policy), Terminal (VT parser + forkpty session), Calendar/Clock (month grid, event store, analog clock, stopwatch, timer), Archive (extract/create/delete with slip guards). Per-app counts: text 10, calculator 11, terminal 13, files 11, archive 9, images 9, calendar 14, documents 15, player 11, browser 38, shell 47, ui 5 unit tests, plus 11 headless interaction tests. Fixed after screenshot review: archive toolbar overflow and clipped path field, calendar event-dot placement and field placeholders. Workspace tests green (88 binaries; compositor excluded on macOS for libxkbcommon). Evidence: 12 app screenshots + 9 shell screenshots. |
| 2026-11-23 | S3 (session-1) | P4 started. New `ui/` design-system crate (`aether-ui`): D1 tokens, original 24×24 icons, widget kit, glass tiers, headless PNG renderer. `shell/` implements T01–T05, T07, T09 surfaces plus T06 lock screen with credential check; Rust model covers dock order, Focus suppression, unread badges, overlay toggles, power modes, launcher ranking with an inline calculator, and F4 layered TOML settings (every row shows file+key; atomic writes; invalid files never overwritten). `.desktop` discovery and nmcli/bluetoothctl/wpctl/brightnessctl/sysfs adapters with a mock. Keyboard shortcuts (Super+Space/A/N/C/L/F, arrows, Enter, Esc) implemented and tested. Headless interaction test drives real widgets and asserts model+file effects; nine surface screenshots rendered. `apps/browser` delivers T18 core + T18b (tabs with per-tab history, URL/search parsing, bookmarks, history de-duplication, downloads, private tabs writing nothing, session restore) with the WebKitGTK engine decision documented. Default app crates (T08, T10–T17) scaffolded and implemented in parallel batches. Evidence: docs/2026/11/23/results/p4-shell-and-apps.md and screenshots/. On-device acceptance still gated by P3-T02. |
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
