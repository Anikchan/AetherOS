# D6 — Compositor residuals: layer-surface damage, stale strips, HiDPI spaces

- Date: 2026-11-28 (session continued from 2026-11-27)
- Scope: `compositor/**` only (P3-T05/T06 follow-ups to P3-T05 glass, P3-T06
  damage scope, P3-T09 coordinate spaces)
- Host: macOS development host, headless backend only. **No Linux/DRM device was
  available**, so every KMS statement below is hardware-gated.
- Gates held: `cargo test -p aether-compositor` (78 lib + 15 integration),
  `compositor/scripts/check-linux-target.sh`, `compositor/scripts/check-shaders.sh`,
  `compositor/scripts/kms-checklist.sh --selftest` → `SELFTEST: PASSED`.

## 1. Layer-surface damage is now translated by its placement

### The defect

`AetherCompositor::record_surface_damage` translates a managed toplevel's damage
by `window.rect`, but layer surfaces (`zwlr_layer_shell_v1`: menu bar, dock,
launcher, notifications, control centre) are deliberately *not* in the P3-T04
window manager. Their branch recorded the client's damage rect verbatim:

```rust
// before
None => {
    for rect in damage {
        self.frames.add_damage(*rect);   // surface-local, no placement
    }
}
```

That is only correct for chrome anchored at the output origin. A dock anchored to
the bottom edge (`anchor=bottom`, 720x64 on 1920x1080) is drawn at (600, 1016),
so its damage was queued at (8, 8) instead of (608, 1024): the dock's strip was
composited at the top of the output and the real dock area never repainted.
`damage_whole_surface` had the same class of bug in the opposite direction — it
damaged the *whole primary output* for any layer surface (a full-screen frame
for a 46 080 px dock).

### The mechanism

The compositor now resolves the placement once and uses it for both paths:

1. `AetherCompositor::layer_surface_rect(entry)` reads the surface's
   `LayerSurfaceCachedState`, computes the negotiated size with
   `layer_surface_size` (unchanged) and places it with
   `backend::layer_location(anchors, size, output_logical_size)` — the **same**
   function the DRM backend's `layer_rect` uses for drawing, so damage and
   rendering share one placement rule.
2. `layer_damage_rects(placement, size, damage)` (free function, pure) clips each
   surface-local rect to the surface and translates it by the placement,
   returning `(local, output)` pairs.
3. `record_layer_damage` feeds the output rects to `FrameScheduler::add_damage`
   and prints, once per placement change:

   ```text
   AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=<ns> placement=<x>,<y> local=<x>,<y>,<w>x<h> output=<x>,<y>,<w>x<h>
   ```

4. `damage_whole_surface` for a layer surface now damages the placed rect
   (`local = 0,0,w,h`), not the whole output.

Positions stay relative to the surface's own output, like the rest of the
single-output damage model in the frame scheduler.

Surfaces that are neither managed toplevels nor layer surfaces (xdg popups, the
cursor surface) now contribute no frame damage from their commits; the cursor has
its own `damage_pointer` path and popups are not composed by the element list, so
dropping them is correct — the old code mixed surface-local coordinates into
output space.

### Proof (headless, real protocol)

`compositor/tests/p3_t02_headless.rs` now also maps a **dock** (720x64, anchored
bottom, primary output) and commits an explicit `wl_surface.damage(8, 8, 100, 20)`.
Captured from the compositor's own log on the dev host:

```text
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-dock layer=Top output=aether-headless-0 scale=1
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-dock size=720x64
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-dock placement=600,1016 local=8,8,100x20 output=608,1024,100x20
```

and for the pre-existing top-anchored panel on the 1.5x monitor the translation
is the identity, as it should be:

```text
AETHER_COMPOSITOR_LAYER_DAMAGE: namespace=aether-shell-test placement=0,0 local=0,0,1920x32 output=0,0,1920x32
```

Unit tests (`compositor/src/lib.rs`, `mod layer_damage_tests`) pin the arithmetic
without a Wayland round-trip: `(dx, dy)` at placement `(x, y)` damages
`(x+dx, y+dy)`; damage is clipped to the surface; a zero placement is the
identity.

### Graded on-device check

`kms-checklist.sh` gained check 9: every `AETHER_COMPOSITOR_LAYER_DAMAGE` line
must satisfy `placement + local == output` (and matching widths). A run with no
layer damage (no shell chrome mapped) is a WARN, never a PASS.

`kms-checklist.sh --selftest` gained a mutation: the full-tier fixture with
`output=608,1024,100x20` rewritten to `output=8,8,100x20` (translation dropped)
must score FAIL. Result:

```text
selftest OK   : full-tier fixture scored PASS
selftest OK   : minimal-tier fixture scored PASS
selftest OK   : broken fixture scored FAIL (failures=2, no VBlank)
selftest OK   : whole-union capture fixture scored FAIL (backdrop_px == backdrop_union_px)
selftest OK   : untranslated layer damage scored FAIL (output 8,8 != placement 600,1016 + local 8,8)
SELFTEST: PASSED
```

and grading the untouched fixture reports:

```text
PASS  graded: layer-surface damage lands on the placed surface (1 marker(s), namespace=aether-shell-dock placement=600,1016 local=8,8,100x20 output=608,1024,100x20)
RESULT: 20 passed, 0 failed, 0 warnings
CHECKLIST: PASSED
```

The two KMS fixtures were extended with a representative dock
(`LAYER_SURFACE` / `LAYER_CONFIGURE` / `LAYER_DAMAGE`) so the grader's new case
has something to score on a host with no DRM device.

## 2. No stale damage-scoped backdrop strip

`compositor/tests/p3_t05_t06_effects_frames.rs` now drives **two consecutive
frames with different damage**: three 1152x648 windows each damage
`(0,0,320,100)` (frames 1–3 as the windows map), then — after the first frame
callbacks were answered — each window damages `(64,128,64,32)` and requests
another frame callback. The new test
`damage_scoped_backdrop_does_not_reuse_a_stale_strip` asserts, for every
presented 3-window frame:

- `backdrop_px >= frost_px` (the capture covers every frosted surface's frost
  region in *this* frame);
- `backdrop_union_px` is identical across frames (the baseline does not grow —
  the glass surface set is unchanged);
- `backdrop_px <= backdrop_union_px`;
- `backdrop_px == frost_px`: all three windows are fully on screen, so the
  captured strips must be exactly this frame's frost regions. A strip carried
  over from the previous, larger damage shows up here;
- the frost really followed the damage (`min(frost_px) < max(frost_px)`).

Measured (headless, tier=full, `AETHER_COMPOSITOR_FRAME`):

| frame | windows | damage_px | frost_px | backdrop_px | backdrop_union_px |
| --- | --- | --- | --- | --- | --- |
| 1 | 1 | 32 000 | 54 464 | 54 464 | 746 496 |
| 2 | 2 | 32 000 | 108 928 | 108 928 | 746 496 |
| 3 | 3 | 32 000 | 163 392 | 163 392 | 746 496 |
| 4 | 3 | 2 048 | 61 440 | 61 440 | 746 496 |

Frame 4 carries the small strip only; the capture dropped from 163 392 to
61 440 px and the union stayed at the two-surface baseline.

Mutation evidence (temporarily making the capture reuse the previous frame's
damage rects):

```text
assertion `left == right` failed: frame captured a stale strip: 163392 != 61440:
AETHER_COMPOSITOR_FRAME: n=4 ... damage_px=2048 frost_px=61440 backdrop_px=163392 backdrop_union_px=746496 windows=3 tier=full
```

so the test fails exactly when a stale strip is reused.

## 3. HiDPI review of the damage / capture / scanout spaces

Spaces reviewed after the earlier capture-bound fix:

| Space | Units | Producer | Consumers |
| --- | --- | --- | --- |
| Client damage | surface-local logical (`Damage::Buffer / buffer_scale`) | `CompositorHandler::commit` | window rect / layer placement translation |
| Frame damage | output logical, origin at the primary output | `FrameScheduler` | `frost_region_scaled`, `backdrop_regions`, DRM element list |
| Frost region | surface-local **physical** | `glass_shader::frost_region_scaled` | `GlassPasses::frost`, `frost_px` |
| Capture region | output **physical** | `glass_shader::backdrop_regions` (`physical_rect` origin + local) | `GlassPasses::backdrop`, `backdrop_px` |
| Scanout | output **physical** | `drm.rs` (`physical(x, y, scale)`) | KMS primary plane |

One real defect was found and fixed in the logical→physical conversion.
`frost_region_scaled` rounded the local origin and the local **width**
independently:

```text
scale 1.5, surface x=1000 w=3, damage x=1001 w=1
  local origin = round(1 * 1.5) = 2
  local width  = round(1 * 1.5) = 2     -> strip [2, 4)
  true damage  = round(1002*1.5) - round(1000*1.5) = 1 px -> strip [2, 3)
```

i.e. the strip reached one physical pixel past the damage. At the surface edge
the same rounding is clipped instead, losing a pixel column:

```text
scale 1.5, surface x=0 w=2 (physical 3), damage x=1 w=1
  before: [2, 4) clipped to 3  -> half of the 1 px footprint lost
  after:  [2, 3)               -> exact
```

Fix: `physical_damage(surface, clipped, scale)` rounds each **edge** once from
its output coordinate (`round(clipped.x*scale) - round(surface.x*scale)`), so the
strip covers exactly the texels the renderer draws and can no longer overhang or
be clipped. `physical_rect` (placement origin) and the DRM `physical()` helper
already round the same way, so capture, frost and scanout now agree.

New tests:

- `glass_shader::tests::frost_region_scaled_never_clips_at_fractional_pixel_edges`
  — at 1.5x and 2x, four surfaces including 1-logical-pixel damage at odd
  offsets: the strip's start/end equal the rounded damage edges, it stays inside
  the physical surface, and the frost region is exactly the physical damage
  footprint. **Fails on the old width-rounding** (verified by temporarily
  restoring it).
- `glass_shader::tests::hidpi_capture_is_not_clipped_and_covers_the_frost` —
  a window near the right/bottom edge of a 2880x1800 @1.5x output and of a
  1920x1080 @2x output: the capture equals the frost region translated to output
  space (the output bound must not cut it), the window fits the output, and the
  halo is clipped by the surface, not by the physical rounding.
- `frost_region_scaled_converts_to_physical_pixels` still passes unchanged
  (1x and 2x exact values), i.e. the fix is a no-op for aligned geometry.

## 4. Gates

| Gate | Command | Result |
| --- | --- | --- |
| Lib tests | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor --lib` | **78 passed** (was 73; +3 layer damage, +2 HiDPI) |
| Integration tests | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor` | **15 passed** (was 14; `p3_t05_t06` now 2: the original tier test + the stale-strip test) |
| Linux target type-check | `./compositor/scripts/check-linux-target.sh` | `Finished dev profile` (needed `rustup target add x86_64-unknown-linux-gnu`, which was missing on this host) |
| Shaders | `./compositor/scripts/check-shaders.sh` | `RESULT: all glass shaders compile` |
| KMS checklist grader | `./compositor/scripts/kms-checklist.sh --selftest` | `SELFTEST: PASSED` (5 cases) |
| KMS run on hardware | `./compositor/scripts/kms-checklist.sh` | **not run** — no Linux device with a toolchain; hardware-gated |

## 5. Remaining (hardware-gated)

1. **KMS run**: `./compositor/scripts/kms-checklist.sh --client 'foot'` on the
   Dell and attach the log. This is the first run that will exercise check 9 with
   a real dock (`AETHER_COMPOSITOR_LAYER_DAMAGE` from the shell's own layer
   surfaces) and confirm `placement + local == output` on the actual connector.
2. **1.5x panel on hardware**: the headless HiDPI evidence is the 2880x1800
   @1.5x `aether-headless-1` output; the Dell's internal panel runs at scale 1,
   so the fractional-scale path is only covered by the unit tests and the
   headless output. Verify with an external 1.5x/2x display or a forced
   `wp_fractional_scale_v1` client.
3. **Multi-output damage space**: layer placements are relative to their own
   output, while the frame scheduler's damage space is the primary output's
   logical origin (pre-existing single-output model). A dock on the *secondary*
   monitor therefore damages primary-output coordinates. Not reachable in the
   shell's current layout (all chrome is on the primary output) but worth
   resolving when multi-output composition lands.
4. **`frosted_px` at Minimal vs layer damage**: only WM windows are glass
   surfaces, so a layer-only frame reports `frost_px=0` (see the p3_t02 log
   above). Layer surfaces are composed by the DRM element list, not the frost
   chain, so this is expected — but a hardware run should confirm the dock is
   actually drawn (it is a plain element, not a frosted one).

## 6. Follow-up in this round

The keyboard half of the shell's layer-shell contract (focus for
`keyboard_interactivity = exclusive` and headless key injection for tests) landed
after this file: see `docs/2026/11/27/results/compositor-seat-focus-injection.md`.

