# D6 — Shell wallpaper layer surface + pointer over the socket

- Date: 2026-11-28 (IST, overnight run, fifth shell round)
- Session: `shell` (territory `shell/**`, `ui/**`, this file)
- Status: **Done.** The shell now owns the desktop background, and a click that
  the compositor injects reaches the shell's dock handler end to end.

## 1. The background layer surface

Nothing was on the `Background` layer, so the first boot showed the chrome over
whatever the compositor clears to. The plan table now starts with:

| Field | Value |
| --- | --- |
| name / namespace | `background` / `aether-shell-background` |
| layer | `zwlr_layer_shell_v1` **Background** (0) |
| anchors | top + bottom + left + right (bitfield 15) |
| size | `0 × 0` (fills the output) → configured **1920×1080** on the headless output |
| exclusive zone | **−1** ("ignore my exclusive zone"): it covers the whole output and reserves nothing, so the menu bar's 28 px still belong to windows |
| keyboard | `none` |
| margin | none |
| created | **first**, so it sits under every other surface |
| visibility | always mapped (it is the desktop), painted once and left alone |

It is part of the surface contract now: `required_namespaces()` includes it, so
`--host-plan` lists it first and `--host-check` fails if a compositor journal has
no `aether-shell-background`, and `--frames-out` writes `layer-background.png`.

### Geometry and why it is painted procedurally

The wallpaper is the design's `Wallpaper` gradient (155°, `#0B1220` → `#0E1116` →
`#101725`) with its two glows (`#1C3D7A` at 33 %, `#6C4CE6` at 20 %). It is
rendered by `LayerPainter::write_wallpaper()` rather than by keeping the Slint
`Wallpaper` node in the layer-host scene, because the scene is cropped into all
six surfaces: with the wallpaper in it, every panel surface would be opaque and
would cover the windows behind a floating dock. The chrome surfaces stay
transparent outside their panels and the background surface carries the wallpaper.

Sampled from `layer-background.png` (1920×1080):

| position | pixel | reading |
| --- | --- | --- |
| 0,0 | (11, 18, 32) | `#0B1220` side of the gradient |
| 400,540 | (16, 24, 39) | first glow lifting the middle-left |
| 1400,540 | (19, 21, 36) | second (purple) glow |
| 1919,1079 | (16, 23, 37) | `#101725` side |

The frame is opaque: `write_wallpaper` writes `A = 255` for every pixel and the
unit test asserts it on the exact bytes; the PNG shows no `(0,0,0)` hole, and its
pixels are not the menu-bar tint `(12,14,18)`.

Cost (one-time, the wallpaper never changes with shell state): **8.8 ms** at
1920×1080 in release (245 ms in the unoptimised debug test build), 7.7 ms at
1280×800 release. The chrome canvas stays at 0.34 ms per frame.

## 2. Pointer over the socket

The compositor's hook landed (`compositor/src/lib.rs`,
`AETHER_COMPOSITOR_INJECT_POINTER="x=…,y=…[,button=…]"`), so the dock-click proof
is now a real socket test. Two notes on the hook's actual shape:

* it logs `AETHER_COMPOSITOR_POINTER_INJECTION: backend=headless x=… y=… button=…`
  (config) and `AETHER_COMPOSITOR_POINTER: surface=… x=… y=… event=enter|motion|button`
  (per event) rather than a single `…_POINTER_INJECTED` line;
* it forwards the `button` value verbatim, so `button=1` reaches the client as
  `wl_pointer.button = 1`, not the protocol's `BTN_LEFT` (0x110 = 272). The shell
  accepts both spellings (1/2/3 = left/middle/right is the X11 convention the hook
  documents), and the test asserts "a left-click code" rather than pinning one.

Transcript — compositor with `AETHER_COMPOSITOR_INJECT_POINTER="x=861,y=1048,button=1"`,
shell `aether-shell --connect --hold-ms 1200`:

```
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=none state=enter window=861,1048
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=1 state=press window=861,1048
AETHER_SHELL_POINTER: surface=aether-shell-dock x=261 y=32 button=1 state=release window=861,1048
AETHER_SHELL_ACTIVATE: surface=dock item=aether-files
AETHER_SHELL_LAYER_HOST: display=wayland-0 outputs=2 output=1920x1080 scale=1 seat=keyboard+pointer
AETHER_SHELL_LAYER_RUN: paints=2 commits=5 unmaps=0 keys=0 pointers=3 focus=none elapsed_ms=1220
```

```
AETHER_COMPOSITOR_POINTER_INJECTION: backend=headless x=861 y=1048 button=1
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=261 y=32 event=enter
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=261 y=32 event=button
AETHER_COMPOSITOR_POINTER: surface=aether-shell-dock x=261 y=32 event=button
```

Reading it: output (861,1048) is surface-local (261,32) on the dock surface
(placed at 600,1016) — the centre of the first pinned tile; the shell adds the
placement, dispatches Slint pointer events at window (861,1048), the dock tile's
`clicked` handler runs `dock-chosen`, and `AETHER_SHELL_ACTIVATE: surface=dock
item=aether-files` records which handler ran. `pointers=3` counts enter, press and
release; `paints=2` is the first frame plus the repaint the activation caused.

`AETHER_SHELL_ACTIVATE` moved from `activate_focus()` into the leaf handlers
(`activate_menubar`, `dock_chosen`, `accept_search`, `activate_control_center`,
`activate_notification`), so a click and a key press over the same item produce
the same marker: `AETHER_SHELL_ACTIVATE: surface=<s> item=<id>`.

## 3. Files changed

| File | Change |
| --- | --- |
| `shell/src/layers.rs` | `background` spec first in the table (Background layer, anchors 15, size 0×0, exclusive zone −1, keyboard none); `required_namespaces()` gains `aether-shell-background`; unit test for the geometry and for "created first". |
| `shell/src/paint.rs` | `write_wallpaper()` (premultiplied ARGB, opaque, at canvas size × scale), `wallpaper_pixel()`, the gradient/glow helpers, a unit test for opacity/variance/≠ panel tint, and the background in the placement test. |
| `shell/src/wayland.rs` | Background layer painted from `write_wallpaper()` and committed once; `background` is always visible; the run/seat markers already carried `pointers=` and `focus=`. |
| `shell/src/app.rs` | `note_activation()` and one `AETHER_SHELL_ACTIVATE` per leaf handler (keyboard and pointer share it). |
| `shell/tests/compositor_handshake.rs` | Background assertions (CREATED/CONFIGURE/LAYER_SURFACE/LAYER_CONFIGURE/placement), `layer-background.png` opaque + gradient + ≠ tint, `mapped=3 painted=3` on a fresh session, and the new **injected dock click** socket test. |
| `shell/tests/layer_pixels.rs` | Wallpaper opacity/alpha assertions and its timing at both sizes; the timing loop skips the background for `write_surface`. |

## 4. Verification

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell -p aether-ui
# 192 passed, 0 failed   (189 before: +1 layers background, +1 paint wallpaper,
#                         +1 injected dock click)
LIBRARY_PATH=/opt/homebrew/lib cargo check --workspace
# clean (no errors, no new warnings in the shell or the design system)

aether-shell --host-plan            # first line: background ... layer=Background anchors=15 exclusive_zone=-1
aether-shell --host-check <journal> # LAYER_SURFACE name=background ... configured=1920x1080 ok=true ... HOST_OK
```

The real-journal check passes with the new surface:

```
LAYER_HOST output=aether-headless-0 logical=1920x1080
LAYER_SURFACE name=background namespace=aether-shell-background bound=true configured=1920x1080 expected=1920x1080 ok=true
LAYER_SURFACE name=menubar ... ok=true
...
AETHER_SHELL_LAYER_HOST_OK
```

Console for the connect run:

```
AETHER_SHELL_LAYER_CREATED: name=background namespace=aether-shell-background layer=Background anchors=15 size=1920x1080 exclusive_zone=-1 margin_top=0 keyboard=0
AETHER_SHELL_LAYER_CONFIGURE: name=background namespace=aether-shell-background configured=1920x1080 serials=2 placement=0,0 scale=1
AETHER_SHELL_LAYER_PAINT: name=background mapped=true buffer=1920x1080 paints=1
AETHER_SHELL_LAYER_HOST_READY: surfaces=6 configured=6 mapped=3 painted=3
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-background placement=0,0 local=0,0,1920x1080 output=0,0,1920x1080
```

## 5. What is still hardware-gated

1. **Panel transparency now needs the wallpaper to be ours.** The chrome surfaces
   are transparent outside their panels and the desktop visible through them is
   the shell's own background surface. If a future round adds a real wallpaper
   setting (image, per-workspace), it must repaint the background surface — the
   host currently commits it once (`layer == 0` short-circuit) and would need a
   dirty flag.
2. **HiDPI wallpaper.** At output scale 2 the wallpaper is generated at
   `logical × 2` (the same code path), but a 3840×2160 gradient costs ~4× the
   release time (~35 ms) once; the device still has no HiDPI surface to exercise
   it end to end.
3. **Per-frame cost of the background.** Zero today (painted once, never
   recommitted). A wallpaper animation or a settings-driven tint must go through
   the same dirty-region logic as the chrome.
4. **Pointer on the device.** The socket path is proven headless; real libinput →
   `wl_pointer` → focus on the surface under the cursor, cursor image and drag
   (`wl_pointer.motion` between press and release) still need the DRM seat.
   Dragging is implemented only as far as Slint's own press/release hit-testing
   allows; there is no grab semantics yet.
5. **Button codes.** The injection hook sends the raw value (`button=1`), so the
   shell accepts both the X11 convention and `BTN_*`. When the DRM backend sends
   real libinput codes only the `BTN_*` path is used; the 1/2/3 branch is for test
   hooks and stays harmless.
6. **Overlay surfaces still missing.** Settings, Overview, the lock screen and the
   not-responding dialog have no layer surface, so they are not painted in
   layer-host mode (unchanged from the previous round). The background surface
   makes the gap less visible (the desktop is no longer a void) but does not
   close it.
7. **Doc drift** (other territory): the acceptance harness and features docs still
   quote the pre-background surface list and the 360×1080/640×480 panel sizes.
