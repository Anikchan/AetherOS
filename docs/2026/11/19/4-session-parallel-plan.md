# 4-Session Parallel Development Plan — Index (AetherOS)

- Date: 2026-11-20
- Owner: Anik
- Coordinator: Session S1 (final merge authority)
- Status: All four sessions' autonomous runs complete (2026-11-20). **Plan of record superseded by v2** (dependency-sliced): `docs/2026/11/22/parallel-plan-v2.md`. v1 retained for history. Billing unblocked (repo public). Dell ready (Ubuntu, SSH, runner, reserved IP). P1: UEFI blocked (retry budget exhausted); real-Dell UEFI investigation is the next S1 path.

## Per-session plans

| Session | Plan file | Territory summary |
|---|---|---|
| S1 | `docs/2026/11/19/session-s1-plan.md` | Critical path: kernel fragment, defconfig, boot chains, CI build workflows, tracker |
| S2 | `docs/2026/11/19/session-s2-plan.md` | Spikes & verification: spike scripts, coverage report, D6 spike/coverage results |
| S3 | `docs/2026/11/19/session-s3-plan.md` | Docs & design: design/, features/, README, boot/results D6 logs |
| S4 | `docs/2026/11/19/session-s4-plan.md` | Tooling & infra: other CI workflows, test scripts, metrics, budgets, Makefile |

Each session reads its own plan file before starting and updates `docs/2026/11/17/os-implementation/plan.md` (append-only progress log) after completing work.

## Shared coordination protocol

1. Before starting any task, read `docs/2026/11/17/os-implementation/plan.md` for recent changes.
2. Do not start a task another session has marked In progress.
3. Mark your task "In progress (session N, date)" in the plan/tracker.
4. On completion, mark Done/Blocked and add a progress-log line; commit and push your own territory (S1 only for defconfig/kernel).
5. Tracker (`03-roadmap-plan.md` §4) is updated only by S1.
6. Conflict resolution: S1 wins (critical path priority). S3 and S4 never block S1 or S2.
7. End-of-sprint merge: S1 reviews all unmerged work, runs `cargo build --workspace` + `cargo test --workspace` + full CI, updates the tracker, commits and pushes.

## Design principle: zero overlap

Each session owns a distinct file territory; no two sessions edit the same file. See each session plan for its "Must NOT touch" list.