# D6 — P3-T07 direct scanout for full-screen apps

- Date: 2026-11-27 (IST)
- Session: S2 — `session - 2 (Nov26)`
- Task: P3-T07 (roadmap: "Full-screen bypasses composition"; feature SPD-05 / RES-10)
- Territory: `compositor/`
- Status: **Done (headless model + acceptance test); KMS plane flip hardware-gated**

## 1. What was implemented

`compositor/src/scanout.rs` (pure model, unit-tested):

- `ScanoutSurface { id, rect, opaque, visible }` — the visible toplevels as the
  decision sees them.
- `decide_scanout(surfaces, output_rect, overlays)` — bypasses composition only
  when **all** of the following hold:
  1. exactly one visible toplevel,
  2. its managed rect equals the output rect exactly (`0,0,1920x1080` on the
     primary headless output),
  3. its committed buffer is opaque (`wl_surface.set_opaque_region` covering the
     window, or an XRGB/RGB SHM format with no alpha channel),
  4. no layer-shell surface in the Top/Overlay layer (panel, dock,
     notification, lock) is present.
- `CompositeReason` — `no_windows`, `multiple_windows`, `not_fullscreen`,
  `not_opaque`, `overlay_present` for the log.
- `DirectScanout` — change detection (one marker per transition), direct-frame
  counter, transition counter, marker/stats formatting.

`compositor/src/lib.rs`:

- Re-evaluates the decision on every `wl_surface.commit`, on toplevel/layer
  creation, and on `fullscreen_request` / `unfullscreen_request` (both now
  implemented for xdg-toplevels; fullscreen configures the window to the
  output size).
- `AETHER_COMPOSITOR_SCANOUT_FRAME` frames on the bypass path skip the glass
  pipeline entirely (no blur/tint/shadow planning, `passes=0`).
- New markers:
  - `AETHER_COMPOSITOR_SCANOUT: mode=direct window=0 output=… geometry=…`
  - `AETHER_COMPOSITOR_SCANOUT: mode=composite reason=… windows=… output=…`
  - `AETHER_COMPOSITOR_SCANOUT_FRAME: n=… window=… damage_px=… passes=0`
  - `AETHER_COMPOSITOR_SCANOUT_STATS: mode=… direct_frames=… transitions=…`
  - `AETHER_COMPOSITOR_WM_FULLSCREEN: id=… rect=…`

## 2. Verification

```
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-compositor
```

Result (2026-11-27): **47 unit tests + 5 integration tests green**, including:

- `scanout::tests::*` (8 tests): direct path, exact geometry match, multiple
  windows, transparency, overlay, hidden windows, transition counting.
- `tests/p3_t07_scanout.rs` (`scanout_decisions`), boots the real binary:
  1. floating window → `mode=composite reason=not_fullscreen`;
  2. `set_fullscreen` + 1920×1080 XRGB commit → `mode=direct window=0` and
     `SCANOUT_FRAME … passes=0`;
  3. Top-layer panel over it → `mode=composite reason=overlay_present`;
  4. second scenario with two windows → `reason=multiple_windows`, never
     `mode=direct`.

## 3. Hardware-gated part

The model decides and logs the bypass and the frame path is modelled as
passes=0; the real KMS atomic commit that flips the client's DMA-BUF to a
primary/overlay plane (`drmModeAtomicCommit`, plane format modifiers, fence
import) needs the Dell/Mesa stack (P2-T02) and stays on the hardware checklist.
DMA-BUF opacity cannot be inspected cheaply, so such clients rely on
`set_opaque_region`; otherwise they keep the composited path (safe default).

## 4. Evidence summary

| Requirement | Evidence |
| --- | --- |
| Detect single full-screen opaque toplevel | `scanout::tests`, `p3_t07_scanout` scenario 1–2 |
| Bypass composition | `AETHER_COMPOSITOR_SCANOUT: mode=direct`, `SCANOUT_FRAME … passes=0` |
| Do not bypass when an overlay/another window is present | overlay and multiple-window scenarios |
| Marker exposed | markers in section 1, asserted by the test |
