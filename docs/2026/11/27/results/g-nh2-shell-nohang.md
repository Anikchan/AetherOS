# D6 — G-NH2 shell side: "Not responding" dialog, shell watchdog, safe mode

- Date: 2026-11-27 (IST)
- Session: S3 — `session - 3 (Nov26)`
- Task: G-NH2 shell half of P3-T08 (addendum 1, F11 section 3)
- Territory: `shell/`, `docs/2026/11/16/os-project/features/`
- Status: **Done (dialog state machine, restart ledger, UI surfaces, marker
  contract, tests); on-device run pending the interactive shell host**

## 1. What was implemented

`shell/src/nohang.rs` (pure state machines, no display server):

| Piece | Behaviour |
| --- | --- |
| `NoHangDialog` | One dialog at a time over the compositor's `APP_STATE` markers: `not_responding` raises it for a client id, a `responsive recovered=true` closes it, other hung apps queue by client id. |
| Wait | Closes the dialog, waives the app for `DEFAULT_WAIT_GRACE` (15 s) and re-offers it while it is still hung (`tick`), so the dialog cannot trap the user. |
| Force Quit | Closes the dialog, removes the app and returns the client id to the host, which emits `AETHER_SHELL_NOHANG_TERMINATE` (the compositor's xdg unresponsive error / service manager ends it). |
| `NoHangDialog::register_name` | Maps compositor client ids to `.desktop` id + display name; unknown ids render as `Application <n>`. |
| `WatchdogReport` | Parses `AETHER_COMPOSITOR_SHELL_WATCHDOG` (`timeouts`, `last_state`, `recovered`, `idle_ms`, `timeout_ms`); `needs_restart()` when the last state is `timeout`, `escalates()` at three. |
| `StartLedger` | `<settings root>/shell/session.json`: consecutive starts that never reached "session ready". Three in a row (`begin_start` without `confirm_start`) set `safe_mode`; three watchdog timeouts arm it too. `reset()` is the banner's Reset. |
| `NoHangReport` | `--nohang-check`: hang transitions with `within_limit`, watchdog summary, ledger state and problems (detection later than 6 s, stalled shell). |
| `resolve_tier` | Safe mode resolves the glass tier to Minimal (the reduced session). |

UI (`shell/ui/surfaces.slint`, `shell/src/app.rs`):

- `NotRespondingDialog` (title `<App> is not responding`, **Wait** / **Force
  Quit**) and `SafeModeBanner` (amber, **Reset**), both above every overlay,
  including the lock screen.
- `ShellController::observe_health` / `tick_dialog` / `dialog_action` /
  `register_client` are the seam the interactive host drives from the journal;
  every action logs its `AETHER_SHELL_NOHANG_*` marker.
- `begin_session_start` / `session_ready` / `reset_safe_mode` own the ledger and
  the banner; `effect_tier()` / `effect_env_line()` now resolve through safe mode.

CLI (`shell/src/main.rs`): `--nohang-check`, `--start-begin [journal]`,
`--start-confirm`, `--safe-mode-reset`, plus `safe-mode:`, `start-attempts:` and
`nohang:` in `--status`.

## 2. Marker contract

| Direction | Marker |
| --- | --- |
| Compositor → shell | `AETHER_COMPOSITOR_APP_STATE: client=… state=not_responding after_ms=…` |
| Compositor → shell | `AETHER_COMPOSITOR_APP_STATE: client=… state=responsive recovered=true` |
| Compositor → shell | `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout\|ok idle_ms=… timeout_ms=… timeouts=…` |
| Shell → journal | `AETHER_SHELL_NOHANG_DIALOG: client=… app=… after_ms=…` |
| Shell → journal | `AETHER_SHELL_NOHANG_ACTION: client=… app=… action=wait grace_ms=15000` |
| Shell → journal | `AETHER_SHELL_NOHANG_ACTION: client=… app=… action=force_quit` |
| Shell → journal | `AETHER_SHELL_NOHANG_RECOVERED: client=… app=…` |
| Shell → host | `AETHER_SHELL_NOHANG_TERMINATE: client=…` |
| Shell → journal | `AETHER_SHELL_NOHANG_SUMMARY: prompts=… waits=… force_quits=… recoveries=… open=…` |
| Shell → journal | `AETHER_SHELL_START: attempt=… safe_mode=… previous=clean\|unconfirmed` |
| Shell → journal | `AETHER_SHELL_START_CONFIRMED: attempts=0` |
| Shell → journal | `AETHER_SHELL_SAFE_MODE: attempts=… timeouts=… reason=… effects=minimal` |
| Shell → journal | `AETHER_SHELL_SAFE_MODE_RESET: attempts=0` |

## 3. Verification

```
cargo test -p aether-shell -p aether-ui
```

Result (2026-11-27): **green** — shell 111 lib tests (12 new `nohang` unit
tests), `tests/nohang.rs` 7/7, `tests/nohang_ui.rs` 1/1, all earlier shell
suites unchanged (effects 6, effects_ui 1, features 1, greeter 1, interaction 1,
layer_host 6), `aether-ui` 5/5.

Covered by the tests:

1. `not_responding` raises the dialog once, a repeated marker does not stack
   prompts, and `recovered=true` closes it (`a_hang_raises_the_dialog_and_a_late_pong_closes_it`).
2. Wait waives for 15 s, does not re-offer inside the grace and re-raises with
   the original `after_ms` after it (`wait_waives_the_dialog_and_reoffers_after_the_grace`).
3. Force Quit returns the client, removes it and shows the next queued hang
   (`force_quit_returns_the_client_and_shows_the_next_hang`).
4. Ping/pending markers are ignored; only decisions are parsed.
5. Watchdog stall/recovery parsing, and the restart/escalation predicates.
6. Three unconfirmed starts enter safe mode, the ledger survives a process
   restart, `--start-confirm` clears the count but not the banner, reset leaves
   it.
7. Three watchdog timeouts arm safe mode from the journal (`--start-begin <log>`).
8. `--nohang-check` accepts a 5.1 s detection and rejects a late (8 s) one and a
   shell whose last watchdog state is `timeout`; `-` reads stdin.
9. `--status` reports `safe-mode: true`, `start-attempts: 3`,
   `nohang: idle` and `effects-effective: minimal` in safe mode.
10. The widget test clicks **Wait**, **Force Quit** and **Reset** through the
    real Slint tree and asserts the markers, the tier change and the re-offer.

## 4. G-NH2 mapping (addendum 1 F11)

| Criterion | Status |
| --- | --- |
| "Not responding" shown within 6 s | Marker `after_ms` is bounded at 6000 ms by `--nohang-check`; the dialog raises on the marker (headless tests). On-device timing is §1c-1 of the acceptance harness. |
| Hung app does not freeze the desktop | The compositor's own test proves frames continue; the shell only reacts to markers and never blocks the render path (no I/O in `sync`). |
| Wait / Force Quit | Implemented and tested (`wait_waives_…`, `force_quit_…`, UI test). Force Quit emits the terminate intent for the host. |
| Shell restart within 5 s without losing windows | Watchdog detection is the compositor's (P3-T08); the shell's restart policy is the ledger + safe mode (`three_unconfirmed_starts_…`). The restart itself is the service manager's job (S4). |
| Safe mode after 3 failed starts | Implemented: three unconfirmed starts or three watchdog timeouts → Minimal tier + banner + Reset. |

## 5. Follow-ups

- The interactive shell host must read `APP_STATE` / `SHELL_WATCHDOG` from the
  compositor journal (or a future socket) and call `observe_health` / `tick_dialog`
  on a short timer; today the tests and the CLI drive the same functions.
- Force Quit ends the client either through the compositor's xdg unresponsive
  error or the service manager consuming `AETHER_SHELL_NOHANG_TERMINATE`; the
  shell deliberately does not `kill()` a client itself.
- `lifecycled` (S4) owns the restart rule for `aether-shell` and the journal
  hand-off; the ledger file is the shared state.
