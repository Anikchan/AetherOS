# P5-T07 remaining tabs + P6-T04 Landlock enforcement — S4 (2026-11-27)

Session S4 (`session - 4 (Nov26)`), hardware-independent round. Scope:

1. Activity Manager remaining tabs — Performance (CPU/memory/graphs from `/proc`
   sampling), Battery (powerd status + F14 history), Sensors (temperature, fan,
   GPU) — plus the `Ctrl+Shift+Esc` shortcut contract and the end-task /
   force-quit dialog, wired to the existing model/contracts.
2. P6-T04 follow-up — Landlock enforcement, fail-closed, with tests.
3. Tests for the new tabs.

No hardware gate is claimed. The Dell hardware validation is still paused.

## 1. Activity Manager tabs, shortcut and dialog

### What changed

| File | Change |
|---|---|
| `apps/taskmanager/src/tabs.rs` (new) | View models: `performance_view` (five normalised 60 s graphs, per-core bars, CPU/memory/disk/network/GPU detail lines), `battery_view` (live status + F14 history: 24 h energy by app with P5-T15 levels, health trend), `sensors_view` (sorted temperatures/fans, 85 °C / 95 °C tones, alerts, explicit fallbacks), plus text reports for CLI/D6 evidence |
| `apps/taskmanager/src/shortcut.rs` (new) | `ShortcutBinding` (`Ctrl+Shift+Esc` Windows profile, `Super+Escape` default, 500 ms budget per TM-AC-01) and a toolkit-independent `KeyChord` parser/matcher |
| `apps/taskmanager/src/manager.rs` | `request_end_task` → `EndTaskRequest`/`ConfirmationDialog` (unsaved-work wording, Force Quit wording for hung apps, PID 1/self refusal) |
| `apps/taskmanager/src/model.rs` | `Tab::key()` / `Tab::from_key()` so the CLI and the window share one tab identifier |
| `apps/taskmanager/src/window.rs` | `bind_tabs`, `bind_shortcut`, `bind_performance`, `bind_battery`, `bind_sensors`, `show_dialog`/`on_end_task`, `on_shortcut`; demo system/history/sensors for the evidence render |
| `apps/taskmanager/ui/activity-manager.slint` | Tab bar (`SegmentedControl`), Performance graph cards + detail columns, Battery metrics + energy table, Sensors sections + alerts, modal end-task dialog, `FocusScope` key handler for `Ctrl+Shift+Esc` / `Super+Escape` |
| `apps/taskmanager/src/main.rs` | `--shortcut [--windows-profile]`, `--tab processes\|performance\|battery\|sensors`, `--render` now draws every tab |
| `apps/taskmanager/tests/tabs.rs` (new) | Headless test: tab bar, shortcut label, graph/line models, battery/sensor models, dialog → signal mapping, shortcut callback |

Everything the window shows comes from the existing contracts: the Energy
column header/null label/tooltip/sort and the battery energy levels are
`EnergyColumnContract` values; graph values and fallbacks come from the tab
view models; the shortcut string comes from `ShortcutBinding`.

### Commands and results

```
$ cargo test -p aether-taskmanager
42 lib tests + 1 tests/tabs.rs + 1 tests/ui.rs → 44 passed, 0 failed

$ cargo run -q -p aether-taskmanager -- --shortcut --windows-profile
{"action":"open-activity-manager","active_accel":"Ctrl+Shift+Esc","budget_ms":500,
 "default_accel":"Super+Escape","profile":"windows","windows_profile_accel":"Ctrl+Shift+Esc"}

$ cargo run -q -p aether-taskmanager -- --tab performance
Performance (60 s window)
  CPU  0.0%  Peak 0.0%  [0 samples] ...            # macOS host: explicit fallbacks
$ cargo run -q -p aether-taskmanager -- --tab battery
Battery  Charge Not reported ... "No battery data: pass --battery-status ..."
$ cargo run -q -p aether-taskmanager -- --tab sensors
Sensors  GPU Engine Not reported / Temperature Not reported by this system ...

$ cargo run -q -p aether-taskmanager -- --render docs/2026/11/27/results/screenshots
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-processes.png (1080x640) [Processes]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-performance.png (1080x640) [Performance]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-battery.png (1080x640) [Battery]
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-sensors.png (1080x640) [Sensors]
```

### Screenshots (demo data; documented as such)

- `screenshots/activity-manager-processes.png` — Processes tab, contract-driven
  Energy column (unchanged behaviour).
- `screenshots/activity-manager-performance.png` — five 60 s graphs from
  `PerformanceHistory` plus CPU/memory/disk/network/GPU detail lines.
- `screenshots/activity-manager-battery.png` — 79 % / 9.8 W / 1 h 03 m / 88.5 %
  health, plus the 24 h energy-by-app table showing contract `High`/`Low` levels.
- `screenshots/activity-manager-sensors.png` — temperatures (success tones),
  fan RPM, GPU engine/VRAM.

The dev host has no `/proc`, `/sys` sensors or `powerd` history, so `--render`
falls back to `demo_system`/`demo_history`/`demo_sensors` and a three-app demo
history. The labels, tones, thresholds and sort order in the screenshots are
the real contract values.

### Acceptance status

| Criterion | Result |
|---|---|
| TM-AC-01 Ctrl+Shift+Esc opens within 500 ms | Binding + matcher + focused-window handler done; global registration and the 500 ms measurement need the compositor/shell and the Dell |
| TM-AC-05 temperatures within 2 °C | Pending hardware (needs the Dell sensors) |
| TM-AC-06 GPU use shows activity during a GPU test | Pending hardware |
| TM-AC-07 overhead ≤ 1 % of one core (G-TM1) | Pending hardware/soak |
| TM-AC-02/03/08/09/10 | Covered by core + window tests; TM-AC-03 device check pending |

## 2. P6-T04 follow-up — Landlock enforcement (fail-closed)

### What changed

| File | Change |
|---|---|
| `apps/sandbox/src/landlock.rs` (new) | `LandlockAccess` (16 rights with ABI gates), `LandlockPolicy::from_profile` (read-only system dirs, writable app folder, devices), ABI masking (`refer` ≥ 2, `truncate` ≥ 3, `ioctl-dev` ≥ 5), `probe()` (kernel ABI), `plan()` (fail-closed gate), `apply()`/`apply_plan()` (raw syscalls 444/445/446, `PR_SET_NO_NEW_PRIVS`, `restrict_self`), shim argument parser |
| `apps/sandbox/src/bin/aether-landlock.rs` (new) | Shim: `--probe [--json]` and `[--ro PATH]… [--rw PATH]… -- PROGRAM [ARGS]…`; applies the ruleset then `execvp`s; exits **126 without running the program** when enforcement is impossible |
| `apps/sandbox/src/enforce.rs` | New `SandboxError::{LandlockUnavailable, LandlockRule, LandlockFailed}`, `Enforcer::command_with_landlock` (shim wrapped inside bwrap, refuses when Landlock or a rule is unavailable), `detect()` probes the Landlock syscall, `describe()` reports the ABI |
| `apps/sandbox/src/bin/aether-sandbox.rs` | `doctor` reports `landlock_status`, `landlock_shim`, `strict_launch`; new `landlock --manifest … --data …` prints the plan (`LANDLOCK_PLAN_OK`) or the refusal (`LANDLOCK_PLAN_REFUSED`) |
| `apps/sandbox/tests/landlock_enforce.rs` (new) | End-to-end shim tests: probe verdict; no-policy/malformed invocation never runs the program (exit 126 / 2); on Linux with Landlock, a granted file is readable and `/proc/version` outside the ruleset is denied with no content leak |
| `apps/runner/src/lib.rs`, `src/bin/aether-runner.rs` | `Runner::require_landlock(true)` / `--strict-landlock` uses the shim path (fail-closed) |
| `os-build/Makefile` | `make landlock-probe` target |
| `.github/workflows/ci.yml` | CI guard: `--shortcut` contains `open-activity-manager`, `--tab performance` prints the report, `doctor` reports `landlock_status`, the shim prints `LANDLOCK_ABI`/`LANDLOCK_UNAVAILABLE` |

### Commands and results

```
$ cargo test -p aether-sandbox
27 lib tests + 3 tests/landlock_enforce.rs → 30 passed, 0 failed
  (kernel allow/deny test skipped on macOS with a printed reason;
   refusal path: shim exits 126, program never runs)

$ cargo test -p aether-runner
41 lib + 4 integration → 45 passed, 0 failed

$ cargo run -q -p aether-sandbox --bin aether-landlock -- --probe
LANDLOCK_UNAVAILABLE Landlock is a Linux kernel feature and this host is macos   # exit 126

$ cargo run -q -p aether-sandbox --bin aether-sandbox -- doctor
{"bwrap": null, "landlock": false,
 "landlock_status": {"reason": "Landlock is a Linux kernel feature and this host is macos","state":"unavailable"},
 "landlock_shim": "/usr/libexec/aether-landlock", "strict_launch": false, "usable": false, ...}
ENFORCER_UNAVAILABLE sandbox cannot be applied: bubblewrap (bwrap) is not installed; ...
```

### Combined run

```
$ cargo test -p aether-taskmanager -p aether-sandbox
taskmanager 42 + 1 + 1, sandbox 27 + 3 → 74 passed, 0 failed
```

## 3. Still pending (hardware / owner / other sessions)

- Global `Ctrl+Shift+Esc` registration and the 500 ms budget: the compositor/
  shell must consume the `--shortcut` binding (the window only handles it while
  focused). The hung flag for the Force Quit wording comes from the compositor's
  no-hang watchdog (P3-T08).
- A real Landlock allow/deny run on the Dell (Linux) and the shim installed at
  `/usr/libexec/aether-landlock` in the image (image packaging is S1's
  `os-build/buildroot/`).
- G-TM1/G-TM2, TM-AC-05/06 and the 72-hour soak on the reference machines.
- The remaining tabs (App history, Startup apps, Users, Details, Services,
  Android) still show a placeholder in the window; their data is available from
  the CLI.
- `Cargo.lock` gained the `libc` edge for `aether-sandbox`; the lock file is
  shared with the other sessions and is left to S1's merge pass.
