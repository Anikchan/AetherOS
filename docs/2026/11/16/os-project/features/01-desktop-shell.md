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
| SHL-AC-04 | Control center changes Wi-Fi, Bluetooth, volume, and brightness | Implemented, headless-verified with mock services — nmcli/bluetoothctl/wpctl/brightnessctl adapters exist; on-device confirmation pending |
| SHL-AC-05 | Focus mode hides notifications during the scheduled time and shows them after | Implemented, headless-verified — the daily window comes from `notifications.toml` (`focus-start`/`focus-end`, overnight supported), manual Focus runs two hours, the minute tick expires it and refreshes cards and badge; the settings pane edits the keys |
| SHL-AC-06 | Snap to halves and quarters works with a mouse drag and a keyboard shortcut | Not tested — compositor work (P3-T04) |
| SHL-AC-07 | Glass tiers switch without restart; Minimal tier used when reduce transparency is on | Partially verified — tier selection and the reduce-transparency override are unit-tested; live switching needs the effect pipeline (P3-T05) |
| SHL-AC-08 | Lock screen rejects a wrong password and accepts the right one | Implemented, headless-verified (2026-11-26) — the screen is modal (scrim, Escape and shortcuts cannot dismiss it), five wrong attempts rate-limit the surface, and authentication goes through an `Authenticator` backend: greetd + PAM for login, `libpam` (`aether-lock` service) for the in-session lock, with the salted-digest prototype as the development fallback. PAM FFI is type-checked (`cargo check --features pam`); on-device PAM confirmation pending the image build |
| SHL-AC-09 | Every settings category opens and every change is written to the config file | Implemented, headless-verified — categories switch the page (clicked in the test), rows are generated from the F4 schema, each shows file+key, and writes are atomic and read back from disk |
| SHL-AC-10 | Two monitors with different scales work without restart | Partially verified (headless, 2026-11-26) — the compositor's `p3_t02_headless` test asserts two outputs at 1× and 1.5× including the logical layout, and the shell verifies the configure size per output with `aether-shell --host-check` (menubar 1920×28 on a 1.5×/1920-logical panel). Live re-scale on a real second monitor pending the on-device run |
| SHL-AC-11 | Screen reader announces menu bar items and launcher results | Partially verified — every interactive element sets an accessible role/label (asserted by the headless tests); AT-SPI session pending |

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
