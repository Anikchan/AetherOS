# Session S2 progress — compositor (P3)

- Session: S2 (Compositor)
- Territory: `compositor/`, `spikes/`, `os-build/test/spike-*`, spike/coverage results
- Date: 2026-11-26
- Source plan: `docs/2026/11/22/parallel-plan-v2.md`, `docs/2026/11/26/session-kickoff.md`

## Task state

| Task | Status | Evidence |
| --- | --- | --- |
| P3-T01 | Done 2026-11-23 (previous session) | `docs/…/p3-t01` commit `0a4778e` |
| P3-T02 | **Done 2026-11-26 (S2)** — xdg-shell, layer shell, two monitors at different scales, HiDPI (fractional scale + buffer scale + xdg-output logical geometry), headless acceptance test | `docs/2026/11/26/results/p3-t02-multi-monitor-hidpi.md` |
| P3-T03 | **Blocked 2026-11-26** — no `Xwayland` binary on the macOS dev host, none installed in CI (`ci.yml` is S4's file), none in the image (`buildroot/` is S1's file). Needs `xwayland` + `x11-apps` in CI/image first. | Same results doc, section 5 |
| P3-T04 | **Done 2026-11-26** — floating/snap/tile/workspaces wired into toplevels + unit tests (commit `f6fe3dd`, CI run 45869439628) | `docs/2026/11/26/results/` |
| P3-T05 | **Done 2026-11-26 (S2)** — glass material pipeline, Full/Reduced/Minimal tiers, tier switch (`AETHER_COMPOSITOR_EFFECT_TIER` + `set_effect_tier` + automatic `TierPolicy`), 60 Hz budget check; **60 fps number is modelled, hardware gap noted** | `docs/2026/11/26/results/p3-t05-t06-glass-and-frame-scheduling.md` |
| P3-T06 | **Done 2026-11-26 (S2)** — damage tracking, 60 Hz frame pacing, `wl_surface.frame` callbacks, idle measurement hooks (`AETHER_COMPOSITOR_IDLE` / `FRAME_STATS`) | Same results doc |
| P3-T07..T08 | Not started | — |

## What was delivered for P3-T02

- `compositor/src/lib.rs`:
  - `HEADLESS_OUTPUTS`: `aether-headless-0` 1920×1080 @1.0 at (0,0) and
    `aether-headless-1` 2880×1800 @1.5 at logical (1920,0) → `wl_output.scale`
    1 and 2, `zxdg_output_v1` logical sizes 1920×1080 / 1920×1200.
  - `OutputManagerState::new_with_xdg_output`, per-output
    `wp_fractional_scale_v1` preferred scale, `wl_surface.enter` on output
    assignment, buffer-scale recording.
  - Layer-shell size negotiation from anchors/margins/requested size against the
    output's **logical** size (panel on the 1.5× monitor configures at
    1920×32).
  - Toplevel and layer-surface registries; `AETHER_COMPOSITOR_GRACE_MS`
    (default 3000) to keep the compositor alive for multi-phase tests.
- `compositor/tests/p3_t02_headless.rs`: spawns the real binary on a private
  `XDG_RUNTIME_DIR`, asserts the two-monitor layout, xdg toplevel map, layer
  surface configure, fractional scale 1.5, buffer scale 2, and the compositor
  markers. Runs in CI through the existing `cargo test --workspace` step (no
  workflow edit needed).

## Verification commands

```
LIBRARY_PATH=/opt/homebrew/lib cargo build -p aether-compositor      # ok
LIBRARY_PATH=/opt/homebrew/lib cargo test  -p aether-compositor      # 1 passed
```

`LIBRARY_PATH` is a local macOS/Homebrew linking quirk (libxkbcommon), not a
code requirement.

`cargo build --workspace` currently fails in `shell/` (`aether-greeter`: PAM/LocalCredentials,
format string) — that is session S3's in-flight work, unrelated to this crate;
`compositor/` compiles and tests green on its own.

## Next for S2

1. **P3-T07** direct scanout for full-screen apps (consumes the T06 damage
   model: a full-screen single-surface frame can bypass composition).
2. **P3-T08** no-hang design (Wayland ping, "Not responding", shell watchdog,
   frame-gap logger — `FrameScheduler::last_gap_ms` / `average_gap_ms` already
   expose the gaps it will log).
3. Once S1 adds `xwayland` to the image and S4 adds it to `ci.yml`, re-verify
   P3-T03 on CI (the code is in place and `xeyes` is reported as mapped).
4. On the Dell (when hardware work resumes): measure real Full-tier frame time
   with three windows, set `GpuProfile::measured = true` with observed
   throughput, and close the P3-T05 60 fps gate.

## P3-T05 / P3-T06 delivery summary (2026-11-26)

- `compositor/src/glass.rs`: tier material model (design doc §4 values 1:1),
  resolved pass list (Full 16 / Reduced 13 / Minimal 5 passes), damage-only
  effect plan, modelled frame cost vs. the 60 Hz budget, `TierPolicy`
  hysteresis (30 over-budget frames → lower, 120 comfortable → raise).
- `compositor/src/scheduling.rs`: `DamageRegion` (coalescing + exact union
  area), `FrameScheduler` (frame pacing, callbacks bookkeeping, idle
  time/ratio/skips, gap stats).
- `compositor/src/lib.rs`: damage from `wl_surface.commit` (surface + buffer
  coords, buffer-scaled) and WM geometry changes; frame tick in the event loop;
  frame callbacks answered per presented frame; tier switch via
  `AETHER_COMPOSITOR_EFFECT_TIER` / `set_effect_tier`; markers
  `EFFECT_TIER`, `EFFECT_PIPELINE`, `EFFECT_TIER_SWITCH`, `GLASS_GPU_PROFILE`,
  `GLASS_PLAN`, `FRAME`, `FRAME_CALLBACKS`, `IDLE`, `FRAME_STATS`.
- `compositor/tests/p3_t05_t06_effects_frames.rs`: real binary at Full and
  Minimal, three toplevels with damage + frame callbacks.
- Numbers: Full tier 3 windows fully repainted = 2.440 ms of 16.67 ms
  (modelled); damage-only frame = 0.176 ms; Minimal tier = 0.114 ms.


## Notes for other sessions

- S3 (shell): the compositor now advertises `zwlr_layer_shell_v1` with sized
  configures per output and per-output fractional scale. Panels/docks can bind
  `zxdg_output_v1` for logical geometry. The headless compositor is the host for
  on-device-free layer-shell checks.
- S1: P3-T02's Done status is recorded in this file and the results doc; the
  roadmap tracker §4 and `plan.md` remain S1-only.
