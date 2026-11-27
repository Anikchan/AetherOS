# D6 — Compositor: frame callbacks, buffer release, pointer forwarding

- Date: 2026-11-28 (IST, round 3 of the overnight parallel run)
- Territory: `compositor/**`, this file
- Follows: `compositor-seat-focus-injection.md` (keyboard + key injection),
  `compositor-layer-damage.md` (layer placement)
- Requested by the shell session after `d06bdf4` (real Slint pixels in the layer
  buffers) and `874a7fc` (wire keyboard + pointer client)
- Status: **both capabilities done and proven headless, end to end with the real
  shell client.** Hardware (KMS flip timing, real libinput pointer) stays
  hardware-gated — §7.

## 1. What was missing and why it blocked the desktop

| Missing | Effect on the shell |
| --- | --- |
| `wl_surface.frame` never answered when nothing was damaged | the shell paced itself with its own 16 ms clock; a client that waits for the callback before drawing the next frame stalled forever, because presentation was (and is) damage-driven |
| `wl_buffer.release` never sent | the shell owned exactly one buffer and re-attached it every frame; double buffering was impossible |
| `wl_pointer` not forwarded | dock/panel clicks did nothing — a desktop with an unusable dock |

## 2. Frame callbacks and empty frames

Callbacks were already collected and answered in `complete_frame`, but that only
runs when the scheduler says a frame is **due**, and `FrameScheduler::due`
returns false with no damage. A self-pacing client has no damage while it waits,
so the callback could wait forever.

`AetherCompositor::frame_tick` (the headless clock, every 10 ms) now falls back to
a **callback-only frame** when the damage-driven frame is not due but a callback
is pending:

```rust
if self.callbacks_pending() && self.frames.due_for_callbacks(now) {
    let frame = self.frames.present_empty(now);
    self.complete_frame(&frame);
}
```

- `FrameScheduler::due_for_callbacks` ignores damage and only checks the frame
  interval; `present_empty` advances the same pacing clock, counts a frame and
  reports `damage_px=0`, but adds no pixels (`presented_px` is unchanged).
- `Frame::damaged_frames` counts frames that actually carried damage, so the
  sampled `AETHER_COMPOSITOR_FRAME` marker still reports the first five *repaints*
  instead of being spent on empty frames (this was a real regression the first
  run caught: the stale-strip test lost its second damaged frame to the sampling).
- `complete_frame` reports `frost_px=0 backdrop_px=0` for such a frame instead of
  computing a "whole surface" frost, because an empty damage list means
  full-repaint to the frost model.
- The DRM path gets the same fallback in `present_if_due` (an empty frame is
  queued so a flip can answer the callback). Exact flip cost is hardware-gated.

## 3. `wl_buffer.release` (double buffering)

`complete_frame` now calls `release_presented_buffers()` **before** answering
callbacks:

- each surface remembers the buffer it presented in the last completed frame;
- when a newer buffer replaced it (or the client detached), the old one is
  released with `wl_buffer.release` and logged as
  `AETHER_COMPOSITOR_BUFFER_RELEASED: surface=<id>`;
- the buffer currently on screen is never released, which is why double buffering
  alternates instead of tearing;
- the count is reported in the frame summary:
  `AETHER_COMPOSITOR_FRAME_STATS: … buffers_released=<n>`.

Release happens at frame completion, i.e. **after the page flip** on DRM
(`complete_frame` is driven by `DrmEvent::VBlank`) and immediately after
presentation headless — the earliest moment the buffer is really free.

`surface=<id>` is a stable numeric id assigned on first use (layer namespaces and
window ids differ per surface kind, and this marker is about the buffer, not the
window).

## 4. Pointer forwarding

The DRM backend had a private hit test that returned *surface-local* coordinates
as smithay's focus point. smithay computes `wl_pointer` coordinates as
`event.location - focus_position`, so that produced `output - local` — wrong
surface coordinates (our first test run received `surface_x=600` for a click at
`(600,1050)` on a dock at `x=600`). The hit test moved into
`AetherCompositor::pointer_target`, which returns the surface **origin** plus a
marker label:

| Hit | Label |
| --- | --- |
| Top/Overlay layer surface | its namespace (`aether-shell-dock`) |
| managed toplevel | `window-<wm id>` |

Both backends now share one path:

- `pointer_move_to(location, time)` — `wl_pointer.motion` (smithay sends
  `enter`/`leave` as the focus moves) with surface-local coordinates, plus
  `AETHER_COMPOSITOR_POINTER: surface=<label> x=<n> y=<n> event=enter|motion`;
- `pointer_button_at(button, pressed, time)` — `wl_pointer.button`, click-to-focus,
  plus `… event=button` (one marker per forwarded button event, so a click is two
  lines);
- clicking a layer surface with `keyboard_interactivity = none` (dock, menu bar)
  does **not** take the keyboard: clicking chrome must not stop the focused window
  from receiving keys.

libinput (DRM) and the headless test hook both call these two methods, so the
marker and the coordinate maths cannot drift between backends.

## 5. Test-only pointer injection

`AETHER_COMPOSITOR_INJECT_POINTER="x=600,y=1050,button=1"` (output logical
coordinates; `x`/`y` required, `button` an evdev button code and optional):

```text
AETHER_COMPOSITOR_POINTER_INJECTION: backend=headless x=600 y=1050 button=1
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=enter
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=button
```

- one action per frame interval, **before** the queued keys (a click can move
  keyboard focus, so it must land first);
- forwarded through exactly the same `pointer_move_to`/`pointer_button_at` path as
  libinput, so the client sees ordinary `wl_pointer` events;
- a hover is a missing `button=`, not `button=0`: an unparsable button disables
  the injection instead of silently turning a click into a hover;
- like key injection it creates the headless `wl_seat` (with a pointer capability)
  and is ignored by the DRM backend, and an unset variable leaves the headless
  backend with no seat.

Both test hooks now live in `AetherCompositor::enable_headless_input` (was
`enable_key_injection`).

## 6. Tests

New `compositor/tests/p3_t13_frames_pointer.rs` (real compositor, real client,
headless backend with both injection variables). It maps a 720×64 dock anchored
to the bottom edge, `keyboard_interactivity = none`, then asserts:

1. **frame callback without damage** — after `wl_surface.frame()` + a commit with
   nothing to repaint, the client's `wl_callback.done` arrives (before P3-T13 it
   never did);
2. **buffer release** — attaching buffer B after that callback makes the
   compositor release **A and only A** (`wl_buffer.release` with `A`'s user data),
   i.e. double buffering works;
3. **pointer enter** — the injected `(600,1050)` arrives on the dock's
   `wl_pointer` as `surface_x=0, surface_y=34` (surface-local), and the log has
   `AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=enter`;
4. **button** — press then release with button `1`, and the `event=button` marker;
5. **no keyboard theft** — the dock is `keyboard_interactivity = none`, so the
   injected key reports `AETHER_COMPOSITOR_KEY_INJECTED: evdev=15 focus=none`;
6. **summary** — `AETHER_COMPOSITOR_BUFFER_RELEASED: surface=` present and
   `buffers_released=1` in `AETHER_COMPOSITOR_FRAME_STATS`.

Measured journal from that run:

```text
AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true
AETHER_COMPOSITOR_KEY_INJECTION: backend=headless keys=1 evdev=15
AETHER_COMPOSITOR_POINTER_INJECTION: backend=headless x=600 y=1050 button=1
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=enter
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=button
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=0 y=34 event=button
AETHER_COMPOSITOR_KEY_INJECTED: evdev=15 focus=none
AETHER_COMPOSITOR_FRAME_CALLBACKS: n=1 time_ms=92
AETHER_COMPOSITOR_BUFFER_RELEASED: surface=0
AETHER_COMPOSITOR_FRAME_STATS: frames=3 idle_skips=254 idle_ms=2937 idle_pct=97.9 presented_px=138240 avg_gap_ms=30.34 last_gap_ms=36.38 target_fps=60 buffers_released=1
```

Unit tests: `scheduling::tests::callback_frames_advance_the_clock_without_damage`
(empty frame indexes, pacing, `presented_pixels` unchanged, damage still wins) and
three new `inject` parser tests (pointer click, order/spaces, rejection cases).

Mutation check: removing the `release_presented_buffers` call makes the test
time out waiting for `wl_buffer.release`; the DRM hit-test regression was caught
by the test itself on the first run (before → `surface_x=600`, after → `0`).

### End-to-end with the real shell client

```sh
XDG_RUNTIME_DIR=$RT AETHER_COMPOSITOR_BACKEND=headless \
AETHER_COMPOSITOR_INJECT_KEYS=15 \
AETHER_COMPOSITOR_INJECT_POINTER="x=600,y=1050,button=1" ./target/debug/aether-compositor &
XDG_RUNTIME_DIR=$RT WAYLAND_DISPLAY=wayland-0 aether-shell --connect --hold-ms 2500
```

Shell side (its own markers, unmodified by this session):

```text
AETHER_SHELL_POINTER: surface=aether-shell-dock x=0 y=34 button=none state=enter window=600,1050
AETHER_SHELL_POINTER: surface=aether-shell-dock x=0 y=34 button=1 state=press window=600,1050
AETHER_SHELL_POINTER: surface=aether-shell-dock x=0 y=34 button=1 state=release window=600,1050
AETHER_SHELL_LAYER_HOST: display=wayland-0 outputs=2 output=1920x1080 scale=1 seat=keyboard+pointer
AETHER_SHELL_LAYER_HOST_OK
```

Compositor side: the `POINTER_INJECTION` / `POINTER … event=enter|button` lines
above, identical coordinates. **Dock clicks work end to end.**

## 7. Checklist coverage

`kms-checklist.sh` gained two graded checks (both PASS on the fixtures, WARN when
the run simply had no client/pointer activity, FAIL on a malformed marker):

| Check | Rule |
| --- | --- |
| 10. double buffering | `AETHER_COMPOSITOR_BUFFER_RELEASED` count > 0 → PASS; none → WARN |
| 11. pointer forwarding | every `AETHER_COMPOSITOR_POINTER` line must have `surface=`, `x=`, `y=` and `event=enter\|motion\|button`; no line → WARN; a malformed line → FAIL |
| frame stats | `buffers_released=` must be present in `FRAME_STATS` (WARN when the binary predates it) |

Both fixtures gained a representative pointer click and buffer release, plus the
`buffers_released=` counter. `--selftest` gained a sixth case: a pointer marker
with `event=hover` must score FAIL.

## 8. Gates

| Gate | Command | Result |
| --- | --- | --- |
| Lib tests | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor --lib` | **84 passed** (was 81; +1 scheduling, +2 inject) |
| Integration | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor` | **17 passed** (was 16; +`p3_t13_frames_pointer`) |
| Linux target | `./compositor/scripts/check-linux-target.sh` | `Finished dev profile` (DRM pointer path refactored onto the shared helper) |
| Shaders | `./compositor/scripts/check-shaders.sh` | `RESULT: all glass shaders compile` |
| Grader selftest | `./compositor/scripts/kms-checklist.sh --selftest` | `SELFTEST: PASSED` (6 cases) |
| Grader, full fixture | `--grade …/kms-full-tier.log --tier full` | `RESULT: 23 passed, 0 failed, 0 warnings` |
| Grader, minimal fixture | `--grade …/kms-minimal-tier.log --tier minimal` | `RESULT: 22 passed, 0 failed, 1 warning` (pre-existing: no key/button in that fixture) |

The summary counters moved from 20/20 to 23 because two new checks and the
`buffers_released` counter were added; nothing fails.

## 9. Remaining (hardware-gated or follow-ups)

1. **DRM flip cost of callback frames.** `present_if_due` now queues an empty
   frame for a waiting client; whether KMS renders it or reports "nothing to
   render" (which then damages the whole output and flips on the next tick) can
   only be measured on the Dell. Worst case is one full-frame flip per callback
   interval while a client is waiting; the accepted behaviour must be confirmed.
2. **Buffer release vs KMS swapchain age.** Releasing the previous buffer at the
   flip that replaced it is correct with N ≥ 2 swapchain slots; on the device this
   must be confirmed with a tearing/vsync check while dragging a window.
3. **Real libinput pointer** (touchpad, cursor) through the shared
   `pointer_move_to`/`pointer_button_at` path: type-checks for the device target,
   never executed here. The marker volume is also worth reviewing on a real
   session (one line per motion event).
4. **Relative pointer / axis** are still DRM-only and not part of this contract.
5. **Keyboard focus for shell chrome**: the shell now maps only the menu bar and
   the dock (both `keyboard_interactivity = none`), and the launcher is unmapped
   while closed — so nothing holds keyboard focus and injected keys report
   `focus=none` (see the end-to-end log). Delivering global shell shortcuts
   without a focused surface needs a compositor key-binding path; that is a new
   task, not a defect of this contract.
6. **Popups** are still not forwarded pointer events (they are not in the hit
   test); no app uses them yet.
