# Session S1 Plan — Critical Path (AetherOS)

- Date: 2026-11-20
- Owner: Anik
- Coordinator: this session (S1 has final merge authority)
- Tracker of record: `docs/2026/11/16/os-project/03-roadmap-plan.md` §4 (updated by S1 only)

## Territory (exclusive)

- `os-build/kernel/`
- `os-build/buildroot/`
- `os-build/test/ci-boot*.exp`
- `.github/workflows/buildroot-build.yml`
- `.github/workflows/kernel-config.yml`
- `docs/2026/11/16/os-project/03-roadmap-plan.md` §4 (tracker)
- `docs/2026/11/17/os-implementation/plan.md`

## May read (not own)

Everything.

## Must NOT touch

- `docs/.../design/`, `docs/.../features/` (S3 owns)
- `.github/workflows/` except buildroot-build.yml and kernel-config.yml (S4 owns)
- `os-build/test/spike-*`, `coverage-report.sh` (S2 owns)

## Goal

Own the build verification loop, defconfig, kernel fragment, and boot chains; drive P1 to Done and prepare P2.

## Current tasks (in sequence)

1. **P1 build verification** — **BLOCKED** on GitHub Actions billing (owner action). Re-run `kernel-config` + `buildroot-build` once billing is fixed; on green flip P1-T01/T02/T03/T04/T05/T07 → Done and write P1-T06/T08 D6 results log.
2. **P2 prep (local, no CI):** P2 kernel additions appended to `os-build/kernel/aether.fragment` (sound, cfg80211/mac80211, Bluetooth, input, sleep/resume). Done 2026-11-20.
3. **P2 prep (local, no CI):** P2 Buildroot package additions documented in `os-build/buildroot/aetheros-p2-additions.defconfig`. Merge into active defconfig only after P1 is green. Done 2026-11-20.
4. **P2-T01:** Firmware list (from coverage report data) — waits on S2 (spare Dell).
5. **P0-T10:** Battery spike (needs spare Dell — owner action, S2).

## Files this session touches

- `os-build/buildroot/aetheros_defconfig`
- `os-build/kernel/aether.fragment`
- `os-build/test/ci-boot*.exp`
- `.github/workflows/buildroot-build.yml`
- `.github/workflows/kernel-config.yml`
- `docs/2026/11/16/os-project/03-roadmap-plan.md` (tracker only)
- `docs/2026/11/17/os-implementation/plan.md`

## Status (2026-11-20)

P1 verification blocked on GitHub Actions billing. Local P2 prep complete and pushed (`b4f6cc2`). Next: re-run CI workflows once billing restored.

## Coordination

See shared protocol in `session-s2-plan.md` / `session-s3-plan.md` / `session-s4-plan.md` (identical protocol section) and the index `4-session-parallel-plan.md`. In short: read `plan.md` before/after work; never start a task another session marked In progress; S1 alone updates the tracker and has merge authority; conflicts favor S1.