# P3-T05 / P3-T06 — Glass material pipeline, effect tiers, frame scheduling (D6 results)

- Date: 2026-11-26
- Session: S2 (compositor)
- Scope: P3-T05 (glass material pipeline + Full/Reduced/Minimal tier switch) and
  P3-T06 (frame scheduling, damage tracking, idle GPU measurement hooks)
- Host: macOS (Apple silicon) dev host for build + headless runs; Linux CI
  (`cargo test --workspace`, ubuntu-latest) runs the same tests
- Crate: `compositor/` (`aether-compositor`)
- Design source: `docs/2026/11/16/os-project/design/ui-design.md` §4 (glass material)

## 1. What changed

| File | Change |
| --- | --- |
| `compositor/src/glass.rs` | **New.** Glass material model per tier (blur/saturation/tint/edge highlight/edge refraction/shadow/corner radius), resolved pass list, damage-based effect plan, modelled frame-cost + 60 Hz budget check, automatic `TierPolicy` with hysteresis |
| `compositor/src/scheduling.rs` | **New.** `DamageRegion` (coalescing + exact union area), `FrameScheduler` (frame pacing at the target refresh, frame callbacks bookkeeping, idle time / idle ratio / skipped frames, gap stats) |
| `compositor/src/lib.rs` | `GlassPipeline` + `TierPolicy` + `FrameScheduler` in the compositor state; damage recorded from `wl_surface.commit` and from WM geometry changes; frame tick in the event loop; `wl_surface.frame` callbacks answered per presented frame; tier switch via `set_effect_tier` / `AETHER_COMPOSITOR_EFFECT_TIER`; new markers (see §3) |
| `compositor/tests/p3_t05_t06_effects_frames.rs` | **New.** Boots the real binary twice (Full and Minimal tier), maps three xdg-toplevels with damage and frame callbacks, asserts tier markers, the 3-window glass plan, damage-based repaint and frame callbacks |

No other crate, workflow or session's file is touched.

## 2. Design

### Tiers (design doc §4, implemented 1:1)

| Layer | Full | Reduced | Minimal |
| --- | --- | --- | --- |
| Backdrop blur | Dual Kawase, 5 down/up levels, radius 24 px, full res | Dual Kawase, 4 levels, radius 16 px, half res | none |
| Saturation | 1.2× | 1.1× | 1.0× |
| Tint | 18% white (light) / 18% black (dark) | same | solid 92% opacity |
| Edge highlight | 1 px, 40%, +20% on top edge | 1 px, 30%, +10% top | 1 px, 20% |
| Edge refraction | on | off | off |
| Shadow | 12 px, 12% | 8 px, 10% | 4 px, 10% |
| Corner radius | 18 px panel / 12 px menu / 8 px button | same | same |
| Resolved passes | 16 | 13 | 5 |

Blur is planned **only over the damaged region** (design rule: "the compositor
applies blur only to visible regions (damage-based)"), so a 320×100 damage strip
on a 1152×648 window costs a fraction of a full repaint.

### Tier switching

- Startup selection: `AETHER_COMPOSITOR_EFFECT_TIER=full|reduced|minimal`
  (default `full`; an unknown value falls back to `full` instead of failing).
- Runtime API: `AetherCompositor::set_effect_tier(tier, reason)` emits
  `AETHER_COMPOSITOR_EFFECT_TIER_SWITCH: from=… to=… reason=…`, re-prints the
  resolved pipeline and re-reports the cost.
- Automatic: `TierPolicy` drops one tier after 30 consecutive over-budget
  frames and raises one tier after 120 consecutive frames below 60% of the
  budget (asymmetric hysteresis, unit-tested). It currently observes the
  *modelled* frame cost; on hardware it will observe measured frame times.

### Frame scheduling and damage tracking (P3-T06)

- `wl_surface.commit` damage (surface and buffer coordinates, buffer-scaled) is
  drained once, translated to output coordinates and coalesced.
- A frame is presented only when there is damage **and** the frame interval
  (1/`AETHER_COMPOSITOR_TARGET_FPS`, default 60 Hz) elapsed. No damage → no
  frame submitted at all.
- `wl_surface.frame` callbacks are answered with the presented frame's
  timestamp, only for windows on the active workspace plus layer surfaces.
- Idle accounting: `idle_ms`, `idle_pct` (share of wall time with no frame) and
  `idle_skips`; this is the measurement hook the T06 gate ("idle GPU use
  measured") will read once real GPU counters exist.

## 3. Verification

### Commands and results (2026-11-26, macOS dev host)

```
$ LIBRARY_PATH=/opt/homebrew/lib cargo build -p aether-compositor
    Finished `dev` profile   (no warnings)

$ LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor
running 29 tests (lib: glass 14, scheduling 10, wm 5)
test result: ok. 29 passed; 0 failed
running 1 test   p3_t02_headless (two outputs, layer shell, xdg toplevel)   ok
running 1 test   p3_t03_xwayland (xeyes maps)                              ok
running 1 test   p3_t05_t06_effects_frames                                 ok
```

`LIBRARY_PATH` is the pre-existing local Homebrew `libxkbcommon` quirk on this
dev host, not a code requirement.

### Composite log from the acceptance run

Full tier, three windows (each 1152×648, the default floating geometry):

```
AETHER_COMPOSITOR_FRAME_RATE: 60
AETHER_COMPOSITOR_EFFECT_TIER: full
AETHER_COMPOSITOR_EFFECT_PIPELINE: tier=full blur=dual-kawase levels=5 res=1x radius=24 saturation=1.20 tint=0.18 edge=1px:0.40 refraction=on shadow=12px:0.12 corner=18
AETHER_COMPOSITOR_GLASS_GPU_PROFILE: name=reference-igpu-modelled measured=false blur_px_per_ms=3000000 refraction_px_per_ms=800000 composite_px_per_ms=20000000
AETHER_COMPOSITOR_FRAME: n=3 time_ms=126 damage_rects=1 damage_px=32000 windows=3 tier=full est_ms=0.176
AETHER_COMPOSITOR_GLASS_PLAN: tier=full … windows=3 passes=16 damaged_px=2239488 covered_px=2239488 est_ms=2.440 budget_ms=16.67 share=0.146 within_budget=true
AETHER_COMPOSITOR_FRAME_CALLBACKS: n=1 time_ms=126
AETHER_COMPOSITOR_IDLE: idle_ms=… idle_pct=… frames=3
```

Minimal tier, same three windows:

```
AETHER_COMPOSITOR_EFFECT_TIER: minimal
AETHER_COMPOSITOR_EFFECT_PIPELINE: tier=minimal blur=none saturation=1.00 tint=0.92 edge=1px:0.20 refraction=off shadow=4px:0.10 corner=18
AETHER_COMPOSITOR_GLASS_PLAN: tier=minimal … windows=3 passes=5 damaged_px=2239488 covered_px=2239488 est_ms=0.114 budget_ms=16.67 share=0.007 within_budget=true
```

(`AETHER_COMPOSITOR_TEST_LOG=1 cargo test -p aether-compositor --test p3_t05_t06_effects_frames -- --nocapture`
reproduces both logs.)

### Measured numbers

| Metric | Value | Where |
| --- | --- | --- |
| Full tier, 3 windows fully repainted, modelled GPU | 2.440 ms (14.6% of the 16.67 ms budget) | `AETHER_COMPOSITOR_GLASS_PLAN` |
| Full tier, damage-only frame (320×100 strip × 3 windows) | 0.176 ms | `AETHER_COMPOSITOR_FRAME` |
| Minimal tier, 3 windows fully repainted | 0.114 ms (0.7% of budget) | `AETHER_COMPOSITOR_GLASS_PLAN` |
| Passes: Full / Reduced / Minimal | 16 / 13 / 5 | unit test `pass_count_shrinks_with_the_tier` |
| Damage coalescing: two overlapping 100×100 rects | 1 rect, 150×150 = 22 500 px (not 20 000 or 40 000) | unit test `overlapping_damage_is_coalesced` |
| Frame pacing at 60 Hz | interval 16 666 µs, 5 ms after a frame → not due, 17 ms → due | unit test `frame_is_paced_to_the_target_interval` |
| Idle accounting | 100 ms idle span → `idle_ratio > 0.9`, closed by the next frame | unit test `damage_after_an_idle_span_closes_it` |

### Test coverage added

| Test | Asserts |
| --- | --- |
| `glass::tests::full_tier_matches_the_design_table` | Every Full-tier value from design doc §4 |
| `glass::tests::reduced_tier_blurs_at_half_resolution_without_refraction` / `minimal_tier_is_a_solid_panel` | Reduced/Minimal values |
| `glass::tests::pass_count_shrinks_with_the_tier` | 16 / 13 / 5 resolved passes |
| `glass::tests::blur_only_processes_the_damaged_region` | Damaged plan is < 25% of a full repaint |
| `glass::tests::damage_outside_the_surface_is_clipped` | Damage clipped to the surface |
| `glass::tests::three_windows_at_full_tier_fit_the_60fps_budget` | 3 × 1152×648 at Full within 16.67 ms, < 50% of budget |
| `glass::tests::tier_policy_drops_after_dwell_over_budget` / `…raises_only_after_a_long_comfortable_run` | Hysteresis in both directions |
| `scheduling::tests::*` (10) | Coalescing, union area, generation, pacing, idle ratio, one idle report per span, stats line |
| `tests/p3_t05_t06_effects_frames.rs` | Real binary at Full and Minimal: tier markers, pipeline contents differ, 3-window glass plan within budget with 16 vs 5 passes, damage < covered pixels, frame callbacks received by the client, idle_pct present |

## 4. Hardware gap (honest scope)

- **The 60 fps acceptance number is modelled, not measured.** `GpuProfile`
  throughput constants are `measured=false`; they exist to check that the
  Full-tier plan fits the frame budget analytically (2.44 ms of 16.67 ms).
  Real numbers need the Dell with the P2 graphics stack (Mesa/ANV + DRM/KMS)
  and a frame-time counter in the renderer.
- **No rendering yet.** The compositor is still headless: the pipeline is a
  resolved plan + cost model, not a GL/Vulkan pass. The pass list is the
  contract the renderer (P3-T07 direct scanout / P2 graphics work) will consume.
- **Damage for layer surfaces** is recorded from the surface origin because the
  headless compositor has no layer-shell placement yet; toplevel damage is
  translated with the managed window geometry.
- **Tier policy observes modelled cost** at the moment; wiring it to measured
  frame times is a one-line change once the renderer reports them.
- Idle GPU use is measured as "share of wall time with no frame submitted", not
  as GPU utilisation counters.

## 5. Next

1. P3-T07 direct scanout for full-screen windows (consumes this damage model).
2. P3-T08 no-hang design (Wayland ping, "Not responding", frame-gap logger —
   `FrameScheduler::last_gap_ms` already exposes the gaps it will log).
3. On the Dell: measure real Full-tier frame time with three windows and set
   `GpuProfile::measured = true` with the observed throughput.
