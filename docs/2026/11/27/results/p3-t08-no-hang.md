# D6 — P3-T08 no-hang design (async updates, ping, watchdog, frame gaps)

- Date: 2026-11-27 (IST)
- Session: S2 — `session - 2 (Nov26)`
- Task: P3-T08 (addendum 1 F11 section 3; G-NH2)
- Territory: `compositor/`
- Status: **Done (state machine + markers + headless acceptance); shell UI and
  service-manager restart are integration items**

## 1. What was implemented

`compositor/src/hang.rs` (pure state machines, unit-tested):

| Rule | Implementation |
| --- | --- |
| NH-01 asynchronous updates | `health_tick` only *queues* pings and timeouts on the event-loop tick; nothing in the commit/render path waits for a client. |
| NH-02 Wayland ping / "Not responding" | `HangDetector`: `Responsive → PingPending → NotResponding` after `AETHER_COMPOSITOR_PING_TIMEOUT_MS` (default **5000 ms**, spec limit 6 s). The pending ping stays in flight (smithay refuses a second one), and a late pong moves the client back to `Responsive` (`recovered=true`). |
| NH-03 shell watchdog | `ShellWatchdog`: armed by the first layer-shell surface, fed by every layer-surface commit; a stall ≥ `AETHER_COMPOSITOR_SHELL_WATCHDOG_MS` (default 5000 ms) logs `state=timeout`; the next commit logs `state=ok recovered=true`. The compositor keeps all windows regardless (the service manager restarts only the shell). |
| NH-06 no blocking calls on the UI thread | no I/O in the ping/commit path (log only). |
| NH-07 frame-gap logger | `FrameGapLogger` consumes the new `Frame.damage_wait_ms` (time a damaged frame waited from commit to presentation, measured in `FrameScheduler`). Gaps ≥ `AETHER_COMPOSITOR_FRAME_GAP_MS` (default **1000 ms**) are logged. Idle time is never a gap because only pending damage is measured. |

`compositor/src/lib.rs`:

- `XdgShellHandler::new_client` registers each shell client; `client_pong`
  clears the pending ping. Pings are queued every
  `AETHER_COMPOSITOR_PING_INTERVAL_MS` (default 2000 ms).
- `health_tick` runs from the calloop loop (10 ms tick).
- New markers:
  - `AETHER_COMPOSITOR_NO_HANG_CONFIG: ping_interval_ms=… ping_timeout_ms=… frame_gap_ms=… shell_watchdog_ms=…`
  - `AETHER_COMPOSITOR_PING: client=… serial=…`
  - `AETHER_COMPOSITOR_PONG: client=… serial=… rtt_ms=… recovered=…`
  - `AETHER_COMPOSITOR_APP_STATE: client=… state=not_responding after_ms=…`
  - `AETHER_COMPOSITOR_APP_STATE: client=… state=responsive recovered=true`
  - `AETHER_COMPOSITOR_FRAME_GAP: ms=… threshold_ms=… gaps=… max_ms=…`
  - `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout|ok …`
  - `AETHER_COMPOSITOR_NO_HANG_SUMMARY: clients=… responsive=… not_responding=…`

`compositor/src/scheduling.rs`: `Frame.damage_wait_ms` + `FrameScheduler`
first-damage timestamp (unit-tested).

## 2. Verification

```
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor
```

Result (2026-11-27): **47 unit tests + 5 integration tests green**, including
`hang::tests` (7 tests: ping/pong RTT, timeout transition, late-pong recovery,
pong without pending, ping interval, counts, frame gaps, watchdog) and
`tests/p3_t08_no_hang.rs` (`no_hang_state_machine`), which boots the real binary
three times with 100 ms ping / 400 ms timeout / 300 ms watchdog:

1. responsive app → `PING` + `PONG … recovered=false` and `FRAME_GAP`;
2. app stops dispatching → `APP_STATE … state=not_responding after_ms=…`, then
   resumes → `state=responsive recovered=true` without restarting the
   compositor;
3. shell layer surface stops committing → `SHELL_WATCHDOG state=timeout`, next
   commit → `state=ok recovered=true`.

## 3. G-NH2 mapping (addendum 1 F11)

| Criterion | Status |
| --- | --- |
| Test app that stops responding is marked "Not responding" within 6 s | **Verified headlessly** with a 400 ms test timeout; default is 5 s (< 6 s), state machine unit-tested at the 5 s boundary. |
| Hung app does not freeze the desktop | **Verified by design + test**: the client stops dispatching for 1.2 s while the compositor continues frames/pings; nothing blocks. |
| Force Quit / Wait actions | Detection marker emitted; the compositor-side force-quit primitive exists in smithay (`ShellClient::unresponsive`, xdg `unresponsive` error). The shell's UI/action and greetd/service-manager shell restart are S3/integration work. |
| Shell restart within 5 s without losing windows | Shell watchdog detection is implemented; the restart itself is the service manager's job (`lifecycled`, S4). |
| 24-hour frame-gap measurement | Hardware/long-run item; logger and threshold are in place. |

## 4. Notes / follow-ups

- The frame-gap logger currently logs while the frame is presented; cause
  classification (CPU/memory/IO/unknown, NH-07) needs PSI readings from
  `powerd`/`lifecycled` and will consume these markers.
- "Not responding" windows are not killed automatically: the spec requires user
  choice (Wait / Force Quit), so the marker is the contract with the shell.
