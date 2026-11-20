# Session S2 Plan — Spikes & Verification (AetherOS)

- Date: 2026-11-20
- Owner: Anik
- Coordinator: S1 (this session does not update the tracker)

## Territory (exclusive)

- `os-build/test/spike-*.sh`
- `os-build/test/spike-*.c`
- `os-build/test/coverage-report.sh`
- `docs/.../results/p*-spike-*.md`
- `docs/.../results/p*-coverage-*.md`

## May read (not own)

Everything.

## Must NOT touch

- `os-build/kernel/`, `os-build/buildroot/` (S1 owns)
- `docs/.../design/` (S3 owns)
- `.github/workflows/buildroot-build.yml` (S1 owns)

## Goal

Own spike reports, hardware verification, and coverage-tool runs on the spare Dell / graphical VM.

## Current tasks (in sequence)

1. **P0-T10 (Battery spike):** Run UPower + sysfs battery test on the spare Dell. Needs: spare Dell with battery, booted into AetherOS. Deliverable: D6 results log with battery readings.
   - Prep (done 2026-11-20): `os-build/test/spike-s4.sh` created; D6 skeleton
     `docs/2026/11/20/results/p0-t10-spike-s4-battery.md`.
   - Blocked on spare Dell availability (owner action).
2. **P2-T08 (Coverage report):** Run `coverage-report.sh` on the spare Dell. Deliverable: populated device table in 04 §6.2.
   - Prep (done 2026-11-20): tool refined (auto-classify "Needs firmware"
     via `MISSING_DRV`); D6 skeleton
     `docs/2026/11/20/results/p2-t08-coverage-report.md`.
   - Blocked on spare Dell availability (owner action).
3. **P2-T07 (Hardware matrix update):** Update 04 §6.2 with real device data.
   - Blocked on P2-T08 real run (owner action / spare Dell).
4. **P0-T12 (Wine GUI):** Run a Windows GUI app under Wine on a Linux host.
   - Prep (done 2026-11-20): `os-build/test/spike-s5-gui.c` and
     `os-build/test/spike-s5-gui.sh` created; D6 skeleton
     `docs/2026/11/20/results/p0-t12-spike-s5-gui.md`.
   - Run pending (needs Wine + mingw-w64 + Xvfb; CI workflow to be added by S4).
5. **P0-T07 (Slint blur):** Status: **Done** (2026-11-18, run 44236833280
   green — `S1_LAYERSHELL_SURFACE_OK`; blur needs `org_kde_kwin_blur_manager`,
   Smithay extension queued for P2). The six Wayland API errors were fixed and
   the spike compiles on CI (Ubuntu). Local cross-check of s1 is blocked by the
   fontconfig sysroot (not billing); s2 + spike-client type-check clean locally
   (verified 2026-11-20). No further S2 work required.

## Files this session touches

- `os-build/test/spike-*.sh`, `os-build/test/spike-*.c`
- `os-build/test/coverage-report.sh`
- `docs/2026/11/18/results/p*-spike-*.md`
- `docs/2026/11/18/results/p*-coverage-*.md`
- `docs/2026/11/17/os-implementation/plan.md` (append progress log only)

## Status (2026-11-20, S2 autonomous run)

Prep work complete for all startable items:
- `spike-s4.sh` (battery) created; D6 skeleton written.
- `coverage-report.sh` refined (auto-classify Needs firmware); D6 skeleton written.
- `spike-s5-gui.c` + `spike-s5-gui.sh` (Wine GUI) created; D6 skeleton written.
- s2 + spike-client verified to type-check locally on x86_64-unknown-linux-gnu
  (clean, 2026-11-20). s1 cannot be cross-checked locally (fontconfig sysroot);
  compiles on CI (Ubuntu) per run 44236833280.
Hardware-dependent runs remain blocked: P0-T10 (spare Dell + battery),
P2-T08/P2-T07 (spare Dell), P0-T12 GUI (Linux host with Wine + Xvfb / CI).
P0-T07 is Done (not blocked). Tracker updates are S1's.

## Coordination

Read `plan.md` before/after work; never start a task another session marked In progress; record progress in `plan.md` only (tracker updates are S1's). Conflicts favor S1 (critical path). S3 and S4 never block S1 or S2. See `session-s1-plan.md`, `session-s3-plan.md`, `session-s4-plan.md` and index `4-session-parallel-plan.md`.