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
| `compositor/src/glass_shader.rs` | Pure mapping: `EffectPlan` → shader stages + uniform values; damage-region clipping; physical-pixel material scaling. Compiled on every host, unit-tested headlessly |
| `compositor/src/backend/glass_gles.rs` | GL execution on the `drm` feature path: program compilation, backdrop capture, blur chain with pooled offscreen targets, frost element (`DrmRenderElement`) |
| `compositor/src/backend/drm.rs` | Wires the chain into `present`: backdrop capture from Background/Bottom layer surfaces, frost under windows and Top/Overlay chrome, `AETHER_COMPOSITOR_GLASS_SHADER` / `_GLASS_STATS` markers, counters in `DrmBackend` |
| `compositor/scripts/check-shaders.sh` | GPU-free GLSL compile/link check for the three fragment stages |
| `compositor/README.md` | "Glass shader passes (P3-T05)" section: tier table, shader files, uniforms, damage-scope residual, hardware checklist |

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
# lib: 64 passed (54 before this task, +10 glass_shader tests)
# integration: p3_t02 1, p3_t03 1, p3_t05_t06 1, p3_t07 1, p3_t08 1, p3_t09 9 — all ok

./compositor/scripts/check-linux-target.sh
# Finished `dev` profile [unoptimized + debuginfo] — DRM/GLES code type-checks
# for x86_64-unknown-linux-gnu with --features drm

./compositor/scripts/check-shaders.sh
# texture.vert (smithay) OK, glass_downsample.frag OK, glass_upsample.frag OK,
# glass_composite.frag OK, link OK ×3 → "RESULT: all glass shaders compile"
```

The shader check compiles the three fragment stages against the smithay texture
vertex stage with the macOS OpenGL framework (desktop GLSL 1.20, ES-only
`precision` qualifiers stripped) and links them. It proves syntax, declarations
and types — not driver behaviour.

## Hardware-gated (NOT claimed by this task)

No GPU or DRM device is available on this host, so the chain was never executed.
On the Dell image, validate:

1. `AETHER_COMPOSITOR_GLASS_SHADER: programs=3 …` at start-up (programs compiled
   through the i915 EGL/GLES context). Any failure is non-fatal and reported as
   `glass shaders unavailable: …`.
2. `AETHER_COMPOSITOR_GLASS_SHADER: tier=… frosted=<n> … backdrop=<WxH>` per
   frame and `AETHER_COMPOSITOR_GLASS_STATS: surfaces=… failures=…` at exit:
   `failures` must stay 0.
3. Visual: blur radius, 18 % tint, 1 px rim (brighter on the top edge) and the
   Full-tier refraction band on a translucent shell panel; the Minimal tier
   shows a solid panel with no backdrop.
4. `AETHER_COMPOSITOR_EFFECT_TIER_SWITCH` Full → Reduced → Minimal produces the
   expected 10 → 8 → 0 blur stages and no stale frost.
5. 60 fps with three windows at Full tier (P3-T05 acceptance) — now measurable
   with real passes instead of only the modelled cost.

Known residual: the frost covers the whole surface, not just the damaged strip,
because `DrmCompositor::render_frame` renders every element into a fresh
swapchain slot (no buffer-age tracking yet). Damage still gates whether a
surface is frosted; strip-scoped blur belongs to the P3-T06 follow-up.
