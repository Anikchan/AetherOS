# 4-Session Parallel Development Plan — AetherOS

- Date: 2026-11-19
- Owner: Anik
- Coordinator: Session 1 (this session)

## Design principle: zero overlap

Each session owns a distinct **file territory**. No two sessions edit the same file.
The roadmap tracker (`03-roadmap-plan.md` section 4) is updated **only by Session 1**.
The session plan (`plan.md`) is updated by all sessions (append-only progress log).

## Session territories

| Session | Territory (exclusive) | May read | Must NOT touch |
|---|---|---|---|
| **S1** (Critical path) | `os-build/kernel/`, `os-build/buildroot/`, `os-build/test/ci-boot*.exp`, `.github/workflows/buildroot-build.yml`, `.github/workflows/kernel-config.yml`, `docs/.../03-roadmap-plan.md` §4 tracker | Everything | `docs/.../design/`, `docs/.../features/` |
| **S2** (Spikes & verification) | `os-build/test/spike-*.sh`, `os-build/test/spike-*.c`, `os-build/test/coverage-report.sh`, `docs/.../results/p*-spike-*.md` | Everything | `os-build/kernel/`, `os-build/buildroot/` |
| **S3** (Docs & design) | `docs/.../design/`, `docs/.../features/`, `docs/.../README.md`, `docs/.../results/p*-boot-*.md`, `docs/.../results/p*-results-*.md` | Everything | `os-build/`, `.github/` |
| **S4** (Tooling & CI) | `.github/workflows/` (except buildroot-build.yml, kernel-config.yml), `os-build/test/*.sh` (except spike-*, coverage-report.sh, ci-boot*.exp), `os-build/metrics/`, `os-build/budgets.toml` | Everything | `os-build/kernel/`, `os-build/buildroot/`, `docs/.../03-roadmap-plan.md` |

## Session 1 — Critical path (this session)

**Owns:** The build verification loop, defconfig, kernel fragment, boot chains.

### Current tasks (in sequence)
1. **P1 build verification** — waiting on run 44347803807 (systemd-networkd DHCP fix)
2. On green: flip P1-T01/T02/T03/T04/T05/T07 → Done
3. Write P1-T06/T08 D6 results log (boot seconds, image size)
4. **P2-T02 prep:** Add Mesa (Intel/AMD) + Vulkan to defconfig
5. **P2-T04 prep:** Add PipeWire + WirePlumber to defconfig
6. **P2-T01:** Firmware list (from coverage report data)
7. **P0-T10:** Battery spike (needs spare Dell — owner action)

### Files this session touches
- `os-build/buildroot/aetheros_defconfig`
- `os-build/kernel/aether.fragment`
- `os-build/test/ci-boot*.exp`
- `.github/workflows/buildroot-build.yml`
- `.github/workflows/kernel-config.yml`
- `docs/2026/11/16/os-project/03-roadmap-plan.md` (tracker only)
- `docs/2026/11/17/os-implementation/plan.md`

### What S1 does NOT touch
- `docs/.../design/` (S3 owns)
- `docs/.../features/` (S3 owns)
- `.github/workflows/` except buildroot-build.yml and kernel-config.yml (S4 owns)
- `os-build/test/spike-*` and `coverage-report.sh` (S2 owns)

---

## Session 2 — Spikes & verification

**Owns:** Spike reports, hardware verification, coverage tool runs.

### Current tasks (in sequence)
1. **P0-T10 (Battery spike):** Run UPower + sysfs battery test on spare Dell.
   - Needs: spare Dell with battery, booted into AetherOS
   - Deliverable: D6 results log with battery readings
2. **P2-T08 (Coverage report):** Run `coverage-report.sh` on spare Dell.
   - Needs: spare Dell booted into AetherOS
   - Deliverable: populated device table in 04 §6.2
3. **P2-T07 (Hardware matrix update):** Update 04 §6.2 with real device data.
4. **P0-T12 (Wine GUI):** Run a Windows GUI app under Wine on graphical VM.
   - Needs: graphical VM session
5. **P0-T07 (Slint blur):** Unblock the 6 Wayland API errors.
   - Needs: local Linux type-check (recipe in plan.md)
   - Fix: wayland-protocols-wlr client feature, wayland-backend dep, create_pool BorrowedFd, KDE blur new_id

### Files this session touches
- `os-build/test/spike-*.sh`, `os-build/test/spike-*.c`
- `os-build/test/coverage-report.sh`
- `docs/2026/11/18/results/p*-spike-*.md`
- `docs/2026/11/18/results/p*-coverage-*.md`
- `docs/2026/11/17/os-implementation/plan.md` (append progress log only)

### What S2 does NOT touch
- `os-build/kernel/`, `os-build/buildroot/` (S1 owns)
- `docs/.../design/` (S3 owns)
- `.github/workflows/buildroot-build.yml` (S1 owns)

---

## Session 3 — Documentation & design

**Owns:** All design docs, feature docs, D6 results logs, README.

### Current tasks (in sequence)
1. **D6 results logs:** Write results docs for completed tasks:
   - P0-T06 (boot test) — already done
   - P0-T09 (spike S3) — already done
   - P0-T12 (Wine) — already done
   - P0-T07/P0-T08 (spikes) — already done
   - P1-T06/T08 (boot speed) — pending CI numbers
2. **Design doc review:** Review and update:
   - `internal-architecture.md` (ADR status, section 9)
   - `power-management-design.md` (D5)
   - `process-manager-design.md` (D3)
   - `ui-design.md` (Slint findings from spike S1)
3. **Feature docs:** Update `features/` based on spike findings
4. **README:** Keep status line current
5. **Opportunistic docs pass:** Fix `nexa-` → `aether-` mentions in docs

### Files this session touches
- `docs/2026/11/16/os-project/design/*`
- `docs/2026/11/16/os-project/features/*`
- `docs/2026/11/16/os-project/README.md`
- `docs/2026/11/18/results/p*-boot-*.md`
- `docs/2026/11/18/results/p*-results-*.md`
- `docs/2026/11/17/os-implementation/plan.md` (append progress log only)

### What S3 does NOT touch
- `os-build/` (S1/S2/S4 own)
- `.github/` (S4 owns)
- `docs/.../03-roadmap-plan.md` §4 tracker (S1 owns)

---

## Session 4 — Tooling & infrastructure

**Owns:** CI workflows (except buildroot-build.yml, kernel-config.yml), build scripts, test harnesses.

### Current tasks (in sequence)
1. **CI improvements:**
   - Add `kernel-config` workflow optimization
   - Add artifact retention cleanup
   - Add Slack/Discord notification on failure (optional)
2. **Build scripts:**
   - `os-build/test/boot-test.sh` improvements
   - `os-build/test/coverage-report.sh` refinements
   - New: `os-build/test/disk-image-test.sh` for testing disk images
3. **Test harnesses:**
   - Improve `spike-s3.sh` for reuse
   - Add `os-build/test/spike-s5.sh` GUI variant
4. **Dev environment:**
   - Document local type-check recipe
   - Add `os-build/Makefile` for common operations
   - Add `os-build/scripts/` for helper scripts

### Files this session touches
- `.github/workflows/` (except buildroot-build.yml, kernel-config.yml)
- `os-build/test/*.sh` (except spike-*, coverage-report.sh, ci-boot*.exp)
- `os-build/metrics/`
- `os-build/budgets.toml`
- `os-build/Makefile` (new)
- `os-build/scripts/` (new)
- `docs/2026/11/17/os-implementation/plan.md` (append progress log only)

### What S4 does NOT touch
- `os-build/kernel/`, `os-build/buildroot/` (S1 owns)
- `os-build/test/spike-*`, `coverage-report.sh`, `ci-boot*.exp` (S2 owns)
- `docs/.../design/`, `docs/.../features/` (S3 owns)

---

## Coordination protocol

### Before starting any task
1. Read `docs/2026/11/17/os-implementation/plan.md` to check for recent changes.
2. Check if another session has marked the task In progress. If so, skip it.
3. Mark your task "In progress (session N, date)" in the tracker/plan.

### After completing any task
1. Mark the task Done/Blocked in the tracker (S1 only) or plan (all sessions).
2. Add a progress-log line.
3. Commit and push (S1 only for defconfig/kernel; all sessions for their own files).

### Conflict resolution
- If two sessions need the same file: S1 wins (critical path priority).
- If S1 is blocked on a task that S2/S3/S4 could help with: S1 delegates via a HANDOFF block in the plan doc.
- S3 and S4 never block S1 or S2.

### Merge protocol (end of sprint)
1. S1 reviews all unmerged work from S2/S3/S4.
2. S1 runs `cargo build --workspace` and `cargo test --workspace` to verify.
3. S1 runs the full CI suite.
4. S1 updates the tracker with final statuses.
5. S1 commits and pushes the merged state.

## Current session assignment

| Session | Current task | Status |
|---|---|---|
| S1 | P1 build verification (run 44347803807) | In progress |
| S2 | P0-T10 battery spike (needs spare Dell) | Blocked on hardware |
| S3 | D6 results logs + design doc review | Ready to start |
| S4 | CI improvements + build scripts | Ready to start |

## What to do right now

- **S1 (this session):** Continue waiting for build 44347803807. On verdict, flip trackers and push.
- **S2:** Prepare battery spike script (os-build/test/spike-s4.sh) and wait for Dell.
- **S3:** Start writing D6 results logs and reviewing design docs.
- **S4:** Start CI improvements and build script enhancements.

## Notes

- The 4-session plan is designed for 4 concurrent AI sessions.
- Each session should read the plan doc before starting and after completing work.
- S1 is the coordinator and has final merge authority.
- No session should modify files outside its territory.
- If a task is blocked, record it and move to the next startable task in your territory.
