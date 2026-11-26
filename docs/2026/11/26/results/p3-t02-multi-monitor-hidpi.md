# P3-T02 — XDG shell, layer shell, multi-monitor, HiDPI (D6 results)

- Date: 2026-11-26
- Session: S2 (compositor)
- Scope: P3-T02 — extend the Smithay compositor with xdg-shell, `zwlr_layer_shell_v1`,
  multi-monitor output layout and HiDPI (two monitors at different scales)
- Host: macOS (Apple silicon) dev host for build + headless run; Linux CI
  (`cargo test --workspace`, ubuntu-latest) runs the same integration test
- Crate: `compositor/` (`aether-compositor`)

## 1. What changed

| File | Change |
| --- | --- |
| `compositor/src/lib.rs` | Two headless `wl_output` globals in one logical layout (`HEADLESS_OUTPUTS`), `zxdg_output_v1` logical geometry, `wp_fractional_scale_v1` preferred scale per surface, `wl_surface.enter` on assignment, `wl_surface.set_buffer_scale` acceptance + marker, layer-shell size negotiation (`configure_layer_surface`), toplevel/layer-surface registries, `AETHER_COMPOSITOR_GRACE_MS` override |
| `compositor/Cargo.toml` | Dev-dependencies for the headless acceptance client (`wayland-client`, `wayland-protocols`, `wayland-protocols-wlr`) |
| `compositor/tests/p3_t02_headless.rs` | New CI acceptance test: boots the real binary and drives it with one Wayland client |

No other crate or workflow is touched. `ci.yml` already runs
`cargo test --workspace`, so the new test is a CI smoke test without a workflow
edit (workflow files belong to other sessions).

## 2. Design

### Monitors and scales (`HEADLESS_OUTPUTS`)

| Output | Mode (physical) | Scale | `wl_output.scale` | `zxdg_output_v1` logical size | Logical position |
| --- | --- | --- | --- | --- | --- |
| `aether-headless-0` | 1920×1080 | 1.0 | 1 | 1920×1080 | (0, 0) |
| `aether-headless-1` | 2880×1800 | 1.5 | 2 (integer protocols round up) | 1920×1200 | (1920, 0) |

This is the "two monitors at different scales" acceptance case: the second
monitor is HiDPI, side by side with the first, and its logical geometry is
computed from the fractional scale.

### HiDPI plumbing

- `wp_fractional_scale_manager_v1` is advertised. When a surface is assigned to
  an output (`wl_surface.enter`), the compositor records that output's
  fractional scale and answers `get_fractional_scale` with
  `preferred_scale` (1.5 → 180/120).
- `wl_surface.set_buffer_scale` is accepted; a scale > 1 emits
  `AETHER_COMPOSITOR_BUFFER_SCALE: surface_buffer_scale=N` so tests (and later
  the renderer) can see that the client drew for the HiDPI monitor.
- `zxdg_output_v1` gives clients logical position/size without doing the
  fractional math themselves.

### Layer shell (panel/dock)

- `zwlr_layer_shell_v1` is advertised (existing P3-T01 handler, extended).
- On creation, the surface is bound to the requested output (or the primary
  output when the client passes `NULL`), gets `wl_surface.enter`, and an
  immediate configure.
- On every commit, the compositor reads `LayerSurfaceCachedState` (anchors,
  margins, requested size) and sends a sized configure:
  - anchored on both opposite edges → stretch to the output's **logical** size;
  - explicit non-zero size → use it;
  - otherwise fill the output along that axis;
  - margins are subtracted.
  A panel anchored `TOP|LEFT|RIGHT` with height 32 on the 1.5× monitor is
  configured at `1920×32` (logical), not `2880×32`.

### xdg-shell

Toplevels are assigned to the primary output (`wl_surface.enter`), registered,
and configured. Popups keep the P3-T01 no-op handler (real popup positioning
lands with the window-management task P3-T04).

## 3. Verification

### Commands and results (2026-11-26, macOS dev host)

```
$ LIBRARY_PATH=/opt/homebrew/lib cargo build -p aether-compositor
    Finished `dev` profile

$ LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor
running 1 test
test two_outputs_layer_shell_and_xdg_toplevel ... ok
test result: ok. 1 passed; 0 failed
```

(`LIBRARY_PATH` is only needed on this dev host because the Homebrew
`libxkbcommon` library directory is not on the default linker search path; it is
a pre-existing local environment quirk, not a code requirement. CI links against
`libxkbcommon-dev` normally.)

### Compositor log from the acceptance run

```
AETHER_COMPOSITOR_OUTPUT: name=aether-headless-0 mode=1920x1080 scale=1 logical=1920x1080 position=0,0
AETHER_COMPOSITOR_OUTPUT: name=aether-headless-1 mode=2880x1800 scale=1.5 logical=1920x1200 position=1920,0
AETHER_COMPOSITOR_OUTPUT_COUNT: 2
AETHER_COMPOSITOR_SOCKET_READY: wayland-0
AETHER_COMPOSITOR_OUTPUT_BOUND: name=aether-headless-0
AETHER_COMPOSITOR_OUTPUT_BOUND: name=aether-headless-1
AETHER_COMPOSITOR_XDG_TOPLEVEL: output=aether-headless-0
AETHER_COMPOSITOR_CLIENT_MAPPED
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-test layer=Top output=aether-headless-1 scale=1.5
AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-test size=1920x32
AETHER_COMPOSITOR_BUFFER_SCALE: surface_buffer_scale=2
```

(`AETHER_COMPOSITOR_TEST_LOG=1 cargo test … -- --nocapture` reproduces it.)

### What the test asserts

| Assertion | Result |
| --- | --- |
| Exactly two `wl_output` globals | Pass |
| Primary: scale 1, mode 1920×1080, logical 1920×1080 at (0,0) | Pass |
| HiDPI: `wl_output.scale` 2, mode 2880×1800, logical 1920×1200 at (1920,0) | Pass |
| `xdg_wm_base` toplevel configures, acks, maps a 100×100 SHM buffer, stays open | Pass |
| `zwlr_layer_shell_v1` surface on the HiDPI output configures at `1920×32` | Pass |
| `wp_fractional_scale_v1.preferred_scale` = 180 (1.5×) for that surface | Pass |
| Layer surface maps a 200×64 buffer at `set_buffer_scale(2)` | Pass |
| Compositor markers: 2 outputs, layer surface on `aether-headless-1`, xdg toplevel on `aether-headless-0`, buffer scale 2, mapped surface | Pass |

P3-T01's existing CI smoke step (`AETHER_COMPOSITOR_SOCKET_READY`,
`AETHER_COMPOSITOR_CLIENT_MAPPED` with `spike-client`) keeps working: both
markers are unchanged and produced by the same code paths.

## 4. Risks / not yet done (honest scope)

- **No presentation yet.** This is still the headless backend: no DRM/KMS, no
  rendering, no frame callbacks. The multi-monitor model, scales and logical
  geometry are real protocol state; pixels arrive with P2-T02 (DRM) and
  P3-T05/T06.
- **Layer-shell placement** is negotiated as a size only; the surface's final
  position within its layer is not computed (no renderer). The shell (S3) can
  already rely on configure sizes and per-output scale.
- **Popups** are not positioned (P3-T04).
- **Input** (seat/pointer/keyboard focus) is unchanged from P3-T01 (no
  `wl_seat` global).
- Output layout is fixed at build time. Runtime reconfiguration (hotplug,
  scale change, rearranging in Settings) belongs with P7/DRM work.

## 5. P3-T03 (XWayland) — blocked in this environment

P3-T03's Done criterion is "X11 test app runs", which needs a real XWayland
process. It cannot be produced or verified in this session:

1. The dev host is macOS; XWayland is Linux-only (`Xwayland` binary absent).
2. The CI image (`ubuntu-latest` + the packages installed by `ci.yml`) does not
   install `xwayland` or an X11 test client (`x11-apps`, e.g. `xeyes`).
   `ci.yml` is session S4's file, not S2's.
3. The OS image has no `xwayland` package yet; Buildroot config is session S1's
   territory (`os-build/buildroot/`).
4. `smithay`'s XWayland support also requires implementing an X11 window manager
   (`XwmHandler`/`X11Wm`) and `xwayland_shell_v1` serial association. That code
   cannot be exercised at all here, so writing it now would be unverified.

**Prerequisites to unblock (for S1/S4):**

- `os-build/buildroot/`: add `xwayland` (and its deps) to the target image.
- `ci.yml`: `sudo apt-get install -y xwayland x11-apps` (or run the test in a
  container that has them).
- Then S2 implements in `compositor/`: smithay `xwayland` feature,
  `XWayland::spawn` + `X11Wm::start_wm`, `xwayland_shell_v1` window→surface
  association, and an acceptance test that runs `xeyes` and checks its
  `wl_surface` maps through the compositor (marker
  `AETHER_COMPOSITOR_XWAYLAND_MAPPED`).

Status recorded as **Blocked (no XWayland binary in dev/CI/image environments;
needs the packages above)**, not Done.
