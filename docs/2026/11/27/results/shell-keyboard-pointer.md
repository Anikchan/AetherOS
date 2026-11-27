# D6 — Shell keyboard + pointer over the socket, and the off-by-8 keycode bug

- Date: 2026-11-28 (IST, overnight run, fourth shell round)
- Session: `shell` (territory `shell/**`, `ui/**`, this file)
- Depends on: compositor commit `a8a95e2` — exclusive layer surfaces take the
  keyboard on first map, and `AETHER_COMPOSITOR_INJECT_KEYS` replays evdev codes
  into the focused client (`AETHER_COMPOSITOR_KEY_INJECTED: evdev=<n> focus=<surface|none>`).
- Status: **Done for keys and the pointer client path.** The pointer has no
  headless injection hook yet, so its socket leg asserts the binding only (§4.3).

## 1. The bug: every key was shifted by 8

`shell/src/keys.rs` assumed `wl_keyboard.key.key` carried `evdev + 8` and
subtracted 8. It does not: the event carries the **evdev keycode**. Injected
evdev 15 (Tab) arrived as 15 and the shell logged `evdev=7`, which is not a key
it owns, so on the device *no* key would have worked (Tab 15 → 7, Enter 28 → 20,
Esc 1 → 0 …). Fixed:

| Was | Now |
| --- | --- |
| `KeyInput::from_wire(code, mods)` → `evdev = code - 8` | `evdev = code` (the wire value is the evdev code) |
| `WL_KEYCODE_OFFSET = 8` | `XKB_KEYCODE_OFFSET = 8` + `xkb_keycode(evdev) = evdev + 8`, documented as "what a keymap is indexed with" |
| keysym table consulted with the wrong code | keysym table is the default `us` layout keyed by evdev: `keysym = keymap(xkb_keycode(evdev))` |

Unit tests pin it: `keysym(15) = 0xff09`, `keysym(28) = 0xff0d`,
`keysym(1) = 0xff1b`, `xkb_keycode(15) = 23`, `from_wire(15).evdev == 15`, and
`keysym(7) == None` (7 is what the old bug produced).

## 2. The second bug: `focus=` named the wrong surface

`AETHER_SHELL_KEY` reported `focus=<shell focus ring>` while the compositor had
focused the *launcher layer surface* — the label answered a different question
than a reader asks ("which surface got the key?"). The marker now separates them:

```
AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true \
  focus=aether-shell-launcher ring=control-center
```

* `focus=` is the layer surface that holds `wl_keyboard` focus, tracked from
  `wl_keyboard.enter` / `leave`; it matches the compositor's
  `AETHER_COMPOSITOR_SEAT_FOCUS … layer=<namespace>`.
* `ring=` is the shell's own focus ring after the event (`kbdnav::Surface`).

## 3. What changed

| File | Change |
| --- | --- |
| `shell/src/keys.rs` | The keycode fix and the `focus=`/`ring=` split; `xkb_keycode()`; tests for 15/28/1. |
| `shell/src/wayland.rs` | Tracks `wl_keyboard` focus per slot (`key_focus`, from `enter`/`leave`) and passes the namespace into `keys::dispatch`; binds `wl_pointer` next to the keyboard and queues `enter`/`motion`/`button`/`leave` with the slot and surface-local position; the run loop **round-trips each frame** (so queued input is read, not just already-buffered events) and routes keys, pointer events and new controller log lines; reports `seat=keyboard+pointer`, `pointers=` and `focus=` in the run/seat markers. |
| `shell/src/pointer.rs` *(new)* | `PointerEvent` (surface-local + placement), `PointerKind`, `PointerRouter::route()` → Slint `PointerMoved`/`PointerPressed`/`PointerReleased`/`PointerExited` at `placement + local`, plus the `AETHER_SHELL_POINTER` marker and `BTN_*` → `PointerEventButton` mapping. |
| `shell/src/app.rs` | `activate_focus()` logs `AETHER_SHELL_ACTIVATE: surface=<s> index=<i>`, so the journal shows *what* Enter/Space activated. |
| `shell/src/main.rs` | `--connect` publishes controller log lines alongside key and pointer markers. |
| `shell/src/lib.rs` | Exports `pointer` (`PointerEvent`, `PointerRouter`). |
| `shell/tests/compositor_handshake.rs` | New socket test: compositor started with `AETHER_COMPOSITOR_INJECT_KEYS=15,15,28`, shell `--connect --act launcher`, asserts the three key markers, the two ring moves, the activation, the compositor's focus/injection markers, `seat=keyboard+pointer`, and that `evdev=7` never appears. |
| `shell/tests/pointer_click.rs` *(new)* | A socket-shaped click on dock tile 0 (surface-local 261,32) through `PointerRouter` activates `aether-files` via the same handler the keyboard uses; a click in the transparent part of the dock surface activates nothing; `leave` clears the hover state. |
| `shell/tests/keyboard_wire.rs` | Uses evdev codes directly (it had encoded the `+8` bug) and asserts the new `focus=`/`ring=` fields. |

## 4. Transcripts

### 4.1 Injected keys over a real socket

```sh
AETHER_COMPOSITOR_BACKEND=headless AETHER_COMPOSITOR_GRACE_MS=30000 \
AETHER_COMPOSITOR_INJECT_KEYS=15,15,28 ./aether-compositor &
./aether-shell --connect --hold-ms 2000 --act launcher
```

Shell:

```
AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true focus=aether-shell-launcher ring=control-center
AETHER_SHELL_KEY: evdev=15 keysym=0xff09 binding=tab handled=true focus=aether-shell-launcher ring=notifications
AETHER_SHELL_KEY: evdev=28 keysym=0xff0d binding=enter handled=true focus=aether-shell-launcher ring=notifications
AETHER_SHELL_ACTIVATE: surface=notifications index=0
AETHER_SHELL_LAYER_HOST: display=wayland-0 outputs=2 output=1920x1080 scale=1 seat=keyboard+pointer
AETHER_SHELL_LAYER_RUN: paints=3 commits=9 unmaps=0 keys=3 pointers=0 focus=aether-shell-launcher elapsed_ms=2019 paint_ms=26.198
```

Compositor:

```
AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true
AETHER_COMPOSITOR_KEY_INJECTION: backend=headless keys=3 evdev=15,15,28
AETHER_COMPOSITOR_SEAT_FOCUS: surface=exclusive layer=aether-shell-launcher
AETHER_COMPOSITOR_KEY_INJECTED: evdev=15 focus=surface
AETHER_COMPOSITOR_KEY_INJECTED: evdev=15 focus=surface
AETHER_COMPOSITOR_KEY_INJECTED: evdev=28 focus=surface
```

Reading it: the exclusive launcher took the keyboard (`SEAT_FOCUS … exclusive`),
all three evdev codes reached the client **unshifted** (`evdev=15`, not 7), the
ring moved twice (launcher → control centre → notification centre — Tab cycles
surfaces and both panels always have items), and Enter ran the activation handler
for the focused item. `paints=3 commits=9` shows each key repainted exactly the
surfaces it touched.

The old code path, for the record, was `evdev=7 keysym=none binding=none
handled=false` for the same injected 15.

### 4.2 Synthetic dock click (the pointer path without an injection hook)

`shell/tests/pointer_click.rs` drives `PointerRouter` with
`enter → motion → press(BTN_LEFT) → release(BTN_LEFT)` at surface-local
`(261, 32)` on the dock surface (placement `600,1016`):

```
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=none state=enter window=861,1048
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=272 state=press window=861,1048
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=272 state=release window=861,1048
```

`active_app` goes `AetherOS → Files` through the same `dock-chosen` handler the
keyboard's Enter uses. A click at surface-local `(20, 32)` — inside the 720 px
surface but outside the 270 px panel, i.e. the transparent part — changes
nothing.

### 4.3 The pointer over the socket

The headless seat created by the injection hook advertises a pointer
(`AETHER_COMPOSITOR_SEAT: … pointer=true`), and the shell binds it: the host
line reads `seat=keyboard+pointer`, which the injected-key test asserts. There is
no `AETHER_COMPOSITOR_INJECT_POINTER` yet, so no `wl_pointer.enter/motion/button`
arrives over the socket and the shell's pointer markers cannot be produced from
the compositor side. The client side is complete and tested at the router
boundary; when the hook lands the same assertions can be made from the journal
with no shell change (the dispatch and markers already exist).

## 5. Verification

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell -p aether-ui
# 189 passed, 0 failed   (184 before: +1 keys keysym test, +2 pointer unit
#                         tests, +1 pointer_click, +1 injected-key socket test;
#                         the old wire-offset test was replaced by the fix)
LIBRARY_PATH=/opt/homebrew/lib cargo check --workspace
# clean (no errors, no new warnings in the shell or the design system)
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell --test compositor_handshake
# 4 passed (surfaces+pixels, opened panel, injected keys, missing socket)
```

## 6. Marker contract changes

| Marker | Change |
| --- | --- |
| `AETHER_SHELL_KEY` | `evdev` is now the evdev code the compositor injected/sent (was `code - 8`); `focus=` names the surface that received the key (`none` without a seat); the shell's ring moved to `ring=`. |
| `AETHER_SHELL_POINTER` *(new)* | `surface=<namespace> x=<n> y=<n> button=<n|none>` plus `state=enter\|motion\|press\|release\|leave` and `window=<x>,<y>`. |
| `AETHER_SHELL_ACTIVATE` *(new)* | `surface=<s> index=<i>` — what Enter/Space activated. |
| `AETHER_SHELL_LAYER_HOST` | `seat=` now reads `keyboard`, `pointer`, `keyboard+pointer` or `none`. |
| `AETHER_SHELL_LAYER_RUN` | Adds `pointers=<n>` and `focus=<namespace\|none>`. |

## 7. Remaining hardware-gated items

1. **`AETHER_COMPOSITOR_INJECT_POINTER`** (compositor): a pointer replay hook
   mirroring the key one — `enter` on a mapped surface at a given local position,
   then motion/press/release, logged as `AETHER_COMPOSITOR_POINTER_INJECTED`.
   With it, §4.2's assertions become a socket test with no shell change.
2. **Real pointer focus** (compositor): the compositor currently gives the
   keyboard to exclusive layer surfaces but leaves `wl_pointer` focus alone, so a
   click on the dock over the socket needs the compositor to send pointer enter.
3. **Frame pacing** (shell + compositor): the run loop round-trips once per tick
   to read input, which is correct but costs a sync per frame. `wl_surface.frame`
   callbacks plus `prepare_read`/`poll` will replace it (and with it the 16 ms
   sleep) on the DRM backend.
4. **On-device keys**: the injected-key test covers the protocol path; the real
   libinput → xkbcommon → `wl_keyboard` path still needs the DRM seat, and the
   shell's keymap is the built-in `us` table — layouts that differ (or
   `wl_keyboard.keymap` remaps) need either a keymap parser or the compositor
   sending keysyms. Today the shell owns Tab/Enter/Escape/arrows/Space/Super+key,
   which are layout-independent on `us`; letters (Super+A/C/F/L/N) are not.
5. **`on_demand` panels**: the launcher is `exclusive` so it always takes the
   keyboard; the notification/control-centre panels take it only when nothing
   else holds it (compositor policy), which is what the shell asked for. Worth a
   device check that opening a panel from a focused terminal does not steal
   typing.
