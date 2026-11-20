# Session S4 Plan — Tooling & Infrastructure (AetherOS)

- Date: 2026-11-20
- Owner: Anik
- Coordinator: S1 (this session does not update the tracker)

## Territory (exclusive)

- `.github/workflows/` (except buildroot-build.yml, kernel-config.yml)
- `os-build/test/*.sh` (except spike-*, coverage-report.sh, ci-boot*.exp)
- `os-build/metrics/`
- `os-build/budgets.toml`
- `os-build/Makefile` (new)
- `os-build/scripts/` (new)

## May read (not own)

Everything.

## Must NOT touch

- `os-build/kernel/`, `os-build/buildroot/` (S1 owns)
- `os-build/test/spike-*`, `coverage-report.sh`, `ci-boot*.exp` (S2 owns)
- `docs/.../design/`, `docs/.../features/` (S3 owns)

## Goal

Own CI workflows (except S1's), build scripts, test harnesses, and dev environment tooling.

## Current tasks (in sequence)

1. **CI improvements:**
   - Add `kernel-config` workflow optimization
   - Add artifact retention cleanup
   - Add Slack/Discord notification on failure (optional)
2. **Build scripts:**
   - `os-build/test/boot-test.sh` improvements
   - `os-build/test/coverage-report.sh` refinements (coordinate with S2)
   - New: `os-build/test/disk-image-test.sh` for testing disk images
3. **Test harnesses:**
   - Improve `spike-s3.sh` for reuse (coordinate with S2)
   - Add `os-build/test/spike-s5.sh` GUI variant (coordinate with S2)
4. **Dev environment:**
   - Document local type-check recipe
   - Add `os-build/Makefile` for common operations
   - Add `os-build/scripts/` for helper scripts

## Files this session touches

- `.github/workflows/` (except buildroot-build.yml, kernel-config.yml)
- `os-build/test/*.sh` (except spike-*, coverage-report.sh, ci-boot*.exp)
- `os-build/metrics/`
- `os-build/budgets.toml`
- `os-build/Makefile` (new)
- `os-build/scripts/` (new)
- `docs/2026/11/17/os-implementation/plan.md` (append progress log only)

## Status (2026-11-20)

Ready to start. Note: all CI runs currently blocked on billing (S1's blocker); S4's workflow edits can still be made locally and pushed, but they won't execute until billing is restored.

## Coordination

Read `plan.md` before/after work; never start a task another session marked In progress; record progress in `plan.md` only (tracker updates are S1's). Conflicts favor S1. S3 and S4 never block S1 or S2. See `session-s1-plan.md`, `session-s2-plan.md`, `session-s3-plan.md` and index `4-session-parallel-plan.md`.