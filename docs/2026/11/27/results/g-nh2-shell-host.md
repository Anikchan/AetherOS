# D6 — G-NH2 shell side: the interactive shell host

- Date: 2026-11-27 (IST)
- Session: S3 — `session - 3 (Nov27)`
- Task: G-NH2 follow-up recorded in
  `docs/2026/11/27/results/g-nh2-shell-nohang.md` §5 ("the interactive shell host
  must read `APP_STATE` / `SHELL_WATCHDOG` from the compositor journal")
- Territory: `shell/`, `docs/2026/11/16/os-project/features/`
- Status: **Done (host loop, journal injection, ledger handshake, tests, harness
  docs); display-backend mapping and the compositor's terminate enforcement are
  hardware-gated**

## 1. What was implemented

`shell/src/runtime.rs` — the loop that actually runs the shell:

| Piece | Behaviour |
| --- | --- |
| `Journal` trait | `poll()` returns only text appended since the previous call, whole lines only. |
| `FileJournal` | Follows the compositor log by byte offset; a half-written marker stays buffered; a truncated/rotated file restarts at offset 0; a missing file is "nothing new yet", not an error. |
| `MemoryJournal` | Shared `Rc<RefCell<String>>` buffer so a test/harness keeps a writing handle while the host reads. |
| `ShellHost::start` | Session ledger handshake: folds the previous run's watchdog markers, then `begin_start()` (`--start-begin`), publishing `AETHER_SHELL_START` (+ `AETHER_SHELL_SAFE_MODE`) before any surface exists. |
| `ShellHost::pump_once` | Reads the journal, applies every `AETHER_COMPOSITOR_APP_STATE` decision via `observe_health`, folds `AETHER_COMPOSITOR_SHELL_WATCHDOG` into the ledger (publishing `AETHER_SHELL_HOST_WATCHDOG`), ticks the dialog clock (1 s) so a **Wait** re-offers after the 15 s grace, and drains every controller marker into the shell journal. |
| `ShellHost::present` | Shows the Slint `ShellRoot`, pumps timers/animations, requests a redraw; the first frame calls `session_ready()` (`--start-confirm`) and publishes `AETHER_SHELL_HOST_READY`; `--frames-out` captures a PNG via the software renderer (a real display backend makes the capture "skipped", not fatal). |
| `ShellHost::run_for` | The loop. `frames == 0` runs until the service manager stops it; `publish` receives each marker batch as it is produced. |
| `ShellHost::take_terminations` | The clients whose `AETHER_SHELL_NOHANG_TERMINATE` was published, for an in-process supervisor. |
| `ShellHost::act` / `set_auto_action` | Press Wait/Force Quit through the same `dialog_action` the Slint callbacks call; the CLI's `--act` presses once when the first dialog is on screen (scripted harness press). |

Supporting changes:

- `nohang::parse_health_line` and `WatchdogReport::observe_line` — the parsers
  now work line-by-line so the host can follow a growing journal;
  `health_events` / `WatchdogReport::scan` are thin wrappers over them (behaviour
  unchanged, existing tests untouched).
- `ShellController::observe_watchdog` — folds a `WatchdogReport` into the ledger,
  arms safe mode, persists `session.json` and logs the safe-mode line.
- CLI: `aether-shell --interactive [--journal <log>] [--frames <n>]
  [--step-ms <ms>] [--frames-out <dir>] [--act wait|force-quit]`, default journal
  `/var/log/aether-compositor.log` (or `$AETHER_COMPOSITOR_LOG`).

## 2. Marker contract (host additions)

| Direction | Marker |
| --- | --- |
| Shell → journal | `AETHER_SHELL_HOST_JOURNAL: path=… step_ms=… frames=<until-stopped\|n>` |
| Shell → journal | `AETHER_SHELL_HOST_READY: frames=1 polls=1` (first presented frame) |
| Shell → journal | `AETHER_SHELL_HOST_WATCHDOG: timeouts=… restart_required=… armed_safe_mode=…` |
| Shell → journal | `AETHER_SHELL_HOST_CAPTURE_SKIPPED: <reason>` (evidence capture only) |
| Shell → journal | `AETHER_SHELL_HOST_DONE: frames=… polls=… dialogs=… actions=… terminations=… ready=…` |
| Shell → journal | `AETHER_SHELL_HOST_TERMINATIONS_PUBLISHED: count=…` |
| Shell → journal | everything the controller logs: `NOHANG_DIALOG`, `…_ACTION`, `…_RECOVERED`, `…_TERMINATE`, `…_SUMMARY`, `AETHER_SHELL_START*`, `SAFE_MODE*` |

Consumption: `AETHER_SHELL_NOHANG_TERMINATE` reaches the shell's own journal from
the host loop and `take_terminations()`; the compositor (xdg unresponsive error)
or the service manager (S4 `lifecycled`) consumes it. The shell still never
`kill()`s a client itself.

## 3. Verification

```
cargo test -p aether-shell -p aether-ui
```

Result (2026-11-27): **green**

- `aether-shell` lib: **117 passed** (111 before + 6 new `runtime` unit tests:
  whole-line file journal, rotation/truncation, memory journal, line-level health
  parsing, incremental watchdog report, run-summary format).
- `tests/host.rs`: 1 passed — one end-to-end function because the Slint testing
  backend installs a process-wide platform. Covers: start counted before
  surfaces → `APP_STATE not_responding` raises the dialog and publishes
  `NOHANG_DIALOG` → first `present` confirms (`START_CONFIRMED: attempts=0`,
  ledger `attempts=0/confirmed=true`) → **Wait** publishes `action=wait
  grace_ms=15000` and re-offers after 16 s → **Force Quit** publishes
  `action=force_quit` + `AETHER_SHELL_NOHANG_TERMINATE: client=7` and
  `take_terminations() == [7]` → a late pong closes the dialog (`…_RECOVERED`) →
  a scripted `--act` press force-quits client 11 through the same path → the
  bounded loop reports `HOST_DONE … dialogs=4 actions=3 terminations=2 ready=true`.
- `tests/host_ledger.rs`: 1 passed — three host launches without a frame reach
  `attempt=3 safe_mode=true`, publish `AETHER_SHELL_SAFE_MODE … effects=minimal`,
  write `attempts=3, confirmed=false, safe_mode=true` to
  `~/.config/aether/shell/session.json`, show the banner and the Minimal tier;
  the next `present` confirms `attempts=0` while the banner stays; the banner
  Reset publishes `SAFE_MODE_RESET` and restores the Full tier; a journal with
  `timeouts=3` arms safe mode at the *next* launch
  (`HOST_WATCHDOG … armed_safe_mode=true`).
- All earlier suites unchanged: `tests/nohang.rs` 7, `nohang_ui` 1, `effects` 6,
  `effects_ui` 1, `features` 1, `greeter` 1, `interaction` 1, `layer_host` 6;
  `aether-ui` 5.

Manual run of the exact chain the harness will use (development host, headless):

```
$ aether-shell --interactive --journal fixture.log --frames 2 --step-ms 50 --act force-quit
AETHER_SHELL_HOST_JOURNAL: path=…/fixture.log step_ms=50 frames=2
AETHER_SHELL_START: attempt=1 safe_mode=false previous=clean
AETHER_SHELL_NOHANG_DIALOG: client=7 app=client-7 after_ms=5100
AETHER_SHELL_NOHANG_ACTION: client=7 app=client-7 action=force_quit
AETHER_SHELL_NOHANG_TERMINATE: client=7
AETHER_SHELL_START_CONFIRMED: attempts=0
AETHER_SHELL_HOST_READY: frames=1 polls=1
AETHER_SHELL_HOST_DONE: frames=2 polls=2 dialogs=1 actions=1 terminations=1 ready=true
AETHER_SHELL_HOST_TERMINATIONS_PUBLISHED: count=1
exit=0
$ cat ~/.config/aether/shell/session.json
{"attempts":0,"confirmed":true,"safe_mode":false,"watchdog_timeouts":0}
```

`cargo clippy -p aether-shell --all-targets`: no new warnings from
`runtime.rs` or the two new test files (pre-existing warnings in `app.rs`,
`model.rs`, `effects.rs` untouched).

## 4. G-NH2 mapping (addendum 1 F11)

| Criterion | Status |
| --- | --- |
| "Not responding" shown within 6 s | The host reacts to the compositor's marker on the next 250 ms poll, so the dialog is bounded by the marker's `after_ms` (≤ 6000, checked by `--nohang-check`) plus one poll. Verified headlessly in `tests/host.rs`; wall-clock on device is §1c-0/1c-1 of the acceptance harness. |
| Hung app does not freeze the desktop | The host does no blocking I/O in the frame path; journal reads are incremental and bounded by the file's new bytes. |
| Wait / Force Quit | Wait re-offers after 15 s from the host clock (`tick` in `pump_once`); Force Quit publishes the terminate intent and hands the client to the supervisor. Both proven in `tests/host.rs`. |
| Shell restart within 5 s, no lost windows | Detection is the compositor's; the ledger survives the restart (`session.json`) and the host counts/confirms each launch. The restart itself is S4's service manager. |
| Safe mode after 3 failed starts | The host owns the handshake: 3 launches that never present a frame, or 3 watchdog timeouts, run the reduced session (Minimal tier + banner + Reset). `tests/host_ledger.rs`. |

## 5. Hardware-gated / not claimed

- The layer-shell `Platform` that maps the presented surfaces onto a real output
  (P3-T02 client side) is still the image-side piece; on the development host the
  loop runs on the software renderer and `--frames-out` captures evidence frames.
- The compositor's enforcement of `AETHER_SHELL_NOHANG_TERMINATE` (xdg
  unresponsive error) and the service manager's restart of `aether-shell`
  (S4 `lifecycled`) are not exercised here; the shell publishes the intent and
  keeps the ledger that the supervisor reads.
- Client id → app-name registration comes from the surface host as toplevels map
  (`ShellController::register_client`); a journal-only run labels unknown clients
  `client-<n>`.
- No on-device run is claimed: hardware validation remains paused per
  `docs/2026/11/26/session-kickoff.md`.
