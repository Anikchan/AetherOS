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
2. **P2-T08 (Coverage report):** Run `coverage-report.sh` on the spare Dell. Deliverable: populated device table in 04 §6.2.
3. **P2-T07 (Hardware matrix update):** Update 04 §6.2 with real device data.
4. **P0-T12 (Wine GUI):** Run a Windows GUI app under Wine on the graphical VM. Needs: graphical VM session.
5. **P0-T07 (Slint blur):** Unblock the 6 Wayland API errors. Needs: local Linux type-check. Fix: wayland-protocols-wlr client feature, wayland-backend dep, create_pool BorrowedFd, KDE blur new_id.

## Files this session touches

- `os-build/test/spike-*.sh`, `os-build/test/spike-*.c`
- `os-build/test/coverage-report.sh`
- `docs/2026/11/18/results/p*-spike-*.md`
- `docs/2026/11/18/results/p*-coverage-*.md`
- `docs/2026/11/17/os-implementation/plan.md` (append progress log only)

## Status (2026-11-20)

P0-T10 and P2-T08 blocked on spare Dell availability. Prep work (battery spike script) can proceed. P0-T07 blocked on local Linux type-check environment.

## Coordination

Read `plan.md` before/after work; never start a task another session marked In progress; record progress in `plan.md` only (tracker updates are S1's). Conflicts favor S1 (critical path). S3 and S4 never block S1 or S2. See `session-s1-plan.md`, `session-s3-plan.md`, `session-s4-plan.md` and index `4-session-parallel-plan.md`.