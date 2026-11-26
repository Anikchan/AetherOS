# P3-T05 — GLES glass shader passes on the DRM renderer (D6 evidence)

- Date: 2026-11-27
- Session: S2 (`session - 2 (Nov27)`)
- Territory: `compositor/`, `docs/.../results/`
- Task: implement the GLES glass shader passes for the DRM renderer
  (dual-Kawase blur Full/Reduced, tint, edge highlight, Minimal fallback),
  consuming the existing `glass::EffectPlan` and damage region; unit-test the
  uniform/plan mapping and the tier → pass-list selection; document the shader
  files and the hardware-gated validation.

## What changed

| File | Purpose |
|---|---|
| `compositor/src/shaders/glass_downsample.frag` | Dual-Kawase 5-tap downsample pass (GLSL ES 1.00) |
| `compositor/src/shaders/glass_upsample.frag` | Dual-Kawase 8-tap tent upsample pass |
| `compositor/src/shaders/glass_composite.frag` | Fused saturation + tint + edge refraction + rim highlight + corner mask + Minimal `fill` |
| `compositor/src/glass_shader.rs` | Pure mapping: `EffectPlan` → shader stages + uniform values; damage-region clipping; physical-pixel material scaling; **`frost_region` / `frost_region_scaled` / `halo_px`** (damage-scoped frost region). Compiled on every host, unit-tested headlessly |
| `compositor/src/backend/glass_gles.rs` | GL execution on the `drm` feature path: program compilation, backdrop capture, per-surface blur chain over the **damage-scoped region**, stable-id frost element (`DrmRenderElement`), pixel-area counters |
| `compositor/src/backend/drm.rs` | Wires the chain into `present`: backdrop capture from Background/Bottom layer surfaces, frost under windows and Top/Overlay chrome over the damaged strip, `AETHER_COMPOSITOR_GLASS_SHADER` / `_GLASS_STATS` markers, `AETHER_COMPOSITOR_RUN_SECS` run bound |
| `compositor/scripts/check-shaders.sh` | GPU-free GLSL compile/link check for the three fragment stages |
| `compositor/scripts/kms-checklist.sh` | **New.** On-device DRM run + marker grading (EGL context, modeset, page flips, input, seat, glass programs, tier/blur, `failures=0`, damage-scoped frost); `--grade` and `--selftest` modes |
| `compositor/scripts/fixtures/kms-{full,minimal}-tier.log` | **New.** Dell-style marker logs the checklist self-test scores without a device |
| `compositor/README.md` | "Glass shader passes (P3-T05)" section: tier table, shader files, uniforms, damage-scope design, on-device checklist, hardware residual |

## Tier → chain (as executed by the DRM path)

| Tier | Blur stages | Blur resolution | Composite |
|---|---|---|---|
| Full | 10 (5 down + 5 up) | full, radius 24 px | saturation 1.2, tint 18 %, refraction 24 px band, rim 40 % (+20 % top) |
| Reduced | 8 (4 down + 4 up) | half, radius 16 px | saturation 1.1, tint 18 %, no refraction, rim 30 % |
| Minimal | 0 | — | `fill` mode, solid 92 % panel, rim 20 % |

Uniform/plan mapping asserted by unit tests (`glass_shader::tests`):

- `full_tier_runs_the_blur_chain_then_the_composite` — 5 down + 5 up + composite, back-to-front.
- `down_levels_halve_and_up_levels_grow_back_to_the_region` — level sizes for a 512×256 damage region.
- `reduced_tier_blurs_at_half_resolution_and_drops_refraction` — first level is 256×128, `refraction = 0`.
- `minimal_tier_is_a_single_filled_composite` — one stage, `fill = 1`, solid 92 % tint.
- `composite_uniforms_carry_the_material_values` — every uniform name/value, and name order matches the compiled program registration.
- `kawase_steps_are_expressed_in_source_texels` — per-level `sample_step`.
- `damage_region_clips_and_unions` — frame damage clipped to the surface and unioned.
- `shader_sources_fit_the_smithay_texture_program_contract` — no `#version`, `//_DEFINES_` present, `tex`/`alpha`/`v_coords` declared, all registered uniforms declared.
- `physical_material_scales_only_the_pixel_values` — HiDPI scaling of rim width and corner radius.
- `stage_plan_shrinks_with_the_tier` — 11 / 9 / 1 stages for Full / Reduced / Minimal.

## Commands and results (2026-11-27)

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor
# lib: 69 passed (64 before this follow-up, +5 frost_region / halo_px tests)
# integration: p3_t02 1, p3_t03 1, p3_t05_t06 1, p3_t07 1, p3_t08 1, p3_t09 9 — all ok

./compositor/scripts/check-linux-target.sh
# Finished `dev` profile [unoptimized + debuginfo] — DRM/GLES code type-checks
# for x86_64-unknown-linux-gnu with --features drm

./compositor/scripts/check-shaders.sh
# texture.vert (smithay) OK, glass_downsample.frag OK, glass_upsample.frag OK,
# glass_composite.frag OK, link OK ×3 → "RESULT: all glass shaders compile"

./compositor/scripts/kms-checklist.sh --selftest
# selftest OK   : full-tier fixture scored PASS
# selftest OK   : minimal-tier fixture scored PASS
# selftest OK   : broken fixture scored FAIL (failures=2, no VBlank)
# SELFTEST: PASSED
```

The shader check compiles the three fragment stages against the smithay texture
vertex stage with the macOS OpenGL framework (desktop GLSL 1.20, ES-only
`precision` qualifiers stripped) and links them. It proves syntax, declarations
and types — not driver behaviour.

## Damage-scoped frost (follow-up, 2026-11-27)

**The residual is closed in code.** The earlier note ("the frost covers the whole
surface because `DrmCompositor::render_frame` renders every element into a fresh
swapchain slot") described the symptom correctly but named the wrong mechanism:
smithay's `DrmCompositor` *does* track buffer age. `render_frame` reads the
acquired slot's age (`slot.age()`) and hands it to
`OutputDamageTracker::render_output`, which repaints the union of
(a) each element's own damage and (b) the damage that slot still holds from
`age` frames ago. What kept the frost whole-surface was in our element:

- the frost was built with `TextureRenderElement::from_static_texture`, whose
  `DamageSnapshot::empty()` makes `damage_since()` report the **whole texture**,
  i.e. the whole surface, every frame;
- the chain ran at surface size (`logical_region = logical_surface`), so the
  texture really was the whole surface, and the blur cost the whole surface.

Both are fixed:

| Piece | Change |
|---|---|
| `glass_shader::frost_region(surface, damage, halo)` | Pure: damage clipped to the surface, grown by `halo`, `None` when no damage reaches it, whole surface above `FULL_FROST_RATIO` (0.5) |
| `glass_shader::frost_region_scaled(surface, damage, scale, halo)` | Same at an output scale (logical → physical surface px) |
| `glass_shader::halo_px(material)` | Blur radius + refraction band: Full 48 px, Reduced 16 px, Minimal 0 |
| `glass_gles::GlassPasses::frost` | Runs the chain at that region (stage sizes, uniforms and backdrop source rect all follow it) and returns a `Frost` covering only it |
| `glass_gles::GlassPasses::frost_id` | One stable `Id` per `wl_surface` (pruned per frame), so the tracker diffs instances instead of seeing a new element each frame |
| `glass_gles::GlassPasses` pool | Targets return to the pool at the start of the next frame (`pending`), so a moving strip does not churn textures |
| `lib.rs::complete_frame` | `AETHER_COMPOSITOR_FRAME` gains `frost_px` (same pure function the DRM path uses) |
| `drm.rs::present` | `AETHER_COMPOSITOR_GLASS_SHADER` gains `frost_px` / `surface_px`; `_GLASS_STATS` accumulates `frosted_px` / `surface_px` |

### Measured (headless, real binary, three 1152×648 windows, 320×100 damage each)

```
AETHER_COMPOSITOR_FRAME: n=3 … damage_px=32000 frost_px=163392 windows=3 tier=full est_ms=0.176
AETHER_COMPOSITOR_GLASS_PLAN: tier=full … damaged_px=2239488 covered_px=2239488
AETHER_COMPOSITOR_FRAME: n=3 … damage_px=32000 frost_px=96000 windows=3 tier=minimal
```

| Tier | Damage | Frost (scoped) | Whole-surface frost | Ratio |
|---|---|---|---|---|
| Full (halo 48 px) | 32 000 px | 163 392 px | 2 239 488 px | **7.3 %** |
| Minimal (halo 0) | 32 000 px | 96 000 px (= exactly 3 × damage) | 2 239 488 px | **4.3 %** |

The integration test asserts both rows (`frost_px * 2 < covered_px`, and the
Minimal run's `frost_px == damage_px × 3`), so the scoping is checked end-to-end
through the binary, not only by unit tests.

### New / updated tests

| Test | Asserts |
|---|---|
| `glass_shader::tests::halo_covers_the_blur_radius_and_the_refraction_band` | 48 / 16 / 0 px for Full / Reduced / Minimal |
| `glass_shader::tests::frost_region_is_the_damaged_strip_plus_the_halo` | 320×100 strip → 416×196 region, 10.9 % of a 1152×648 surface |
| `glass_shader::tests::frost_region_halo_is_clipped_to_the_surface` | Corner damage grows only inwards |
| `glass_shader::tests::frost_region_falls_back_to_the_whole_surface` | Empty damage → whole; off-surface damage → `None`; ≥ 50 % → whole |
| `glass_shader::tests::frost_region_scaled_converts_to_physical_pixels` | 1× vs 2× output scale |
| `tests/p3_t05_t06_effects_frames.rs` | `frost_px` present, ≥ damage, < half the covered pixels; Minimal exactly `damage × 3` |

## Hardware-gated (NOT claimed by this task)

No GPU or DRM device is available on this host, so the chain was never executed.
On the Dell image, validate:

1. `AETHER_COMPOSITOR_GLASS_SHADER: programs=3 …` at start-up (programs compiled
   through the i915 EGL/GLES context). Any failure is non-fatal and reported as
   `glass shaders unavailable: …`.
2. `AETHER_COMPOSITOR_GLASS_SHADER: tier=… frosted=<n> … frost_px=<n>
   surface_px=<n>` per frame and `AETHER_COMPOSITOR_GLASS_STATS: … failures=0` at
   exit: `failures` must stay 0 and `frosted_px` must stay below `surface_px`.
3. Visual: blur radius, 18 % tint, 1 px rim (brighter on the top edge) and the
   Full-tier refraction band on a translucent shell panel; the Minimal tier
   shows a solid panel with no backdrop.
4. `AETHER_COMPOSITOR_EFFECT_TIER_SWITCH` Full → Reduced → Minimal produces the
   expected 10 → 8 → 0 blur stages and no stale frost.
5. 60 fps with three windows at Full tier (P3-T05 acceptance) — now measurable
   with real passes instead of only the modelled cost.
6. No visible seam at a strip boundary during a window drag (the 48 px halo is
   the mitigation; it is a visual check, not asserted by any test).

Run all of it with:

```sh
./compositor/scripts/kms-checklist.sh --duration 60 --client 'foot'
./compositor/scripts/kms-checklist.sh --grade kms-checklist-*/primary-full.log
```

The script bounds the run with `AETHER_COMPOSITOR_RUN_SECS` so the exit markers
are printed, then grades: backend=DRM, libseat session, primary GPU, device
open, EGL/GLES context, modeset, page flips, libinput + seat, key/button
markers (WARN without a keypress), 3 glass programs, effect tier + blur,
`failures=0`, damage-scoped frost, frame stats, clean shutdown. `--selftest`
scores `scripts/fixtures/*.log` on any host, and a mutated fixture (failures=2,
no VBlank) must score FAIL — so the grader itself is verified.

**Residual (unchanged by this follow-up):** the backdrop capture still renders
the whole union of glass surfaces every frame (wallpaper pass), and layer-surface
damage is recorded from the surface origin (no placement yet). Both are noted in
`compositor/README.md`.

## Next

1. Dell run of `scripts/kms-checklist.sh` (hardware testing paused on
   2026-11-26) → attach the logs as D6 evidence and set `GpuProfile::measured`.
2. Scope the backdrop capture by damage (it is the largest remaining full-screen
   pass per frame).
3. Place layer surfaces before recording their damage, so panel/dock damage is
   translated instead of recorded from the surface origin.
