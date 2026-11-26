# F1 — Desktop Shell

Owner component: `aether-shell` (menu bar, dock, launcher, control center, notifications, lock screen, file manager, overview) on top of `aether-compositor`.

Priority key: MVP (needed for the first daily-driver build), v1 (needed for release), Later (after release).

## 1. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| SHL-01 | Menu bar | MVP | Top bar with the active app name, its menus, system tray, clock, and a status menu. Height 28 px (see D1). |
| SHL-02 | Dock | MVP | Bottom or side dock with pinned and running apps, running indicators, drag to reorder, badge counts, auto-hide. |
| SHL-03 | Launcher and search | MVP | Full-screen or panel launcher. Search apps, files, settings, and simple calculations. Keyboard-first. |
| SHL-04 | Control center | MVP | Quick panel for Wi-Fi, Bluetooth, sound, brightness, Focus, and display settings. |
| SHL-05 | Notification center | MVP | Grouped notifications by app. Focus modes that mute notifications by schedule or by app. |
| SHL-06 | Window management | MVP | Window controls (close, minimize, maximize), drag to snap to halves and quarters, double-click title bar to maximize. |
| SHL-07 | Tiling mode | v1 | Per-workspace tiling layout with keyboard shortcuts. Windows can float or tile per app. |
| SHL-08 | Spaces and overview | v1 | Multiple workspaces. Overview shows all windows and workspaces. Three-finger swipe to switch. |
| SHL-09 | Glass material system | MVP (prototype), v1 (full) | Translucent panels with blur, tint, and edge highlight. Effect tiers in F6 and D1. |
| SHL-10 | Lock screen and login | MVP | Password or PIN unlock. Fast user switching later (v1). |
| SHL-11 | Settings app | MVP | Categories: display, sound, network, power, keyboard, mouse and touchpad, apps, privacy, appearance, about. Each change writes to config (F4). |
| SHL-12 | File manager | MVP | List, column, and icon views. Basic copy, move, rename, trash. Tags and quick preview later. |
| SHL-13 | Quick preview | Later | Press Space on a file to preview it. |
| SHL-14 | Gestures | v1 | Touchpad: three-finger swipe (spaces), four-finger pinch (overview), two-finger scroll. |
| SHL-15 | Multi-monitor and HiDPI | MVP | Different scales per monitor. Arrangement and mirroring. |
| SHL-16 | Appearance | v1 | Light, dark, and automatic (time-based) themes. Accent color. Wallpaper. |
| SHL-17 | Screenshot and recording | MVP | Area, window, and full-screen capture. Screen recording to a file. |
| SHL-18 | Accessibility | MVP (base), v1 (full) | Screen reader via AT-SPI, keyboard navigation for all shell features, high contrast, reduce transparency, reduce motion, text scaling. |

## 2. Window Controls and Behavior

- Window buttons are an original design (see D1). They are placed at the left of the title bar by default. Position is configurable (F4).
- Double-click on a title bar toggles maximize.
- Dragging a window to a screen edge shows a snap preview. Release snaps to the half or quarter.
- Windows of the same app group in the dock and in the overview.

## 3. Glass Material Behavior

- Glass applies to panels, menus, the dock, the control center, and notifications. It does not apply to document content.
- The compositor provides the blurred background. The shell provides the tint and highlight.
- Effect tiers:
  - Full: blur, saturation, tint, edge highlight, subtle edge refraction.
  - Reduced: blur at half resolution, no refraction.
  - Minimal: solid tinted panels with a border (used on low-end GPUs and when "reduce transparency" is on).
- Tier is chosen at startup from a GPU capability check and can be changed in settings (F4).

## 4. Dependencies

- P3 compositor (windows, layer shell, effects pipeline).
- F2 process manager (app state for the dock and overview).
- F4 configuration (all settings).

## 5. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| SHL-AC-01 | Menu bar shows the active app menu and updates within 100 ms of focus change | Implemented, headless-verified (2026-11-23) — menus are property-driven; the 100 ms figure needs compositor focus events (P3-T02) |
| SHL-AC-02 | Dock shows running indicators; a closed app's indicator disappears within 1 s | Implemented, headless-verified — indicator is bound to `App.running`; live timing needs the session |
| SHL-AC-03 | Launcher opens by keyboard shortcut within 200 ms and finds apps by partial name | Implemented, headless-verified — Super+Space opens it, ranking is a pure function (no I/O) and partial names rank above substrings |
| SHL-AC-04 | Control center changes Wi-Fi, Bluetooth, volume, and brightness | Implemented, headless-verified with mock services — nmcli/bluetoothctl/wpctl/brightnessctl adapters exist; 2026-11-27 adds the Full/Reduced/Minimal effects-tier switch (persisted, published to the compositor, verified by `aether-shell --effects-check`); on-device confirmation pending |
| SHL-AC-05 | Focus mode hides notifications during the scheduled time and shows them after | Implemented, headless-verified — the daily window comes from `notifications.toml` (`focus-start`/`focus-end`, overnight supported), manual Focus runs two hours, the minute tick expires it and refreshes cards and badge; the settings pane edits the keys |
| SHL-AC-06 | Snap to halves and quarters works with a mouse drag and a keyboard shortcut | Not tested — compositor work (P3-T04) |
| SHL-AC-07 | Glass tiers switch without restart; Minimal tier used when reduce transparency is on | Implemented, headless-verified (2026-11-27) — the control centre's Full/Reduced/Minimal switch writes `theme.effects-tier` through aether-config, every panel re-renders at the resolved tier, and `theme.reduce-transparency` forces Minimal (D1 §9). The running compositor reads `AETHER_COMPOSITOR_EFFECT_TIER` at startup, so a tier change applies to the next compositor start; `aether-shell --effects-check` reads the compositor's own markers and reports the mismatch instead of claiming a live switch. Live full→reduced switch on the Dell is the remaining on-device step |
| SHL-AC-08 | Lock screen rejects a wrong password and accepts the right one | Implemented, headless-verified (2026-11-26) — the screen is modal (scrim, Escape and shortcuts cannot dismiss it), five wrong attempts rate-limit the surface, and authentication goes through an `Authenticator` backend: greetd + PAM for login, `libpam` (`aether-lock` service) for the in-session lock, with the salted-digest prototype as the development fallback. PAM FFI is type-checked (`cargo check --features pam`); on-device PAM confirmation pending the image build |
| SHL-AC-09 | Every settings category opens and every change is written to the config file | Implemented, headless-verified — categories switch the page (clicked in the test), rows are generated from the F4 schema, each shows file+key, and writes are atomic and read back from disk |
| SHL-AC-10 | Two monitors with different scales work without restart | Partially verified (headless, 2026-11-26) — the compositor's `p3_t02_headless` test asserts two outputs at 1× and 1.5× including the logical layout, and the shell verifies the configure size per output with `aether-shell --host-check` (menubar 1920×28 on a 1.5×/1920-logical panel). Live re-scale on a real second monitor pending the on-device run |
| SHL-AC-11 | Screen reader announces menu bar items and launcher results | Partially verified — every interactive element sets an accessible role/label (asserted by the headless tests); AT-SPI session pending |
| SHL-AC-12 | Reduce motion, reduce transparency and text scale apply to every shell surface; every shell surface is reachable by keyboard and the focus indicator is always visible | Implemented, headless-verified (2026-11-27) — `theme.reduce-motion` collapses slides to a 120 ms fade, `theme.reduce-transparency` forces Minimal (SHL-AC-07) and `theme.text-scale` (100–200 %) scales the font tokens, menu bar and row heights; Tab/Shift+Tab reach the menu bar, dock, launcher, control centre and notifications with a 2 px accent ring. On-device visual confirmation and AT-SPI announcement pending |

Evidence: `docs/2026/11/23/results/p4-shell-and-apps.md` and
`docs/2026/11/23/results/screenshots/`.

Status values: Not tested, Pass (date and evidence), Fail (ticket ID).

## 6. Open Questions

- Dock position default: bottom (macOS-like) or side. Decide with the owner (Q1 relates to the visual reference). **Status: the implementation ships a bottom dock; the config key `shell/dock.toml: position` is accepted by the schema but only `bottom` is rendered so far.**
- Whether to include a start-menu-style layout as an option (Windows-friendly users). Proposed: yes, as a launcher layout option in v1. **Status: the launcher is the macOS-like centred panel; the start-menu layout remains a v1 option.**

## 7. Implementation Notes (2026-11-23)

- The shell is `shell/` (Rust + Slint); the visual language, widgets and icons
  live in the shared `ui/` crate so the default apps match.
- Every surface is rendered headlessly in CI-able form; screenshots are the P4
  evidence.
- Keyboard shortcuts: Super+Space (launcher), Super+A (overview), Super+N
  (notifications), Super+C (control centre), Super+L (lock), Super+F (Focus),
  arrows + Enter in the launcher, Esc closes any overlay. Names match the
  `input/keybindings.toml` keys.
- Keyboard navigation (SHL-18): Tab/Shift+Tab move between the menu bar, dock,
  launcher, control centre and notifications; arrows wrap inside a surface;
  Enter/Space activate; the focused item shows a 2 px accent ring (see §13).
- The on-device gate for this document's acceptance table is the layer-shell
  host from P3-T02 plus greetd/PAM for the lock screen.

## 8. Login and Lock Screen (P4-T06, 2026-11-26)

Two surfaces share one protocol implementation:

- **Login (before a session exists).** `aether-greeter` runs as the unprivileged
  `greeter` user under greetd. It speaks the documented greetd IPC protocol
  (`shell/src/greetd.rs`: native-endian `u32` length prefix + JSON, requests
  `create_session` / `post_auth_message_response` / `start_session` /
  `cancel_session`, replies `auth_message` / `success` / `error`). greetd runs as
  root and executes PAM itself, so **the greeter holds no credentials logic**.
  The login state machine (`shell/src/greeter.rs`) answers `info`/`error`
  messages with no response, returns the first `visible`/`secret` prompt to the
  UI, counts `auth_error` failures and cancels the session after
  `max_attempts`. `GreeterController` (`shell/src/greeter_app.rs`) drives the
  `GreeterRoot` surface: one "Sign in" press performs create-session, the typed
  password response and start-session.
- **Lock (inside a running session).** There is no greetd to ask, so the lock
  screen uses the `Authenticator` trait (`shell/src/auth.rs`). On Linux images
  the shell is built with `--features pam` and authenticates against the
  `/etc/pam.d/aether-lock` service through a small `libpam` binding
  (`pam_start` / `pam_authenticate` / `pam_acct_mgmt` / `pam_end`, plus one
  conversation callback). Only `PAM_SUCCESS` unlocks; `auth_error`, `maxtries`
  and account states show a specific message; service errors show "unavailable"
  and do not count as a wrong password. The FFI compiles only with the feature,
  so the macOS development host and default CI builds use the salted-digest
  prototype (the demo account `aether`/`aether`).

Deployment files live in `shell/greetd/`:

| File | Installed as | Purpose |
| --- | --- | --- |
| `greetd-config.toml` | `/etc/greetd/config.toml` | terminal VT and `default_session` = `aether-greeter` |
| `greeter.toml` | `/etc/aether/greetd/greeter.toml` | pre-filled user, attempt budget, session command and environment |
| `aether-lock.pam` | `/etc/pam.d/aether-lock` | auth/account only, no session or password change |

Behaviour and evidence:

- `aether-greeter --status` reports the socket, session command and whether the
  build links PAM; `--render <dir>` writes `greeter-login.png` and
  `greeter-denied.png`; `--stdin` logs in on a terminal (usable in the QEMU
  image before the layer-shell host lands).
- Tests: greetd framing and every response variant; request JSON exactly as
  documented; a real UNIX-socket round-trip; login flow (prompt, info skipping,
  denial with attempts left, lockout, missing greetd, refused start); greeter
  controller (single-press login, retry, visible prompt, lockout, backend
  unavailable); PAM status mapping and conversation; lock-screen rate limit and
  backend selection.
- Remaining on-device step: build the image with `--features pam`, install the
  three files and confirm SHL-AC-08 against the real account.

## 9. Layer-shell host contract (P3-T02 → P4, 2026-11-26)

The shell's chrome (menu bar, dock, launcher, notifications, control centre) is
described once, in `shell/src/layers.rs`. `shell/src/host.rs` is the shell-side
half of the contract with the compositor:

- `plan()` turns each descriptor into the exact `zwlr_layer_surface_v1` request
  (namespace, layer enum, anchor bitfield, requested size, exclusive zone,
  keyboard interactivity), so the device host has nothing to decide.
- `negotiate()` mirrors the compositor's size rules, so the shell draws a
  surface at the size it will be configured to.
- `BindingReport::scan()` reads the compositor's markers
  (`AETHER_COMPOSITOR_OUTPUT`, `..._LAYER_SURFACE`, `..._LAYER_CONFIGURE`) and
  fails when a surface never bound, bound without a configure, moved layer or
  was configured at a different size.

```sh
aether-shell --host-plan                      # requests the host sends
aether-shell --host-check /tmp/compositor.log # exit 0 only when all bind+configure
cargo test -p aether-shell --test layer_host  # 5 tests, drives the real binary
```

Status: headless-verified on the development host; the socket-level bind runs on
the image because the development host has no `libwayland-client`. Evidence:
`docs/2026/11/26/results/p3-t02-layer-host-wiring.md`.

## 10. Effect-tier control (P4-T04 ↔ P3-T05, 2026-11-27)

The control centre's effects row is the user-facing half of the compositor's
glass tiers. The whole contract is one environment variable plus the
compositor's markers, so neither side needs a new protocol:

| Side | Interface |
| --- | --- |
| Shell → compositor | `AETHER_COMPOSITOR_EFFECT_TIER=full\|reduced\|minimal` (read once at startup) |
| Shell → config | `theme.effects-tier` and `theme.reduce-transparency` in `~/.config/aether/theme/tokens.toml` |
| Compositor → shell/harness | `AETHER_COMPOSITOR_EFFECT_TIER`, `..._EFFECT_TIER_SWITCH`, `..._EFFECT_SUMMARY`, `..._FRAME`, `..._FRAME_CALLBACKS`, `..._IDLE`, `..._FRAME_STATS` |

Rules (`shell/src/effects.rs`):

- Full is the default; Reduced is half-resolution blur without refraction;
  Minimal is solid tinted panels (D1 §4).
- `reduce-transparency = true` always resolves to Minimal, whatever tier is
  requested — an accessibility override, not a preference.
- The settings app's Appearance page and the control centre write the same key,
  so both paths agree and a fresh shell instance sees the persisted tier.
- The running compositor keeps its tier until restart (runtime switches come
  from its own frame-budget policy), so the shell never claims a live switch;
  `--effects-check` compares the log with the setting and names the mismatch.

```sh
aether-shell --effects-tier                     # effective tier, one word
eval "$(aether-shell --effects-env)"            # export the compositor env
aether-shell --effects-check /tmp/compositor.log # exit 0 only when tier+frames+idle agree
cargo test -p aether-shell --lib effects         # 8 unit tests
cargo test -p aether-shell --test effects        # 6 command tests
cargo test -p aether-shell --test effects_ui     # control-centre switch
```

Evidence: `docs/2026/11/27/results/p4-t04-t18b-effects-and-browser-security.md`.

## 11. No-hang dialog, shell watchdog and safe mode (G-NH2, 2026-11-27)

The compositor never waits for an app (P3-T08): it pings each client, logs a
frame gap and watches the shell's commits. The shell's half is the user
decision and the restart policy, and it needs no new protocol — only the
compositor's journal:

| Signal (compositor) | Shell action (`shell/src/nohang.rs`) |
| --- | --- |
| `AETHER_COMPOSITOR_APP_STATE: client=… state=not_responding after_ms=…` | raise `NotRespondingDialog` for that client (app name from the dock/`.desktop` registry) |
| `AETHER_COMPOSITOR_APP_STATE: client=… state=responsive recovered=true` | close the dialog; the app recovered on its own |
| `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout timeouts=…` | count a restart; three stalls arm safe mode on the next start |

Rules:

- One dialog at a time (FIFO by client id); other hung apps queue behind it.
- **Wait** closes the dialog for 15 s and re-offers it while the app is still
  hung, so the user is never trapped in a modal loop.
- **Force Quit** closes it and emits `AETHER_SHELL_NOHANG_TERMINATE: client=…`;
  the host/compositor ends the client (xdg unresponsive error). Windows of every
  other app are untouched.
- The dialog is drawn above every overlay, including the lock screen: a hung app
  must never hide the user's way out.
- Safe mode counts **consecutive starts that never reached "session ready"**
  (`~/.config/aether/shell/session.json`): three in a row, or three shell-watchdog timeouts,
  set it. It resolves the glass tier to Minimal (G-NH2 "reduced session") and
  shows an amber banner with **Reset**; `--start-confirm` clears the count but
  keeps the banner until that reset.

```sh
aether-shell --nohang-check /tmp/compositor.log  # exit 0 only for a hang ≤ 6 s
aether-shell --start-begin [journal]             # count a start (arms safe mode)
aether-shell --start-confirm                     # the session bound its surfaces
aether-shell --safe-mode-reset                   # leave safe mode (banner Reset)
aether-shell --interactive [--journal <log>]     # run the shell host (see §12)
cargo test -p aether-shell --lib nohang           # 12 unit tests
cargo test -p aether-shell --test nohang          # 7 command tests
cargo test -p aether-shell --test nohang_ui       # dialog + banner
```

Status: dialog state machine, safe-mode ledger and marker contract verified
headlessly; on-device steps are in §1c of
`docs/2026/11/26/results/on-device-acceptance-harness.md`. Evidence:
`docs/2026/11/27/results/g-nh2-shell-nohang.md`.

## 12. Interactive shell host (`shell/src/runtime.rs`, 2026-11-27)

The state machines above only matter if something runs them. `aether-shell
--interactive` is that host:

- **Journal in.** `Journal` is a trait: `FileJournal` follows the compositor log
  by byte offset (complete lines only; a rotated file restarts from zero) and
  `MemoryJournal` is the test/fixture source. Nothing in the shell speaks a new
  protocol to the compositor.
- **Timer out.** Each 250 ms step reads the new journal lines, applies every
  `AETHER_COMPOSITOR_APP_STATE` decision with `observe_health`, folds
  `AETHER_COMPOSITOR_SHELL_WATCHDOG` into the ledger, and (once per second) ticks
  the wait grace with `tick_dialog`, so a **Wait** re-offers on its own.
- **Surfaces presented.** Every step shows the Slint `ShellRoot`, pumps timers
  and animations and requests a redraw. `--frames-out <dir>` captures PNG frames
  as evidence on the software renderer; on an image with a display backend the
  capture is skipped, not fatal.
- **Markers published.** Everything the controller logs — dialog raised,
  Wait/Force Quit, recovery, `AETHER_SHELL_NOHANG_TERMINATE`, safe mode, reset —
  is printed to the shell's own journal, so the supervisor/compositor consumes
  the terminate intent from there. `ShellHost::take_terminations` also hands the
  client ids to an in-process supervisor.
- **Session ledger handshake.** `start()` counts the launch before any surface
  exists (`--start-begin`), folding the previous run's watchdog markers first;
  the first presented frame confirms it (`--start-confirm`). A crash between the
  two is a failed start, which is exactly what safe mode counts.

```sh
aether-shell --interactive --journal /var/log/aether-compositor.log   # on the image
aether-shell --interactive --journal fixture.log --frames 4 \
    --act force-quit --frames-out /tmp/shots                          # harness form
cargo test -p aether-shell --test host          # journal → dialog → action → terminate
cargo test -p aether-shell --test host_ledger   # start/confirm/safe mode through the host
```

`--act wait|force-quit` presses the dialog button once when a dialog is on
screen; it calls the same `dialog_action` the Slint callbacks do, so a scripted
press and a click are indistinguishable in the journal. `--frames 0` (the
default) runs until the service manager stops the process.

Hardware-gated: the layer-shell `Platform` that maps these surfaces on a real
output, and the compositor's side of the terminate intent, still need the image;
the loop itself, the ledger and every marker are covered headlessly. Evidence:
`docs/2026/11/27/results/g-nh2-shell-host.md`.

## 13. Accessibility: motion, transparency, text scale, keyboard (SHL-18, 2026-11-27)

Design D1 sections 8–9 define the contract; `shell/src/a11y.rs` resolves it and
`shell/src/kbdnav.rs` owns keyboard navigation. Three settings keys, all in
`theme/tokens.toml`, drive the surfaces:

| Key | Values | Effect |
| --- | --- | --- |
| `theme.reduce-motion` | `true` / `false` | Slides and spring-like moves collapse to `0 ms`; every remaining transition is a **120 ms fade** (`motion-slide`, `motion-base`, `motion-fade` in `ui/design.slint`) |
| `theme.reduce-transparency` | `true` / `false` | Forces the **Minimal** glass tier over any requested tier (SHL-AC-07, D1 §9) |
| `theme.text-scale` | `100%` … `200%` (25 % steps, arbitrary values clamped) | Scales every font token, the menu bar height and the row heights the shell owns |

- **One token binding.** `ui/design.slint` computes `font-*` and
  `menubar-height` from `Tokens.text-scale` and `motion-*` from
  `Tokens.reduce-motion`; `ShellController::sync` sets both globals
  (`ui.global::<Tokens>()`), so no surface threads accessibility properties by
  hand. A fresh shell reads the persisted values before the first frame.
- **Settings app.** `Appearance` gains a "Text scale" select row next to
  "Reduce motion"; both write the same keys the control centre and the CLI read,
  so the settings app, the session environment and the harness agree.
- **Keyboard navigation.** Tab / Shift+Tab move between the menu bar, dock,
  launcher, control centre and notifications (empty surfaces are skipped); the
  arrows move and wrap inside one surface; Enter/Space activate the focused item
  through the same handler as a click; Escape closes the overlay. In the
  launcher the arrows move the search selection and the focus ring follows it.
  Enter on a notification marks it read (the dismiss button stays the mouse
  path); Enter on a settings value cycles it; Enter on a slider nudges it 10 %.
- **Focus indicator.** Exactly one item is focused; the focused widget draws the
  D1 §9 outline — `Tokens.focus-ring` (2 px) in `Tokens.focus-ring-color`
  (`#4C8DFF`) — and exposes `accessible-description: "Focused"` so AT-SPI and
  the headless tests see the same state. The shell reports
  `focus-ring-width` / `focus-ring-color` on `ShellRoot` so the harness can
  assert the contract without reading pixels.

```sh
aether-shell --a11y-status                     # resolved settings + keyboard plan
eval "$(aether-shell --a11y-env)"              # export AETHER_SHELL_* for apps
aether-shell --render-a11y <dir>               # 150 %/200 % and focus-ring evidence
cargo test -p aether-shell --lib a11y          # 6 unit tests (settings, clamp, markers)
cargo test -p aether-shell --lib kbdnav        # 5 unit tests (order, wrap, skip)
cargo test -p aether-shell --test a11y         # 2 command tests (status + env)
cargo test -p aether-shell --test a11y_ui      # settings + keyboard + focus ring in the widget tree
```

Environment contract for apps (the shell publishes, the app honours):

| Variable | Meaning |
| --- | --- |
| `AETHER_SHELL_REDUCE_MOTION` | `1` when motion is reduced |
| `AETHER_SHELL_REDUCE_TRANSPARENCY` | `1` when panels must be solid |
| `AETHER_SHELL_TEXT_SCALE` | factor, `1.00`–`2.00` |
| `AETHER_SHELL_MOTION_MS` / `_FADE_MS` | resolved durations |
| `AETHER_SHELL_FOCUS_RING_PX` | always `2` |

Status: headless-verified on the development host at 100–200 % and with motion
reduced. Hardware-gated: the visual check of the ring and the 2× layout on the
Dell panel, plus the AT-SPI announcement of the focused item (needs the image's
AT-SPI session). Evidence: `docs/2026/11/27/results/shl-18-accessibility.md`.


