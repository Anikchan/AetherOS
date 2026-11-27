# D6 — Shell paints the Slint scene into the layer surfaces (P3-T02 → P4)

- Date: 2026-11-28 (IST, overnight run, third shell round)
- Session: `shell` (territory `shell/**`, `ui/**`, this file)
- Status: **Done on the development host.** The `--connect` host now renders the
  real Slint scene into the five `/wl_shm` buffers the compositor composites, so
  the first graphical boot shows AetherOS chrome instead of palette blocks.
  GPU rasterisation and vsync pacing remain hardware-gated (§7).

## 1. What changed

| File | Change |
| --- | --- |
| `shell/src/paint.rs` *(new)* | `LayerPainter`: prepares the shell window for layer hosting, renders the composed scene into a premultiplied ARGB canvas only when Slint is dirty, reports the painted rectangles, and crops/upscales one surface out of the canvas into `Argb8888` bytes. |
| `ui/src/lib.rs` | `render::render_argb_into()` — paints a component into a caller-owned `&mut [PremultipliedRgbaColor]` and returns the painted `DirtyRect`s (or `None` when nothing changed). `render_rgb` (screenshots) is untouched. |
| `shell/src/wayland.rs` | Binds `wl_output` per instance (scale) and follows `wl_surface.enter`; sets the top margin per surface; creates `Argb8888` buffers at the configured size × scale; writes pixels, `damage_buffer`, `attach`, `commit`; maps/unmaps panels by overlay state; drives the frame loop (keys, timers, one commit per surface per frame) and dumps per-surface PNGs for evidence. |
| `shell/src/layers.rs` | `specs_at(text_scale)`: the menu bar is `28px × text-scale`, the right-hand panels widen with the text scale (`+140px` per step) and reserve the menu-bar height as `margin_top`, the launcher follows its `.slint` height (`420px + (ts-1)×100`). Scale clamped to 100–200 %. |
| `shell/src/host.rs` | `HostRequest.margin_top`, margin-aware `negotiate()` (mirrors the compositor's `layer_surface_size`), and `placement()` (the layer-shell anchoring rule) so the host knows where each surface lands without a round trip. `plan_at(text_scale)`. |
| `shell/src/main.rs` | `--connect` now paints: `--frames-out <dir>` writes `layer-<name>.png` per surface, `--act <panel>` opens a panel before painting (harness driver, same idea as `--interactive --act`), and the run prints paint/commit counters and the paint time. Apps spawn for real (`set_spawn_apps`). |
| `shell/ui/surfaces.slint` | `ShellRoot.layer-host` mode: transparent window background, no wallpaper/desktop preview/scrim, dock on the bottom edge, launcher centred, right-hand panels flush to the right edge below the menu bar. Overlays that have no layer surface yet (Settings, Overview, Lock, Not-You-Responding, safe-mode banner) are not painted in this mode. |
| `shell/tests/compositor_handshake.rs` | +1 test and pixel assertions: menu-bar strip is its glass tint with the accent logo on the left only, dock centre is its tint with transparent corners and painted icons, closed panels are unmapped and have no pixels, placements match the compositor's own marker. |
| `shell/tests/layer_pixels.rs` *(new)* | Frame behaviour without a compositor: idle shell does not repaint, a text-scale change repaints and changes pixels, the tier reaches the components, reduce-motion slows the frame clock, plus the timings in §6. |

No `Cargo.toml` change was needed: the workspace `slint` dependency already
enables `renderer-software`, and `PremultipliedRgbaColor` implements
`TargetPixel` in that renderer, which is exactly what provides the alpha channel
the compositor needs for the gaps between panels.

## 2. How the scene reaches the surfaces

```
ShellRoot (layer-host: true)              one 1920×1080 logical canvas
        │  Slint software renderer, ARGB, dirty-region aware
        ▼
LayerPainter.canvas  ──crop/upscale──►  menubar   1920×28
        │                               dock       720×64  (panel centred, rest transparent)
        │                               launcher   640×420  (only while open)
        │                               notif.     360×1052 (margin_top 28, only while open)
        └────────────────────────────►  control    340×1052 (margin_top 28, only while open)
                                            │
                              Argb8888 wl_shm buffer + damage_buffer + commit
```

* **One render, five crops.** The five panels are one composed scene; rendering
  it once and cropping is both cheaper and impossible to desynchronise. The
  panels sit at the same coordinates the compositor places their surfaces at, so
  a crop is the surface's content with no offset (verified against the
  compositor's `AETHER_COMPOSITOR_LAYER_DAMAGE placement=` marker).
* **Transparent outside the panel.** `layer-host` mode makes the window
  background transparent, so the rounded corners and the empty part of the
  notification column are alpha 0 and the compositor shows the desktop through.
* **Change-driven.** `render_argb_into` returns `None` when Slint has nothing to
  redraw; that is the single switch for settings changes, key input, focus/hover
  and animation ticks. A repaint commits only the surfaces whose rectangle
  intersects the regions Slint painted, at most once per surface per frame.
  Idle cost is one `dispatch_pending` + timer advance per 16 ms tick and no
  rendering: `--hold-ms 1500` reports `paints=1 commits=2`.
* **Scale.** `wl_output.scale` for the output the surface entered decides the
  buffer size (`logical × scale`) and `set_buffer_scale`. The scene itself is
  laid out at logical size (see §7 for the HiDPI caveat).
* **Closed panels are unmapped.** Only the menu bar and the dock are up on a
  fresh session; the launcher/notification/control-centre surfaces are created
  and configured but never attached until their overlay opens, and are unmapped
  (`attach(null)` + commit) when it closes.

## 3. Sizes and margins

| Surface | Logical size | Margin | Note |
| --- | --- | --- | --- |
| menubar | 1920 × 28 | — | `28 × text-scale`; exclusive zone follows |
| dock | 720 × 64 | — | 270 px panel centred in a wider surface, rest transparent |
| launcher | 640 × 420 | — | `420px + (ts-1)×100` matches the `.slint` panel |
| notifications | 360 × 1052 | top 28 | `360px + (ts-1)×140`, below the menu bar |
| control centre | 340 × 1052 | top 28 | `340px + (ts-1)×140`, below the menu bar |

`negotiate()` and `placement()` mirror the compositor's `layer_surface_size()`
and anchoring rules; the handshake test asserts the compositor's own damage
placement equals them.

## 4. Test transcript

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo build -p aether-compositor
XDG_RUNTIME_DIR=$RT AETHER_COMPOSITOR_BACKEND=headless \
  AETHER_COMPOSITOR_GRACE_MS=40000 ./target/debug/aether-compositor > $RT/c.log &
XDG_RUNTIME_DIR=$RT WAYLAND_DISPLAY=wayland-0 \
  ./target/debug/aether-shell --connect --hold-ms 1500 --frames-out /tmp/shots
```

```
AETHER_SHELL_LAYER_HOST: display=wayland-0 outputs=2 output=1920x1080 scale=1 seat=none
AETHER_SHELL_LAYER_CREATED: name=menubar ... size=1920x28 exclusive_zone=28 margin_top=0 keyboard=0
AETHER_SHELL_LAYER_CONFIGURE: name=menubar configured=1920x28 serials=2 placement=0,0 scale=1
AETHER_SHELL_LAYER_PAINT: name=menubar mapped=true buffer=1920x28 paints=1 last_ms=1.113
AETHER_SHELL_LAYER_CREATED: name=dock ... size=720x64 ...
AETHER_SHELL_LAYER_CONFIGURE: name=dock configured=720x64 serials=2 placement=600,1016 scale=1
AETHER_SHELL_LAYER_PAINT: name=dock mapped=true buffer=720x64 paints=1 last_ms=0.991
AETHER_SHELL_LAYER_CREATED: name=launcher ... size=640x420 ... keyboard=1
AETHER_SHELL_LAYER_CONFIGURE: name=launcher configured=640x420 serials=2 placement=640,330 scale=1
AETHER_SHELL_LAYER_PAINT: name=launcher mapped=false buffer=0x0 paints=0 last_ms=0.000
AETHER_SHELL_LAYER_CREATED: name=notifications ... size=360x1052 margin_top=28 keyboard=2
AETHER_SHELL_LAYER_CONFIGURE: name=notifications configured=360x1052 serials=2 placement=1560,28 scale=1
AETHER_SHELL_LAYER_PAINT: name=notifications mapped=false buffer=0x0 paints=0 last_ms=0.000
AETHER_SHELL_LAYER_CREATED: name=control-center ... size=340x1052 margin_top=28 keyboard=2
AETHER_SHELL_LAYER_CONFIGURE: name=control-center configured=340x1052 serials=2 placement=1580,28 scale=1
AETHER_SHELL_LAYER_PAINT: name=control-center mapped=false buffer=0x0 paints=0 last_ms=0.000
AETHER_SHELL_LAYER_HOST_READY: surfaces=5 configured=5 mapped=2 painted=2
AETHER_SHELL_LAYER_HOST_OK
AETHER_SHELL_LAYER_RUN: paints=1 commits=2 unmaps=0 keys=0 elapsed_ms=1508 paint_ms=28.261
```

Compositor journal for the same run:

```
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-menubar placement=0,0 local=0,0,1920x28 output=0,0,1920x28
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-dock placement=600,1016 local=0,0,720x64 output=600,1016,720x64
AETHER_COMPOSITOR_FRAME: n=1 time_ms=940 damage_rects=2 damage_px=99840 frost_px=0 ... tier=full
```

`damage_px=99840` is exactly the menu bar (1920×28 = 53760) plus the dock
(720×64 = 46080): the compositor accounted nothing else, i.e. the shell damaged
only what it actually painted.

With `--act launcher` the launcher surface is mapped and painted as well:

```
AETHER_SHELL_LAYER_PAINT: name=launcher mapped=true buffer=640x420 paints=1 last_ms=5.834
AETHER_SHELL_LAYER_HOST_READY: surfaces=5 configured=5 mapped=3 painted=3
AETHER_SHELL_LAYER_RUN: paints=1 commits=3 unmaps=0 keys=0 elapsed_ms=815 paint_ms=40.284
```

The per-surface PNGs (`--frames-out`) decode to the design's tokens:

| Surface | Measured pixels |
| --- | --- |
| `layer-menubar.png` | 1920×28; dominant `(12,14,18)` = `Tokens.background` #0E1116 at 86 % alpha premultiplied; accent-family pixels (blue > 230, blue > red+60) only in the left 40 px (the logo gradient from `Tokens.accent` #4C8DFF) |
| `layer-dock.png` | 720×64; 7 440 px of `(21,25,31)` = `Tokens.surface` #1A1F27 at 82 % alpha; corners `(0,0,0)` (transparent); 7 198 px of `(30,36,45)` app tiles; >100 bright pixels for icons/labels; no panel pixel at x < 90 (the 270 px panel is centred) |
| `layer-launcher.png` (`--act launcher`) | 640×420; >2000 tinted pixels of `(24,29,37)` = `Tokens.surface` @94 %; transparent corner |

## 5. Test results

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell -p aether-ui
# 184 passed, 0 failed   (179 before this round: +1 handshake test for an
#                         opened panel, +1 layer_pixels test, +1 paint unit
#                         test, +1 wayland panel unit test, +1 layer text-scale
#                         test; the pixel assertions extend the existing
#                         handshake test)
LIBRARY_PATH=/opt/homebrew/lib cargo check --workspace
# clean (no errors, no new warnings in the shell or the design system)
```

New/extended tests:

* `the_shell_creates_every_layer_surface_on_a_real_compositor` — also asserts
  the two panels are mapped and the three closed ones are not, the PNG pixels
  above, and that the compositor's placement and frame markers match.
* `an_opened_panel_is_mapped_painted_and_placed` — `--act launcher`: the
  overlay surface is mapped, painted at 640×420, tinted, transparent at the
  corner, and placed at `640,330` by the compositor.
* `frames_repaint_on_change_and_respect_the_motion_settings` — no repaint when
  idle, repaint + changed pixels on a text-scale change, tier reaches the
  components, reduce-motion moves the frame clock from 16 ms to 100 ms, and
  `write_surface(…, scale=2)` produces a doubled buffer.
* `only_open_panels_are_mapped`, `panel_geometry_follows_the_text_scale`,
  `surfaces_are_placed_where_the_anchors_say` — the pure rules.

## 6. Measured frame timings

`cargo test -p aether-shell --release --test layer_pixels -- --nocapture`
(Mac Studio-class host, single-threaded, macOS 15, no GPU work — the software
renderer only). "Canvas" is the whole composed scene; the per-surface numbers
are the crop + ARGB packing the host then writes to the SHM file.

| Size | Canvas (median) | Canvas (max) | menubar | dock | launcher | notifications | control centre |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1920×1080 | **0.32 ms** | 0.56 ms | 0.019 ms | 0.009 ms | 0.158 ms | 0.316 ms | 0.049 ms |
| 1280×800 | **0.27 ms** | 0.45 ms | 0.007 ms | 0.007 ms | 0.085 ms | 0.163 ms | 0.035 ms |

Debug build (what `cargo test` builds) for comparison:

| Size | Canvas (median) | menubar | dock | launcher | notifications | control centre |
| --- | --- | --- | --- | --- | --- | --- |
| 1920×1080 | 10.0 ms | 1.14 ms | 0.99 ms | 5.7 ms | 8.2 ms | 7.5 ms |
| 1280×800 | 7.3 ms | 0.75 ms | 0.98 ms | 5.6 ms | 6.1 ms | 5.5 ms |

First frame of a real `--connect` run (release) — font/layout caches cold:

```
AETHER_SHELL_LAYER_PAINT: name=menubar ... last_ms=0.015
AETHER_SHELL_LAYER_PAINT: name=dock ... last_ms=0.019
AETHER_SHELL_LAYER_PAINT: name=launcher ... last_ms=0.233
AETHER_SHELL_LAYER_RUN: paints=1 commits=3 unmaps=0 keys=0 elapsed_ms=1013 paint_ms=11.187
```

So a steady frame costs ~0.3 ms of rendering plus ~0.4 ms of copying for the two
always-visible panels: the 16.7 ms budget is not the constraint on this class of
hardware; the debug build (10 ms canvas) is within budget but has little headroom,
which is why the image ships release binaries.

## 7. What is still hardware-gated

1. **GPU rasterisation / vsync pacing.** Everything above is the software
   renderer writing into SHM. The host ticks at 16 ms (100 ms with
   `reduce-motion`) but does not pace to the display: `wl_surface.frame`
   callbacks are available and unused. On the DRM backend the frame clock should
   be driven by `frame` callbacks (and the compositor's `AETHER_COMPOSITOR_FRAME`
   markers) instead of `sleep(16ms)`.
2. **Scanout pixel readback.** The headless backend has no rasteriser, so the
   pixel assertions are on the exact SHM bytes the shell attaches (what the
   compositor composites), cross-checked with the compositor's own placement and
   damage markers. Verifying the actual scanout buffer needs a DRM dump, which
   does not exist yet.
3. **Crisp HiDPI.** The scene is laid out at logical size and upscaled with
   nearest-neighbour sampling when the output scale is 2, because the software
   renderer's `MinimalSoftwareWindow` cannot be told a scale factor (the setter
   is crate-private in `i-slint-core` 1.19). Layout, damage and buffer scale are
   correct; text is logical-resolution. A real fix needs a renderer that lays
   out at the device scale (the winit/femtovg path) or a patched window adapter.
   The headless layout puts every surface on the 1× primary output, so this path
   is unit-tested (`scale=2` → 2560×56 buffer) but never exercised end to end.
4. **Buffer back-pressure.** One buffer per surface is reused every frame; the
   compositor does not send `wl_buffer.release` yet (no `send_frames_surface_tree`
   in the headless frame path), so double buffering would gain nothing today. On
   the DRM backend the two should land together: release-driven double buffering
   plus `frame`-callback pacing.
5. **Pointer input.** `wl_pointer` is not bound, so hover states and click
   handling do not reach the shell over the socket. The redraw path needs no
   change when they do: any Slint dirty region repaints the affected surfaces.
6. **Full-screen overlays.** Settings, Overview, the lock screen and the
   not-you-responding dialog have no layer surface of their own yet, so
   `layer-host` mode does not paint them (they would otherwise leak into the
   chrome crops). They need two or three more surfaces (one full-output
   `Overlay`-layer surface for Settings/Overview, one for the lock screen).
7. **Documentation drift (other territory).** The acceptance harness and the
   features file still quote the old numbers (`notifications 360×1080`,
   `control centre 340×1080`, `launcher 640×480` at the 12 px inset). The new
   sizes are 360/340 × 1052 (menu-bar margin) and 640×420. Those files are not
   in this session's territory; the owner of
   `docs/2026/11/26/results/on-device-acceptance-harness.md` and
   `docs/2026/11/16/os-project/features/01-desktop-shell.md` should refresh
   them.
