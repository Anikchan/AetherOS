# 4-Session Parallel Development Plan v2 — AetherOS (dependency-sliced)

- Date: 2026-11-22
- Owner: Anik
- Coordinator: Session S1 (final merge authority)
- Supersedes: `docs/2026/11/19/4-session-parallel-plan.md` (territory-sliced v1)
- Design principle: **dependent tasks live in the same session**, so each session runs its
  slice sequentially without waiting on another session. Cross-session dependencies are
  recorded as "soft blocks" (session does local prep while waiting) rather than hard
  blockers. File territories are non-overlapping so parallel edits never collide.

## Current overall status (2026-11-22)

- **P0:** ~90% done (11/12 tasks; P0-T10 battery spike blocked on Dell hardware, now ready).
- **P1:** T01/T02/T03/T04/T06/T07 Done (verified by CI). T05 (UEFI) Blocked — retry
  budget exhausted on QEMU/OVMF; real-Dell UEFI investigation is the new path. T08 partial
  (direct boot 23–24 s, BIOS 27 s, TCG; bare-metal pending P2). Image 15 MB (gate 150 MB).
- **P2:** Not started. Dell now ready (Ubuntu, SSH, runner, lid-close disabled, reserved IP).
- **P3:** P3-T01 Done (compositor skeleton; CI smoke test passes). P3-T02..T08 not started.
- **P4–P10, P6-AND, P7-PK, P7-AI:** Not started.
- **Core progress:** ~6–7% of 4,250 h; M1 not yet achieved (UEFI blocked).

## Session territories (non-overlapping)

| Session | Owns (exclusive) | May read | Must NOT touch |
|---|---|---|---|
| **S1** Boot & hardware | `os-build/kernel/`, `os-build/buildroot/`, `os-build/test/ci-boot*.exp`, `os-build/board/`, `.github/workflows/buildroot-build.yml`, `.github/workflows/kernel-config.yml`, tracker §4, plan.md | Everything | `compositor/`, `shell/`, `apps/`, `lifecycled/`, `powerd/` |
| **S2** Compositor | `compositor/`, `spikes/`, `os-build/test/spike-*`, spike/coverage results | Everything | `os-build/kernel/`, `os-build/buildroot/`, `shell/`, `apps/`, `lifecycled/` |
| **S3** Desktop shell & apps | `shell/`, `apps/` (except apps/pkg, apps/store, apps/sandbox, apps/runner), `docs/.../features/` | Everything | `os-build/`, `compositor/`, `lifecycled/`, `powerd/`, tracker §4 |
| **S4** Process/power, platform, installer, docs | `lifecycled/`, `powerd/`, `installer/`, `apps/pkg`, `apps/store`, `apps/sandbox`, `apps/runner`, `docs/.../design/`, `docs/.../README.md`, other workflows, metrics, budgets, Makefile, scripts | Everything | `os-build/kernel/`, `os-build/buildroot/`, `compositor/`, `shell/`, tracker §4 |

## Session S1 — Boot & hardware (critical path; unblocks S2/S3/S4)

**Goal:** close P1 (M1) and deliver P2 (M2).

### Task sequence (sequential within S1)
1. **P1-T05 UEFI debug (investigation, not a retry):** root cause of "BdsDxe: failed to load Boot0001 … Not Found" / no serial output. Steps:
   - Inspect the GRUB2 EFI ESP from the latest build artifact (`aetheros-images.zip`).
   - Boot the built image on the Dell's real UEFI firmware (USB or QEMU-on-Dell) and capture serial output.
   - Compare OVMF vs. real firmware behavior; adjust `post-image.sh` (GRUB2 EFI config/console) or the workflow's OVMF invocation.
   - If it works on real hardware, record as "verified on real hardware; QEMU/OVMF limitation noted" and unblock M1.
2. **P1-T08 D6 boot-speed log:** write available numbers (image 15 MB; direct 23–24 s, BIOS 27 s, TCG; UEFI n/a) — **Done 2026-11-23**.
3. **M1 closure:** mark P1 Done once UEFI passes (or document the QEMU-only limitation with owner acceptance).
4. **P2-T01 Firmware list:** fill `p2-t01-firmware-list-template.md` from S2's coverage report; update defconfig firmware subset.
5. **P2-T02 Graphics:** Mesa + Vulkan (ANV, RADV) in defconfig; KMS on Intel/AMD. Merge `aetheros-p2-additions.defconfig` graphics block.
6. **P2-T03 Wi-Fi/Bluetooth:** connect, pair, reconnect after sleep.
7. **P2-T04 Audio (PipeWire):** output, input, HDMI audio.
8. **P2-T05 Input:** touchpad, keyboard backlight, brightness keys.
9. **P2-T06 Sleep/resume:** suspend-to-idle, lid switch, 10 cycles.
10. **P2-T07 Hardware matrix update** (04 §6) + **P2-T08 Coverage report** (tool + sample).
11. **M2 closure:** owner's real machine passes the checklist.

### Soft blocks
- P2-T01/T07/T08 depend on S2 running `coverage-report.sh` on the Dell — S2 does this in parallel; S1 consumes the result when ready.
- P0-T10 (battery spike) is S2's task but S1 updates the tracker.

### Files
`os-build/kernel/`, `os-build/buildroot/`, `os-build/test/ci-boot*.exp`, `os-build/board/`,
`.github/workflows/buildroot-build.yml`, `.github/workflows/kernel-config.yml`,
tracker §4, plan.md.

## Session S2 — Compositor & window system (P3)

**Goal:** deliver the compositor (M3-partial) and unblock S3.

### Task sequence (sequential within S2)
1. **P0-T10 Battery spike (S2 territory):** run `spike-s4.sh` on the Dell; write D6 log.
   (Moved here because it needs the Dell; S2 owns spike scripts.)
2. **P2-T08 Coverage report (S2 territory):** run `coverage-report.sh` on the Dell; refine
   auto-classify "Needs firmware"; write D6 skeleton.
3. **P3-T01** — Done (compositor skeleton, CI smoke test passes).
4. **P3-T02** XDG shell, layer shell, multi-monitor, HiDPI (two monitors at different scales).
5. **P3-T03** XWayland for X11 apps.
6. **P3-T04** Window management: floating, snap, tiling, workspaces (F1 acceptance).
7. **P3-T05** Glass material pipeline and effect tiers (60 fps with 3 windows at Full tier).
8. **P3-T06** Frame scheduling and damage tracking (lower idle GPU use).
9. **P3-T07** Direct scanout for full-screen apps.
10. **P3-T08** No-hang design (async app updates, Wayland ping, "Not responding", watchdog, frame-gap logger; G-NH2).

### Soft blocks
- P3-T05/T06/T07 need real GPU (Dell) for tier validation — S2 develops on the VM and
  validates on the Dell when P2-T02 lands.
- P3-T02 (layer shell) for the shell is needed by S3; S2 delivers it, S3 consumes.

### Files
`compositor/`, `spikes/`, `os-build/test/spike-*`, `os-build/test/coverage-report.sh`,
spike/coverage results.

## Session S3 — Desktop shell & default apps (P4)

**Goal:** deliver the daily-driver desktop (M3).

### Task sequence (sequential within S3)
Shell block:
1. **P4-T01** Menu bar (system tray, clock).
2. **P4-T02** Dock (running indicators, auto-hide).
3. **P4-T03** Launcher and search.
4. **P4-T04** Control center (Wi-Fi, BT, sound, brightness, Focus, battery/power mode).
5. **P4-T05** Notification center and Focus modes.
6. **P4-T06** Lock screen and login (greetd + custom greeter).
7. **P4-T07** Settings app from config schema (F4).
8. **P4-T08** Files app.
9. **P4-T09** Overview (all windows and workspaces).

Default apps + browser block (after shell):
10. **P4-T10** Text app (AetherPad).
11. **P4-T11** Calculator.
12. **P4-T12** Images.
13. **P4-T13** Documents / PDF viewer.
14. **P4-T14** Player + codec decision.
15. **P4-T15** Terminal.
16. **P4-T16** Calendar and Clock.
17. **P4-T17** Archive.
18. **P4-T18** Browser core (tabs, bookmarks, history, downloads, private window).
19. **P4-T18a** Browser media and streaming.
20. **P4-T18b** Browser bookmarks/history/downloads + security update channel.

### Soft blocks
- Depends on S2's compositor (P3-T02 layer shell) for shell surfaces — S2 delivers first;
  S3 consumes and integrates.

### Files
`shell/`, `apps/` (except pkg/store/sandbox/runner), `docs/.../features/`.

## Session S4 — Process/power, app platform, installer, docs (P5–P10)

**Goal:** process & power management, app platform, installer, and documentation.

### Task sequence (sequential within S4)
Process management block:
1. **P5-T01** App identity + one cgroup scope per app.
2. **P5-T02** Lifecycle state machine (+ unit tests).
3. **P5-T03** Lifecycle protocol for apps.
4. **P5-T04** Freeze/hibernate (cgroup.freeze, memory.reclaim, zram).
5. **P5-T05** Pressure-driven killer (priority bands).
6. **P5-T06** Exceptions + background task budget.
7. **P5-T07** Activity Manager (F13) — includes spike S7 (sensor/GPU).
8. **P5-T08** 50-app benchmark + 72-hour soak harness.

Battery management block:
9. **P5-T09** `aether-powerd` policy engine (UPower, power-profiles-daemon).
10. **P5-T10** Runtime power management tuner.
11. **P5-T11** Battery telemetry + time-remaining estimate.
12. **P5-T12** Charge limit control.
13. **P5-T13** Low-battery actions + sleep policy (incl. suspend-then-hibernate).
14. **P5-T14** Battery benchmark + control-center battery view.
15. **P5-T15** Energy-impact column.

Battery analytics + screen time block:
16. **P5-T16** Battery analytics (cycle count S8, health trend, sessions, temp, drain, energy by app).
17. **P5-T17** Screen time (basic).

App platform block:
18. **P6-T01** Package format + manifest.
19. **P6-T02** Native bundle install & run.
20. **P6-T03** AppImage runner.
21. **P6-T04** Sandbox (Landlock, namespaces, seccomp) + permission broker.
22. **P6-T05** Windows engine packaging + prefix manager.
23. **P6-T06** Windows installer flow + uninstall.
24. **P6-T07** Cross-platform bundle support + samples.
25. **P6-T08** macOS package detection.
26. **P6-T09** Software app (basic).
27. **P6-T10** Compatibility database + 40-app Windows test.

Optional packs / AI / installer / updates (after M5):
28. **P7-T01..T07** (config schema, theme engine, nexactl, Lua hooks, keybindings, optional feature manager).
29. **P7-PK-T01..T04** (packs D–G).
30. **P7-AI-T01..T05** (AI assistant pack C).
31. **P8-T01..T07** (installer + dual boot).
32. **P9-T01..T06** (updates, security, hardening).
33. **P10-T01..T05** (beta & public release).

Documentation & design (continuous, interleaved):
34. Design doc review (internal-architecture, power-management, process-manager, ui-design).
35. Feature doc updates, README status, opportunistic `nexa-` → `aether-` pass.
36. D6 results logs as tasks complete.

### Soft blocks
- P5/P6/P7/P8 depend on P4 (daily driver) for full integration, but process/power
  (P5-T01..T06) and the app platform (P6-T01..T04) can start as soon as P3 lands.
- P8 (installer) needs P5/P6 stable.
- P10 needs everything before it.

### Files
`lifecycled/`, `powerd/`, `installer/`, `apps/pkg`, `apps/store`, `apps/sandbox`,
`apps/runner`, `docs/.../design/`, `docs/.../README.md`, other workflows, metrics,
budgets, Makefile, scripts.

## Coordination protocol (same as before)
1. Read `plan.md` before/after work; never start a task another session marked In progress.
2. Mark your task "In progress (session N, date)" in the plan/tracker.
3. On completion, mark Done/Blocked + progress-log line; commit/push your own territory.
4. Tracker (§4) updated only by S1.
5. Conflict resolution: S1 wins (critical path). S3/S4 never block S1/S2.
6. End-of-sprint merge: S1 reviews all unmerged work, runs cargo build/test + full CI,
   updates tracker, commits and pushes.

## Current session assignment

| Session | Current task | Status |
|---|---|---|
| S1 | P1-T05 UEFI debug on real Dell firmware (investigation) + P2-T01 (waits on S2) + P3-T02 | In progress |
| S2 | P0-T10 battery spike + P2-T08 coverage report on Dell | Ready (Dell ready) |
| S3 | P4 shell (P4-T01..) — waits on S2 P3-T02 for layer shell | Prep / wait |
| S4 | P5 process block (P5-T01..) — can start now; docs pass continuous | Ready |

## What to do right now
- **S1:** Debug P1-T05 UEFI on the Dell (real firmware); if it passes, unblock M1 and start P2. Otherwise record and continue with P3-T02.
- **S2:** Run P0-T10 + P2-T08 on the Dell; report results to S1.
- **S3:** Start P4-T01..T03 (menu bar, dock, launcher) — they don't need the compositor's layer shell for initial dev.
- **S4:** Start P5-T01 (app identity + cgroup scope) and the docs/design pass.

## Notes
- This v2 plan groups dependent tasks into the same session so each session runs its slice
  sequentially without cross-session blocking. Cross-session dependencies are recorded as
  "soft blocks" (do local prep while waiting).
- The previous territory-sliced v1 plan remains at `docs/2026/11/19/4-session-parallel-plan.md`
  and the per-session files; v2 supersedes them for task assignment.