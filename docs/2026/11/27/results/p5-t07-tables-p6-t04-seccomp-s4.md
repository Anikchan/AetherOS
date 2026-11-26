# P5-T07 table tabs + P6-T04 seccomp — S4 (2026-11-27, round 2)

Session S4 (`session - 4 (Nov26)`), second hardware-independent round.
Follow-up to `p5-t07-tabs-p6-t04-landlock-s4.md` (commit `354aa14`).

1. Remaining Activity Manager tabs — App history, Startup apps, Users, Details,
   Services — as view models in `apps/taskmanager/src/tabs.rs`, with the Slint
   surface and tests.
2. Seccomp filtering as the final P6-T04 sandbox layer: policy, filter plan,
   shim integration, fail-closed enforcement, tests.

No hardware gate is claimed; the Dell hardware validation is still paused.

## 1. Remaining Activity Manager tabs

### What changed

| File | Change |
|---|---|
| `apps/taskmanager/src/tabs.rs` | `TableRow`/`TableView` plus `app_history_view` (24 h + 7-day energy per app with the P5-T15 levels), `startup_view` (scope, state, "Not measured" impact, per-row Enable/Disable action id), `users_view` ("This session" marker), `details_view` (PID/PPID/user/threads/files plus `sandbox_level` from the cgroup), `services_view` (load/active/sub + tone), `table_report` for CLI/D6 evidence |
| `apps/taskmanager/ui/activity-manager.slint` | Generic `DataTable` component (headers, rows with a hint line, tone-coloured status cell, per-row action button, empty state with the honesty note, footer note) and the `table` content block; header/row column alignment fixed |
| `apps/taskmanager/src/window.rs` | `bind_table`, `on_row_action`, demo startup/services/users/sensor data, demo cgroups so the Details tab shows app vs system sandbox levels |
| `apps/taskmanager/src/main.rs` | `--tab app-history\|startup-apps\|users\|details\|services`, `table_for` helper, `--render` now draws nine tabs |
| `apps/taskmanager/tests/tables.rs` (new) | Headless test: table binding for all five tabs, row actions, empty state |
| `apps/taskmanager/README.md` | TM-12/13/14/16/17 status and the remaining limits |

Startup apps is the only table tab with an action: the row button carries the
`.desktop` path and label, and `on_row_action` hands it to the caller, which
persists the change through `set_startup_enabled` (already core-tested).
Services stays read-only in the window; start/stop/restart need the permission
prompt (TM-17).

### Commands and results

```
$ cargo test -p aether-taskmanager
48 lib + 1 tests/tabs.rs + 1 tests/tables.rs + 1 tests/ui.rs = 51 passed, 0 failed

$ cargo run -q -p aether-taskmanager -- --tab startup-apps
STARTUP APPS
  0 of 0 enabled · user and system autostart
  Name   Scope  State  Startup impact
  No autostart entries found (~/.config/autostart and /etc/xdg/autostart).
  Startup impact needs a boot trace and currently reads "Not measured" instead of a guess. ...

$ cargo run -q -p aether-taskmanager -- --render docs/2026/11/27/results/screenshots
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-app-history.png (1080x640) [App history]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-startup-apps.png (1080x640) [Startup apps]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-users.png (1080x640) [Users]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-details.png (1080x640) [Details]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-services.png (1080x640) [Services]
   (+ the four tabs from round 1 still render)
```

### Screenshots (demo data; documented as such)

- `screenshots/activity-manager-app-history.png` — 24 h / 7-day energy per app
  with the contract `High`/`Low` levels and share percentages.
- `screenshots/activity-manager-startup-apps.png` — three autostart entries,
  one disabled, with Enable/Disable buttons and the "Not measured" impact
  column.
- `screenshots/activity-manager-users.png` — two logind sessions, the first
  marked "This session", a missing seat shown as "—".
- `screenshots/activity-manager-details.png` — PID/PPID/user/threads/files plus
  the sandbox level (app scopes accented, system scope muted), command as the
  hint line.
- `screenshots/activity-manager-services.png` — three active units, one
  inactive, one failed (danger tone), descriptions as hints.

## 2. Seccomp filtering (P6-T04 final layer)

### What changed

| File | Change |
|---|---|
| `apps/sandbox/src/seccomp.rs` (new) | `Denial` table (36 syscalls with per-arch numbers and a reason), `SeccompPolicy` (+`without`), architecture check (`AUDIT_ARCH_X86_64`/`AARCH64`), classic-BPF `plan()` (arch mismatch → `KILL_PROCESS`, denials → `EPERM`, else `ALLOW`), `probe()`, `apply()` (raw `prctl` `PR_SET_NO_NEW_PRIVS` + `PR_SET_SECCOMP` with `SECCOMP_MODE_FILTER`) |
| `apps/sandbox/src/enforce.rs` | `SandboxError::{SeccompUnavailable, SeccompFailed}`, `Enforcer::command_strict` (namespaces + Landlock + seccomp, all required, no fallback), `shim_command` shared with `command_with_landlock`, `describe()` reports the seccomp verdict |
| `apps/sandbox/src/landlock.rs` | `ShimInvocation.seccomp`; `parse_shim_args` accepts `--seccomp`; tests for both argument positions |
| `apps/sandbox/src/bin/aether-landlock.rs` | `--seccomp` installs the filter after Landlock and before `execvp`; failure exits 126 without running the program; `--probe --json` reports Landlock **and** seccomp |
| `apps/sandbox/src/bin/aether-sandbox.rs` | `doctor` reports `seccomp` and `strict_launch`; new `seccomp [--arch …]` command prints the filter plan (`SECCOMP_PLAN_OK`) |
| `apps/sandbox/tests/seccomp_enforce.rs` (new) | Shim tests: refusal without kernel support (exit 126, marker absent), ordinary program still runs under the filter, denied `personality()` fails while the unfiltered run succeeds |
| `apps/runner/src/lib.rs`, `src/bin/aether-runner.rs` | `require_strict_layers` (Landlock + seccomp), `require_landlock` kept as an alias, `--strict-sandbox` (old `--strict-landlock` still accepted), test asserts the fail-closed refusal or a both-layers success |
| `apps/sandbox/README.md` | Seccomp section, layers table, commands, honest limits |
| `.github/workflows/ci.yml` | CI guards: `aether-sandbox seccomp` prints `SECCOMP_PLAN_OK`, the shim probe reports both layers, `doctor` reports `seccomp` |
| `os-build/Makefile` | `make seccomp-plan` |

### Commands and results

```
$ cargo test -p aether-sandbox
35 lib + 3 tests/landlock_enforce.rs + 2 tests/seccomp_enforce.rs = 40 passed, 0 failed
  (kernel allow/deny tests skipped on macOS with a printed reason;
   refusal path: exit 126, program never runs)

$ cargo test -p aether-runner
41 lib + 4 integration = 45 passed, 0 failed

$ cargo run -q -p aether-sandbox --bin aether-sandbox -- seccomp | tail -8
  {"arch": "aarch64", "audit_arch": 3221225655, "denied": [...],
   "instructions": 77, "probe": {"available": false, "reason": "... macos"}}
  SECCOMP_PLAN_OK

$ cargo run -q -p aether-sandbox --bin aether-landlock -- --probe --json
{"landlock": {"state": "unavailable", ...}, "seccomp": {"available": false, ...}}
exit=0
$ cargo run -q -p aether-sandbox --bin aether-landlock -- --seccomp --ro /usr -- /bin/true
aether-landlock: refusing to start the app: Landlock enforcement is unavailable: ...
exit=126
```

### Combined run (required command)

```
$ cargo test -p aether-taskmanager -p aether-sandbox
taskmanager 48 + 1 + 1 + 1 = 51, sandbox 35 + 3 + 2 = 40 → 91 passed, 0 failed
```

## 3. Still pending (hardware / owner / other sessions)

- Kernel seccomp/Landlock allow/deny runs on the Dell and the shim installed at
  `/usr/libexec/aether-landlock` in the image (S1 packaging). The deny list must
  be re-validated against the shipped kernel before claiming AP-AC-05.
- Global `Ctrl+Shift+Esc` registration and TM-AC-01's 500 ms budget (shell /
  compositor); the hung flag for Force Quit comes from the P3-T08 watchdog.
- Services start/stop/restart (TM-17) need the permission prompt UI.
- G-TM1/G-TM2, TM-AC-05/06 and the 72-hour soak on the reference machines.
