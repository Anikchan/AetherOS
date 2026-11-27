# D6 — Overlay layer surfaces, frame-callback pacing, and the handshake flake

- Date: 2026-11-28 (IST, overnight run, sixth shell round)
- Session: `shell` (territory `shell/**`, `ui/**`, this file)
- Status: **Done.** The flake is fixed and evidenced first, the three
  full-screen overlays have their own lazy surfaces, and commits now follow the
  compositor's frame clock with two buffers per surface.

## 1. The flake (fixed first)

`cargo test -p aether-shell --test compositor_handshake` failed
`an_injected_click_on_the_dock_activates_its_tile` under default parallelism
while passing alone.

Two things were wrong, and only one of them is "shared state":

1. **Shared CPU and the compositor's frame clock.** The tests do not mutate
   process-global `XDG_RUNTIME_DIR` (each child gets its own via `Command::env`),
   but five tests booting five compositors and five shells at once starve each
   other, and the compositor replays an injected action on *its* frame clock,
   which only starts after the first presented frame. Under that load the action
   landed after the shell's fixed `--hold-ms` had expired.
2. **A unix-socket path trap.** The runtime dir was
   `aether-shell-handshake-<tag>-<pid>`, which under a long `TMPDIR` sits at the
   `SUN_LEN` (104 byte) limit and fails outright for the longer tags.

Fixes: a `static SERIAL: Mutex<()>` guard that **every** test in the binary takes
for its whole body (`fn serial() -> MutexGuard<'static, ()>`), short
`aether-hs-<tag>-<pid>` runtime dirs, and room in the inject tests' holds
(2.5–3 s, with the compositor outliving the shell so it can print
`FRAME_STATS`).

### Acceptance: three consecutive runs, default parallelism

```
$ for i in 1 2 3; do cargo test -p aether-shell --test compositor_handshake; done

test a_missing_socket_is_an_error_not_a_hang ... ok
test an_opened_overlay_is_created_and_painted_over_the_wallpaper ... ok
test an_opened_panel_is_mapped_painted_and_placed ... ok
test an_injected_click_on_the_dock_activates_its_tile ... ok
test frames_follow_the_compositor_and_idle_shell_does_nothing ... ok
test injected_keys_reach_the_shell_and_move_the_ring ... ok
test the_shell_creates_every_layer_surface_on_a_real_compositor ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.02s

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.06s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.02s
```

Whole suite, default threads, three consecutive runs:

```
$ cargo test -p aether-shell            # run 1, 2, 3
passed total: 190 (all green)
passed total: 190 (all green)
passed total: 190 (all green)
```

(`190` is the shell crate alone: 147 lib + 43 integration. With `aether-ui` the
combined suite is 195 — the design crate adds 5.)

`keyboard_wire`, `layer_pixels` and `pointer_click` need no guard: they never
touch `XDG_RUNTIME_DIR` or spawn a compositor, and each installs the Slint
platform once in its own binary.

## 2. The nine-surface plan and the three overlay surfaces

| # | name | namespace | layer | anchors | size (logical) | zone | margin | keyboard | created |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | background | `aether-shell-background` | Background | all 4 | fills → 1920×1080 | −1 | — | none | boot |
| 2 | menubar | `aether-shell-menubar` | Top | top+left+right | 1920×28 | 28 | — | none | boot |
| 3 | dock | `aether-shell-dock` | Top | bottom | 720×64 | 0 | — | none | boot |
| 4 | launcher | `aether-shell-launcher` | Overlay | centred | 640×420 | 0 | — | exclusive | boot |
| 5 | notifications | `aether-shell-notifications` | Top | top+bottom+right | 360×1052 | 0 | 28 | on-demand | boot |
| 6 | control-center | `aether-shell-control-center` | Top | top+bottom+right | 340×1052 | 0 | 28 | on-demand | boot |
| 7 | settings | `aether-shell-settings` | Overlay | all 4 | fills → 1920×1080 | −1 | — | exclusive | **lazy** |
| 8 | overview | `aether-shell-overview` | Overlay | all 4 | fills → 1920×1080 | −1 | — | exclusive | **lazy** |
| 9 | lock | `aether-shell-lock` | Overlay | all 4 | fills → 1920×1080 | −1 | — | exclusive | **lazy** |

The three overlays are `lazy: true` in the table: they are **created the first
time their panel opens** (`ensure_overlays` in `shell/src/wayland.rs`) and
unmapped when it closes, so the boot journal has six surfaces and a boot does not
pay for three full-output surfaces nobody opened.

`--host-plan` always lists all nine, and `--host-check` prints nine
`LAYER_SURFACE` lines; an unbound *lazy* line is `ok=true` (expected later)
while an unbound eager line is still a failure:

```
LAYER_SURFACE name=background ... bound=true lazy=false configured=1920x1080 expected=1920x1080 ok=true
...
LAYER_SURFACE name=settings namespace=aether-shell-settings bound=false lazy=true configured=none expected=1920x1080 ok=true
LAYER_SURFACE name=overview namespace=aether-shell-overview bound=false lazy=true configured=none expected=1920x1080 ok=true
LAYER_SURFACE name=lock namespace=aether-shell-lock bound=false lazy=true configured=none expected=1920x1080 ok=true
AETHER_SHELL_LAYER_HOST_OK
```

Connect report on a fresh session (only the mapped subset is up, all nine are
listed):

```
AETHER_SHELL_LAYER_PLANNED: name=background ... lazy=false created=true
...
AETHER_SHELL_LAYER_PLANNED: name=settings ... lazy=true created=false
AETHER_SHELL_LAYER_HOST_READY: planned=9 created=6 configured=6 mapped=3 painted=3
```

With `--act settings` (or `overview`/`lock`):

```
AETHER_SHELL_LAYER_PLANNED: name=settings namespace=aether-shell-settings layer=Overlay lazy=true created=true
AETHER_SHELL_LAYER_PAINT: name=settings mapped=true buffer=1920x1080 paints=1
AETHER_SHELL_LAYER_HOST_READY: planned=9 created=7 configured=7 mapped=4 painted=4
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-settings layer=Overlay
```

Pixel test (`an_opened_overlay_is_created_and_painted_over_the_wallpaper`): the
opened Settings surface is 1920×1080, has **no (0,0,0) pixel** (opaque, it is a
full-screen panel, not a hole), and differs from `layer-background.png` at more
than half its pixels. A closed overlay is asserted to have `created=false` and no
PNG at all.

`ShellRoot.layer-host` now paints Settings/Overview/Lock into the canvas (their
crops cover the whole frame); the safe-mode banner and the not-responding dialog
are still hidden there because they have no surface of their own.

## 3. Frame-callback pacing and double buffering

Before: one buffer per surface, committed whenever the scene changed, with the
loop sleeping 16 ms between ticks (a timer-driven client).

Now, per surface:

* **Two buffers** (`Slot.buffers[2]`). `wl_buffer.release` marks a buffer free
  (`Dispatch<wl_buffer::WlBuffer, (usize, usize)>`); the next frame writes into a
  released buffer and flips. If both are still held, the frame is *skipped* and
  the work stays `pending` — no commit is queued that the compositor would drop.
* **`wl_surface.frame`** is requested immediately before each commit; the
  `wl_callback.done` event (`Dispatch<wl_callback::WlCallback, usize>`, matched by
  object id so a stale callback cannot unlock a newer frame) is what allows the
  next paint of that surface. The loop polls at 2 ms while a callback is
  outstanding and at `frame_interval` (16 ms, or 100 ms with reduce-motion) when
  idle.
* **50 ms fallback** (`FRAME_FALLBACK`): if no callback arrives, the surface is
  painted anyway. A compositor that is not presenting frames (no damage, or a
  backend without callbacks) would otherwise freeze the shell; 50 ms is three
  frames at 60 Hz, long enough to be a real stall and short enough to be
  invisible.
* The wallpaper is still painted exactly once (`layer 0` short-circuit).

Idle run (`grace 1200 ms`, shell `--hold-ms 400`, no input):

```
AETHER_SHELL_LAYER_RUN: paints=1 commits=3 unmaps=0 keys=0 pointers=0 focus=none elapsed_ms=422
AETHER_COMPOSITOR_FRAME_STATS: frames=1 idle_skips=148 idle_ms=1724 idle_pct=98.6 presented_px=2073600 ... buffers_released=0
```

One frame, three commits (background, menu bar, dock), then nothing: the counters
do not grow and no buffer is replaced.

One injected click (`AETHER_COMPOSITOR_INJECT_POINTER="x=861,y=1048,button=1"`):

```
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=1 state=press window=861,1048
AETHER_SHELL_ACTIVATE: surface=dock item=aether-files
AETHER_SHELL_LAYER_PAINT: name=background ... paints=1
AETHER_SHELL_LAYER_PAINT: name=menubar ... paints=2
AETHER_SHELL_LAYER_PAINT: name=dock ... paints=2
AETHER_SHELL_LAYER_RUN: paints=2 commits=5 keys=0 pointers=3
AETHER_COMPOSITOR_FRAME_STATS: frames=2 ... buffers_released=2
```

Asserted by `frames_follow_the_compositor_and_idle_shell_does_nothing`:
`paints=1 commits=3` while idle with `buffers_released=0`, then `paints=2` for one
click with **`buffers_released>=1`** (it parses the compositor's
`AETHER_COMPOSITOR_FRAME_STATS … buffers_released=` after a graceful shutdown).
Three injected keys produce `paints=3 commits=10` and six releases, i.e. buffer
reuse after release works across several frames.

## 4. Files changed

| File | Change |
| --- | --- |
| `shell/src/layers.rs` | `lazy` on the spec; the three overlay specs (Overlay layer, all-four anchors, 0×0, zone −1, exclusive keyboard); `required_namespaces()` now nine; unit test for their geometry and for "six eager". |
| `shell/src/host.rs` | `HostRequest.lazy`; `SurfaceBinding.lazy` and a lazy-aware `problem()`; `LAYER_SURFACE` line carries `lazy=`. |
| `shell/src/wayland.rs` | Two buffers per surface + `wl_buffer.release`; `wl_surface.frame` + `wl_callback.done` gating with the 50 ms fallback; `pending` work flag; lazy overlay creation (`ensure_overlays`); `PLANNED`/`CREATED`/`READY planned=… created=…` markers; `panel_open` for the three overlays. |
| `shell/ui/surfaces.slint` | Settings, Overview and Lock are painted in `layer-host` mode too (they now have their own surfaces). |
| `shell/src/main.rs` | `--act settings`, `--act overview`, `--act lock`. |
| `shell/tests/compositor_handshake.rs` | `SERIAL` mutex guard for every test; short runtime dirs; `wait_for_shutdown()`; the 9-surface plan assertions; the overlay test; the pacing/idle test. |
| `shell/tests/layer_host.rs` | `bound=true lazy=false` in the summary assertion. |

## 5. Verification

```sh
cargo test -p aether-shell --test compositor_handshake     # 7 passed, three consecutive runs
cargo test -p aether-shell                                  # 190 passed, three consecutive runs
cargo test -p aether-shell -p aether-ui                     # 195 passed
cargo check --workspace                                     # clean
```

The run summary from the flake fix and the pacing transcript above are the
evidence for items 1 and 3; §2 is the evidence for item 2.

## 6. What is still hardware-gated

1. **Real vsync.** The commits now follow `wl_callback.done`, but the headless
   compositor answers callbacks on its own 60 Hz scheduler with no display
   attached; on DRM the callback is the page flip, so latency and pacing must be
   re-measured there. Timing numbers in this D6 are headless.
2. **Buffer count.** Two buffers per surface are enough for a 60 Hz chrome; an
   animated wallpaper or a full-screen overlay redrawn every frame may need
   three. The `free_buffer()` gate already fails safe (it skips the frame).
3. **Overlay keyboard ownership.** Settings/Overview/Lock ask for `exclusive`
   keyboard, and the compositor grants focus on first map. Opening Settings over
   a focused terminal should therefore stop typing in the terminal — correct, but
   it needs a device check that focus returns when the overlay unmaps.
4. **Overlay re-creation.** An overlay is created once and then kept (unmapped
   when closed), not destroyed; a long session with many panel toggles keeps
   three extra full-output surfaces' buffers (3 × 8 MB) alive. Acceptable now,
   worth revisiting with a surface pool.
5. **Pointer on the device** (unchanged): real libinput focus, cursor image and
   drag semantics still need the DRM seat.
6. **Flake hardening for CI.** The inject tests now wait for the compositor's
   own shutdown, which adds ~3 s each; on a slower CI runner the `--hold-ms`
   values may need to grow rather than shrink. The serial guard makes the suite
   slower (18 s vs 3 s) but deterministic.
7. **Doc drift** (other territory): the acceptance harness and features file
   still list six surfaces and the old sizes; they now need to say nine, with the
   three lazy overlays.
