# Project Instructions — AetherOS

These instructions apply to every AI session working in this repository. They
extend, and never override, the user-level instructions in
`~/.config/opencode/AGENTS.md`.

## Autonomous development is the default

1. **Work the roadmap task-by-task, in sequence.** Source of truth:
   `docs/2026/11/16/os-project/03-roadmap-plan.md` (section 3 tasks, section 4
   tracker, section 6 working rules).
2. **Pick the next task yourself**: the first task that is not Done and whose
   dependencies are Done. If the natural next task is blocked on an owner
   decision or missing environment, skip to the next startable task, record
   the block, and keep going — do not idle.
3. **Update the checklists at runtime** (before and after every task):
   - Mark a task "In progress (session name, date)" in the roadmap tracker
     (section 4) before starting it.
   - When it ends, mark it Done or Blocked with the date, and add a line to
     the Progress Log.
   - Keep the session plan (`docs/2026/11/17/os-implementation/plan.md`)
     checklist and progress log synchronized in the same edit batch.
4. **Verify before Done.** A task is Done only when its "Done when" condition
   is verified by the working session itself (build, test, boot test, spike
   run) or by an owner-confirmed CI result. Never claim success from an
   unconfirmed CI run; record "pending owner confirmation" instead.
5. **Record evidence** in D6 format at
   `docs/<year>/<month>/<day>/results/<task-or-gate-id>.md` for anything
   measurable (boot times, image sizes, spike numbers).
6. **Retry rule**: at most 2 retries per failed task or test case, then stop
   that task, record the blocker honestly, and move to the next startable
   task.
7. **Do not stop between tasks.** Continue task-by-task in sequence. Stop only
   when: (a) genuinely required owner input or approval is missing (state
   exactly what is needed), or (b) no startable task remains, or (c) the
   retry budget of the current task is exhausted.
8. **Commit and push after each completed task**, with the task ID (for
   example `P1-T02`) in the commit message. Remote: `origin` =
   `https://github.com/Anikchan/AetherOS.git`, branch `main`.
9. **Parallel sessions**: never start or touch a task another session has
   marked In progress; never modify owner-created files (for example
   `docs/2026/11/17/branding/`) — record and leave them.
10. **Naming**: code uses the `aether-` prefix (ADR-012). Docs keep their
    historical `nexa-` mentions; fix them opportunistically in a docs pass
    only.

## Quick task-state reference

Allowed tracker statuses: Not started, In progress (session and date), Blocked
(reason), Done (date), Deferred (reason).
