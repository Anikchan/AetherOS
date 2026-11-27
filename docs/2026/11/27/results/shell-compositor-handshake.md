# D6 — Shell ↔ compositor: the real layer-shell handshake, end to end

- Date: 2026-11-27 / 28 (IST, overnight run)
- Session: `shell` (territory `shell/**`, `ui/**`, this file)
- Tasks: P3-T02 residual ("the shell never binds a socket"), F1 SHL-18 keyboard
  path, first-graphical-boot blockers
- Status: **Done for the handshake and keyboard path on the development host.**
  The visual pass (Slint pixels inside the layer surfaces, panel margins,
  on-device IM) is listed in §6.

## 1. What was simulated, and what is real now

| Before | Now |
| --- | --- |
| `shell/src/host.rs` emitted the protocol *values* and `--host-check` verified the compositor's **markers** in a log fixture (`shell/tests/layer_host.rs`). No socket was ever opened. | `shell/src/wayland.rs` is a real `zwlr_layer_shell_v1` client: `aether-shell --connect` connects to `$WAYLAND_DISPLAY`, creates all five surfaces, receives and acks their configures, and attaches an SHM buffer per surface. `shell/tests/compositor_handshake.rs` boots the shipped `aether-compositor` (`AETHER_COMPOSITOR_BACKEND=headless`) and asserts both directions. |
| Keyboard was a string entry point (`handle_key`) with nothing feeding it. | `shell/src/keys.rs` is the wire path: `wl_keyboard.key` (evdev+8) + `wl_keyboard.modifiers` → XKB keysym → `handle_key` / `handle_shortcut`. `shell/tests/keyboard_wire.rs` drives it with wire keycodes only. |

## 2. Files changed

| File | Purpose |
| --- | --- |
| `shell/src/wayland.rs` (new) | The layer-shell host: connects, binds `wl_compositor`/`wl_shm`/`zwlr_layer_shell_v1` (+ `wl_seat`/`wl_keyboard` when advertised), creates the plan's surfaces in order with exact layer/anchors/size/exclusive zone/keyboard mode, acks configures, attaches and fills SHM buffers, reports `AETHER_SHELL_LAYER_*` markers, and collects key presses for the controller. |
| `shell/src/keys.rs` (new) | evdev ⇄ XKB keysym table for the keys the shell owns, `KeyInput::from_wire` (the `+8` offset and Shift/Mod4 masks), `dispatch()` = keycode → keysym → `ShellController::handle_key` / `handle_shortcut`, and the `AETHER_SHELL_KEY` marker. |
| `shell/src/layers.rs` | **Bug fix:** `notifications` and `control-center` had `height: 0` with a top-only anchor — an illegal layer-shell request (see §3). They now anchor `TOP|BOTTOM|RIGHT`. Keyboard interactivity became a three-state `Keyboard` enum (`None`/`OnDemand`/`Exclusive`) instead of a bool. |
| `shell/src/host.rs` | `KEYBOARD_ON_DEMAND = 2`; the request carries the three-state keyboard mode; the plan test asserts an always-mapped panel is never `Exclusive`. |
| `shell/src/main.rs` | New `--connect [--hold-ms <ms>]` host mode; key presses are dispatched through `keys::dispatch` and logged as `AETHER_SHELL_KEY`. |
| `shell/src/lib.rs` | Exports `keys`, `wayland`, `Keyboard`, `KeyInput`, `KeyEvent`. |
| `shell/Cargo.toml` | `wayland-client` 0.31 + `wayland-protocols` 0.32 + `wayland-protocols-wlr` 0.3 (client). Pure-Rust backend, so no `libwayland` is needed on the macOS dev host. |
| `shell/tests/compositor_handshake.rs` (new) | 2 tests: the real handshake (§4) and "a missing socket is an error, not a hang". |
| `shell/tests/keyboard_wire.rs` (new) | 1 test: Tab → Shift+Tab → arrows → Enter → Super+Space → Escape → unowned key, all from keycodes. |

## 3. What the real handshake exposed (and the fixes)

First run of the new test, before any fix:

```
Protocol error 1 on object zwlr_layer_surface_v1@13: height 0 requested
without setting top and bottom anchors
Error: "Wayland roundtrip failed: Backend(Protocol(ProtocolError { code: 1, ... }))"
```

The compositor journal showed the first three surfaces fine and then stopped at
`notifications`: the shell had been asking for `height = 0` with only a *top*
anchor. A zero size along an axis is only legal together with **both** anchors on
that axis, so two of the five shell surfaces could never have been created on
the device. Fixed by anchoring `notifications` and `control-center` to
`TOP|BOTTOM|RIGHT` (their intended size is the full output height, which is what
`negotiate()` already predicted: 360×1080 and 340×1080 — the numbers did not
move, only the anchors became legal).

Second fix, found by reading the same request against the protocol: the
notification column and the control centre were `keyboard_interactivity =
exclusive`. Both are *always mapped* shell chrome, so on the device they would
have taken the keyboard from every window for the whole session (no typing in
apps). They now request `on_demand` (2); only the launcher overlay keeps
`exclusive` (1).

Surface creation order (`menubar → dock → launcher → notifications →
control-center`) and exclusive zones (menubar 28, everything else 0) were
verified against the compositor journal and needed no change.

## 4. Verification

### 4.1 End-to-end handshake (real compositor, real socket)

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo build -p aether-compositor
AETHER_SHELL_TEST_LOG=1 LIBRARY_PATH=/opt/homebrew/lib \
  cargo test -p aether-shell --test compositor_handshake -- --nocapture
```

`test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

Shell side (stdout of the shipped `aether-shell --connect`):

```
AETHER_SHELL_LAYER_HOST: display=wayland-0 outputs=2 seat=none
AETHER_SHELL_LAYER_CREATED: name=menubar namespace=aether-shell-menubar layer=Top anchors=13 size=1920x28 exclusive_zone=28 keyboard=0
AETHER_SHELL_LAYER_CONFIGURE: name=menubar namespace=aether-shell-menubar configured=1920x28 serials=2
AETHER_SHELL_LAYER_CREATED: name=dock namespace=aether-shell-dock layer=Top anchors=2 size=720x64 exclusive_zone=0 keyboard=0
AETHER_SHELL_LAYER_CONFIGURE: name=dock namespace=aether-shell-dock configured=720x64 serials=2
AETHER_SHELL_LAYER_CREATED: name=launcher namespace=aether-shell-launcher layer=Overlay anchors=0 size=640x480 exclusive_zone=0 keyboard=1
AETHER_SHELL_LAYER_CONFIGURE: name=launcher namespace=aether-shell-launcher configured=640x480 serials=2
AETHER_SHELL_LAYER_CREATED: name=notifications namespace=aether-shell-notifications layer=Top anchors=11 size=360x1080 exclusive_zone=0 keyboard=2
AETHER_SHELL_LAYER_CONFIGURE: name=notifications namespace=aether-shell-notifications configured=360x1080 serials=2
AETHER_SHELL_LAYER_CREATED: name=control-center namespace=aether-shell-control-center layer=Top anchors=11 size=340x1080 exclusive_zone=0 keyboard=2
AETHER_SHELL_LAYER_CONFIGURE: name=control-center namespace=aether-shell-control-center configured=340x1080 serials=2
AETHER_SHELL_LAYER_HOST_READY: surfaces=5 configured=5 drawn=5
AETHER_SHELL_LAYER_HOST_OK
```

Compositor side (the headless backend's own journal, same run):

```
AETHER_COMPOSITOR_BACKEND: headless
AETHER_COMPOSITOR_OUTPUT: name=aether-headless-0 mode=1920x1080 scale=1 logical=1920x1080 position=0,0
AETHER_COMPOSITOR_OUTPUT: name=aether-headless-1 mode=2880x1800 scale=1.5 logical=1920x1200 position=1920,0
AETHER_COMPOSITOR_SOCKET_READY: wayland-0
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-menubar layer=Top output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-menubar size=1920x28
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-dock layer=Top output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-dock size=720x64
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-launcher layer=Overlay output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-launcher size=640x480
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-notifications layer=Top output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-notifications size=360x1080
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-control-center layer=Top output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-control-center size=340x1080
AETHER_COMPOSITOR_CLIENT_MAPPED
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-menubar placement=0,0 local=0,0,1920x28 output=0,0,1920x28
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-dock placement=600,1016 local=0,0,720x64 output=600,1016,720x64
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-launcher placement=640,300 local=0,0,640x480 output=640,300,640x480
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-notifications placement=1560,0 local=0,0,360x1080 output=1560,0,360x1080
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-control-center placement=1580,0 local=0,0,340x1080 output=1580,0,340x1080
AETHER_COMPOSITOR_FRAME: n=1 time_ms=1090 damage_rects=1 damage_px=2073600 ... windows=0 tier=full
```

The test also feeds that journal to `BindingReport::scan` (the code behind
`aether-shell --host-check`) and asserts `is_complete()`: the acceptance harness
now runs against a genuine compositor journal, not a fixture.

`seat=none` is correct: the headless backend creates no `wl_seat` (no input
devices). The host reports it instead of inventing a keyboard.

### 4.2 Keyboard path

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell --test keyboard_wire
```

`test result: ok. 1 passed; 0 failed; ...`

The test presses raw wire keycodes (`evdev + 8`, as `wl_keyboard` delivers them)
and asserts, with no pointer event anywhere:

| Input (wire) | Path | Asserted result |
| --- | --- | --- |
| Tab (23) | `handle_key("tab")` | focus menu bar → dock, ring visible, marker `keysym=0xff09` |
| Right (114) | `handle_key("right")` | focus index moves inside the dock |
| Enter (36) | `handle_key("enter")` | focused dock app launches through the same handler a click uses (`active_app`, `app.running`) |
| Shift+Tab (23 + Shift) | `handle_key("shift+tab")` | focus back to the menu bar, `keysym=0xfe20` (ISO_Left_Tab) |
| Super+Space (65 + Mod4) | `handle_shortcut("super+space")` | launcher opens, focus ring moves to it |
| Down/Up (116/111) | `handle_key("down"/"up")` | launcher selection and focus index follow each other |
| Escape (9) | `handle_key("escape")` | overlay closes |
| Mute (121) | none | `handled=false`, focus unchanged (compositor passes it to the app) |
| Shift+A (38 + Shift) | none | `keysym=0x0041`, `handled=false` (typing never reaches the shell) |

### 4.3 Suites

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell -p aether-ui
# 170 passed, 0 failed   (163 + 5 before; +4 keys unit tests, +2 handshake, +1 keyboard)
LIBRARY_PATH=/opt/homebrew/lib cargo check --workspace
# Finished `dev` profile ...  (no errors; the only warning in the tree is
# aether-powerd's unused assignment, owned by another session)
```

`Cargo.lock` carries the shell's new `wayland-*` entries, but the file is
currently shared with another session's in-flight `aether-launcher-registry`
work, so it was **not** committed here; CI does not use `--locked`.

## 5. Marker contract added

| Direction | Marker |
| --- | --- |
| shell → stdout/journal | `AETHER_SHELL_LAYER_HOST: display=… outputs=… seat=none\|keyboard` |
| shell → stdout/journal | `AETHER_SHELL_LAYER_CREATED: name=… namespace=… layer=… anchors=… size=WxH exclusive_zone=… keyboard=0\|1\|2` |
| shell → stdout/journal | `AETHER_SHELL_LAYER_CONFIGURE: name=… namespace=… configured=WxH serials=N` |
| shell → stdout/journal | `AETHER_SHELL_LAYER_HOST_READY: surfaces=… configured=… drawn=…` |
| shell → stdout/journal | `AETHER_SHELL_LAYER_HOST_OK` / `…_PROBLEM: …` / `…_FAILED` (exit 1) |
| shell → stdout/journal | `AETHER_SHELL_KEY: evdev=… keysym=0x… binding=… handled=… focus=…` |

## 6. What remains for the on-device visual pass

1. **Pixels.** `--connect` fills each surface with a flat theme colour; the Slint
   tree is not yet blitted into the SHM buffers. That needs the layer-shell
   `SlintPlatform` from spike S1 (one window per layer surface, software
   renderer into the SHM pool). Everything below the renderer (surfaces,
   configures, buffers) is now proven.
2. **Panel margins.** The compositor places the notification column and the
   control centre at `y=0`, so their top 28 px sit under the menu bar (see the
   `placement=1560,0` / `1580,0` damage lines). The clean fix is a
   `set_margin(TOP, 28)` on those two requests; `zwlr_layer_surface_v1` subtracts
   margins from the negotiated size, so the size contract and `--host-check`
   must gain a margin term in the same change. Not done tonight because it moves
   the documented `360x1080` / `340x1080` numbers.
3. **Surface visibility.** All five surfaces are mapped from startup. The
   launcher is an `Exclusive` overlay, so once the compositor grants focus to
   explicit-keyboard layer surfaces (item 4) it would hold the keyboard while it
   is supposed to be closed. The launcher (and the two panels, when closed) must
   be unmapped — detach the buffer and commit, or destroy the surface — from the
   overlay state machine in `runtime.rs`. The handshake does not need it, the
   interactive session does.
4. **Layer-surface keyboard focus (compositor).** Smithay's layer shell does not
   assign focus itself: the compositor gives the keyboard to xdg toplevels
   (`commit` → `keyboard.set_focus`), and nothing hands it to a layer surface
   with `Exclusive`/`OnDemand` interactivity. Request for the compositor
   session:
   - on `zwlr_layer_surface_v1.commit` (or on `new_layer_surface` + first
     commit), if the surface's `keyboard_interactivity` is `Exclusive`, set
     `seat.get_keyboard().set_focus(surface, serial)` and log
     `AETHER_COMPOSITOR_SEAT_FOCUS: surface=yes layer=<namespace>`;
   - on unmap/destroy of that surface, return focus to the last xdg toplevel.
   With that, `--connect --hold-ms …` will exercise `keys::dispatch` from the
   real socket on the device.
5. **Input injection for CI.** To test the socket keyboard path headless, the
   compositor needs a virtual keyboard (or `AETHER_COMPOSITOR_INJECT_KEY=…`
   that calls `keyboard.input` with a synthetic keycode) in the headless
   backend. Request for the compositor session; the shell side is ready and
   tested at the `keys::dispatch` boundary.
6. **One process.** `--interactive` (journal host) and `--connect` (display
   host) are still two modes of the binary. They become one event loop when the
   Slint layer-shell platform lands (item 1); the start/confirm ledger already
   brackets the first presented frame, so the merge is mechanical.
7. **On-device run** (harness §1): under a real session,
   `aether-shell --connect --hold-ms 5000` plus
   `aether-shell --host-check /tmp/compositor.log` must both print `_OK`, and a
   typed Tab must produce `AETHER_SHELL_KEY … focus=dock` in the journal.
