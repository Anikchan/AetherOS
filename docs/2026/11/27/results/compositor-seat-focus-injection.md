# D6 — Compositor: layer-surface keyboard focus + headless key injection

- Date: 2026-11-28 (IST, overnight parallel run, round 2)
- Territory: `compositor/**`, this file
- Requested by: the shell session, `docs/2026/11/27/results/shell-compositor-handshake.md`
  §6 item 4 (layer-surface keyboard focus) and §6 item 5 (input injection for CI)
- Status: **both hooks done and proven headless on the dev host.** Hardware
  (DRM/libinput) statements remain hardware-gated — see §7.

## 1. What was missing

The shell now really binds `zwlr_layer_shell_v1` (`shell/src/wayland.rs`) and its
keyboard handling is tested at the `keys::dispatch` boundary
(`shell/tests/keyboard_wire.rs`), but the two compositor-side halves of that path
did not exist:

1. `zwlr_layer_shell_v1.keyboard_interactivity` was accepted and ignored: smithay
   never assigns keyboard focus for layer surfaces, and the compositor only gave
   the keyboard to xdg toplevels (`new_toplevel`) and to whatever the pointer
   clicked. An `exclusive` launcher therefore never received a key.
2. The headless backend had **no seat at all** (no input device), so no client
   could ever be handed a `wl_keyboard` on the dev host or in CI, and the socket
   keyboard path could not be exercised without the Dell.

## 2. Hook 1 — layer-surface keyboard focus

`CompositorHandler::commit` now calls `AetherCompositor::update_layer_focus`
after the damage bookkeeping. On the **first commit that carries a buffer** (the
map) of a layer surface, the requested interactivity decides:

| `keyboard_interactivity` | Policy | Why |
| --- | --- | --- |
| `exclusive` | always `keyboard.set_focus(surface)` | launcher/overlay: it owns the keyboard while it is up |
| `on_demand` | only when `keyboard.current_focus().is_none()` | an always-mapped panel must never steal typing from a focused window |
| `none` | never | wallpaper/dock: it must not take the keyboard at all |

Latching on the first mapped commit (a `HashSet<ObjectId>` of mapped layer
surfaces) is what keeps this from turning into a focus fight: a panel that
redraws every frame cannot re-grab the keyboard from the window the user clicked
in the meantime; only the first map grants focus. (A surface that unmaps and maps
again on the same `wl_surface` is therefore not re-granted — part of the unmap
hand-back follow-up in §7.)

`SeatHandler::focus_changed` reports the change and names the surface, so the
shell can assert its own launcher got the keyboard:

```text
AETHER_COMPOSITOR_SEAT_FOCUS: surface=yes                       # an xdg toplevel
AETHER_COMPOSITOR_SEAT_FOCUS: surface=exclusive layer=aether-shell-launcher
AETHER_COMPOSITOR_SEAT_FOCUS: surface=on_demand layer=aether-shell-notifications
AETHER_COMPOSITOR_SEAT_FOCUS: surface=none                      # focus lost
```

`surface=yes` is unchanged for toplevels (the P3-T09 marker), so nothing that
already graded it moved; the layer form is additive.

## 3. Hook 2 — headless key injection (test-only)

`AETHER_COMPOSITOR_INJECT_KEYS="15,28,57"` (comma-separated **evdev** codes,
parsed by the new `compositor/src/inject.rs`) makes the headless backend:

1. create a `wl_seat` with a keyboard and a pointer — there is no device to do it
   otherwise — and log
   `AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true` followed by
   `AETHER_COMPOSITOR_KEY_INJECTION: backend=headless keys=3 evdev=15,28,57`;
2. replay the codes through `KeyboardHandle::input`, **one per frame interval**,
   as a press+release pair, to whatever surface holds keyboard focus, logging
   `AETHER_COMPOSITOR_KEY_INJECTED: evdev=<n> focus=<surface|none>`.

Details that make it deterministic rather than flaky:

- the replay runs from `frame_tick`, which the headless loop calls every 10 ms,
  and is paced to `FrameScheduler::interval()` — so it does not depend on damage
  being present (no frames are presented while the desktop is idle);
- nothing is consumed until a client surface has mapped (`state.mapped`), so the
  keycodes are not spent before the seat has a focus;
- `run_headless` is the only caller: the DRM backend ignores the variable, and an
  unset variable leaves the headless backend without any `wl_seat` global, exactly
  as before (the shell's handshake test still reports `seat=none` there).

Keycode space, verified against the wire and not assumed (see §5): the seat takes
XKB keycodes (`Keycode::new(evdev + 8)`), and smithay's `wl_keyboard` forwards
`keycode − 8`, so **the client sees the evdev code the variable lists** — evdev
30 arrives as `wl_keyboard.key` 30. A client that wants an XKB keysym adds 8,
which is the standard Wayland contract.

## 4. Tests added

`compositor/tests/p3_t12_seat_focus.rs` (new binary, one test, real compositor +
real client) boots the headless backend with `AETHER_COMPOSITOR_INJECT_KEYS=30,48`
and drives it in this order:

1. xdg toplevel maps → `SEAT_FOCUS: surface=yes`, the client's `wl_keyboard` gets
   `enter` and then the injected keys (`(30, pressed), (30, released), (48, …)`);
2. an `on_demand` layer panel maps → the client sees **no** new `enter` and **no**
   `leave`: focus stayed on the toplevel;
3. an `exclusive` overlay maps → the toplevel gets `leave`, the overlay gets
   `enter`, and the log shows `surface=exclusive layer=aether-shell-launcher`;
4. a `none` layer surface maps → no further `leave`/`enter`, and no
   `layer=aether-shell-silent` focus marker in the log.

Compositor journal from that run:

```text
AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true
AETHER_COMPOSITOR_KEY_INJECTION: backend=headless keys=2 evdev=30,48
AETHER_COMPOSITOR_XDG_TOPLEVEL: output=aether-headless-0 client=0
AETHER_COMPOSITOR_SEAT_FOCUS: surface=yes
AETHER_COMPOSITOR_CLIENT_MAPPED
AETHER_COMPOSITOR_KEY_INJECTED: evdev=30 focus=surface
AETHER_COMPOSITOR_KEY_INJECTED: evdev=48 focus=surface
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-panel layer=Top output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-launcher layer=Overlay output=aether-headless-0 scale=1
AETHER_COMPOSITOR_SEAT_FOCUS: surface=exclusive layer=aether-shell-launcher
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-silent layer=Top output=aether-headless-0 scale=1
```

Note there is no `on_demand` and no `silent` focus line: the panel and the
keyboard-disabled surface never got the keyboard.

Mutation evidence (each mutation applied temporarily, test re-run, code
restored — all three make the test fail, so the assertions are not vacuous):

| Mutation | Failure |
| --- | --- |
| `update_layer_focus` call removed | `the toplevel must lose the keyboard to the exclusive overlay` |
| `on_demand` → always take focus | `an on_demand panel must not take focus from a mapped toplevel` |
| `none` → take focus | `a keyboard_interactivity=none surface must never take focus` |

`compositor/src/inject.rs` adds three unit tests for the parser: list of codes,
whitespace/empty fields, junk and out-of-range codes dropped without losing the
list, and the shell's own key codes (Tab/Enter/Space/Super).

## 5. End-to-end run with the real shell client

The shell's shipped host was run against a compositor with injection enabled
(read-only cross-check; nothing under `shell/**` was modified):

```sh
XDG_RUNTIME_DIR=$RT AETHER_COMPOSITOR_BACKEND=headless \
AETHER_COMPOSITOR_INJECT_KEYS=15,28,58 ./target/debug/aether-compositor &
XDG_RUNTIME_DIR=$RT WAYLAND_DISPLAY=wayland-0 \
  cargo run -q -p aether-shell --bin aether-shell -- --connect --hold-ms 2000
```

Compositor side:

```text
AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true
AETHER_COMPOSITOR_KEY_INJECTION: backend=headless keys=3 evdev=15,28,58
AETHER_COMPOSITOR_CLIENT_MAPPED
AETHER_COMPOSITOR_SEAT_FOCUS: surface=exclusive layer=aether-shell-launcher
AETHER_COMPOSITOR_KEY_INJECTED: evdev=15 focus=surface
AETHER_COMPOSITOR_KEY_INJECTED: evdev=28 focus=surface
AETHER_COMPOSITOR_KEY_INJECTED: evdev=58 focus=surface
```

Shell side:

```text
AETHER_SHELL_LAYER_HOST: display=wayland-0 outputs=2 seat=keyboard
… five surfaces created, configured, drawn …
AETHER_SHELL_LAYER_HOST_OK
AETHER_SHELL_KEY: evdev=7 keysym=none binding=none handled=false focus=menubar
AETHER_SHELL_KEY: evdev=20 keysym=none binding=none handled=false focus=menubar
AETHER_SHELL_KEY: evdev=50 keysym=0x006d binding=m handled=false focus=menubar
```

The whole path works — seat advertised, exclusive layer focused, keys delivered
to its `wl_keyboard` and decoded by `shell/src/keys.rs`. Two shell-side
discrepancies are visible in those three lines and are **the shell session's to
fix** (reported in the handoff):

1. **8-off keycode translation.** Injected evdev 15 reaches the shell as
   `evdev=7` (15 − 8), 28 as 20, 58 as 50. `shell/src/keys.rs` assumes
   `wl_keyboard.key` is `evdev + 8` and subtracts 8 (`from_wire(23) → evdev 15`,
   asserted in `keyboard_wire.rs`); the wire value is already the evdev code
   (xkb keycode − 8, which is what smithay and wlroots send). The client should
   treat the wire value as evdev and add 8 only for XKB keysym lookup.
2. **`focus=` label.** The compositor focused `aether-shell-launcher`, but the
   shell logs `focus=menubar`: its dispatch reports the controller's own focus
   ring rather than the surface that received the key. Worth aligning so a
   `focus=` value in `AETHER_SHELL_KEY` means the same thing on both sides.

## 6. Gates

| Gate | Command | Result |
| --- | --- | --- |
| Lib tests | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor --lib` | **81 passed** (was 78; +3 `inject` parser tests) |
| Integration tests | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor` | **16 passed** (was 15; +`p3_t12_seat_focus`) |
| Linux target type-check | `./compositor/scripts/check-linux-target.sh` | `Finished dev profile` |
| Shaders | `./compositor/scripts/check-shaders.sh` | `RESULT: all glass shaders compile` |
| KMS grader selftest | `./compositor/scripts/kms-checklist.sh --selftest` | `SELFTEST: PASSED` (5 cases) |
| KMS grader on fixture | `./compositor/scripts/kms-checklist.sh --grade …/kms-full-tier.log --tier full` | `RESULT: 20 passed, 0 failed, 0 warnings` |
| Shell handshake regression | `cargo test -p aether-shell --test compositor_handshake` | `2 passed` (the headless seat stays absent without the env var) |

## 7. Remaining (hardware-gated or follow-ups)

1. **Real libinput keyboard focus on layer surfaces**: the hook is backend
   independent (it only calls `keyboard.set_focus`), but only the headless seat
   has been exercised. On the Dell, a click on a panel must not be overridden by
   the `on_demand` grant (it cannot be: the grant happens once, at map), and an
   `exclusive` overlay must keep the keyboard while it is up. Needs a KMS run.
2. **Unmap/destroy hand-back**: when an `exclusive` overlay goes away the
   compositor does not restore focus to the previous toplevel — nothing calls
   `set_focus` again, so the keyboard stays on a dead surface until the user
   clicks. The shell's own plan (handshake §6 item 3) is to unmap the launcher
   when it closes; the compositor needs a `last_toplevel: Option<WlSurface>` to
   fall back to. Tracked here, not implemented (it changes focus policy for every
   client and deserves its own review).
3. **New toplevel vs focused overlay**: `new_toplevel` still takes the keyboard
   unconditionally (P3-T09 behaviour), so a window that maps while an `exclusive`
   overlay is focused will steal it. Same follow-up as (2): focus policy needs one
   owner instead of three call sites.
4. **Injection is not a device**: press+release pairs are synthetic, there is no
   repeat and no timing jitter, so it proves delivery and routing, not
   autorepeat or key-repeat policy. Modifiers work (they come from the XKB state
   the seat builds up), but a modifier-only sequence across frames was not tested.
5. **`focus=none`** (a key delivered with no focused client) is implemented and
   logged, but the test suite never produces it: with no mapped client nothing is
   injected by design. Covered by inspection of one branch only.
6. **XWayland**: X11 windows never receive focus from these hooks (they are
   toplevels, and `XwmHandler` does not call `set_focus`). Untested, hardware-
   and XWayland-gated.

## 8. Follow-up in this round

The pointer half of the same test hook (`AETHER_COMPOSITOR_INJECT_POINTER`),
`wl_surface.frame` callbacks for self-pacing clients and `wl_buffer.release` for
double buffering landed next: see
`docs/2026/11/27/results/compositor-frames-pointer.md`. The `enable_key_injection`
entry point named in §3 is now `enable_headless_input` (it sets up the same
headless seat for keys and pointer).
