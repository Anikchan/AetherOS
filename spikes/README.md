# Graphics spikes (P0-T07 / P0-T08)

Standalone Cargo workspace for the two graphics architecture spikes from the
roadmap (`docs/2026/11/16/os-project/03-roadmap-plan.md`). It is deliberately
**not** part of the repository root workspace: the spikes target Linux x86-64
only and must not affect root builds on the macOS dev host.

| Spike | Roadmap | What it proves | Pass marker |
| --- | --- | --- | --- |
| S1 `spike-s1-slint-layershell` | P0-T07 | Slint can draw a layer-shell surface (custom platform API + software renderer over `zwlr_layer_shell_v1`) and request a background blur region | `S1_LAYERSHELL_SURFACE_OK` plus `S1_BLUR_REQUEST_SENT` or `S1_BLUR_UNAVAILABLE` |
| S2 `spike-s2-smithay-headless` | P0-T08 | A minimal Smithay compositor can run headless (no DRM/KMS), accept a client, configure xdg-shell/layer-shell surfaces and detect a mapped surface (buffer attached) | `S2_SOCKET_READY` then `S2_CLIENT_MAPPED`, exit 0 |
| Client `spike-client` | helper for S2 | A pure `wayland-client` xdg toplevel with a 100x100 SHM buffer | `CLIENT_WINDOW_MAPPED`, exit 0 |

## Markers

### Spike S1 (`spike-s1-slint-layershell`)

- `S1_LAYERSHELL_SURFACE_OK` — the compositor sent a
  `zwlr_layer_surface_v1.configure`, the client acked it, rendered the Slint UI
  (colored rectangle + "S1" text) with `SoftwareRenderer` into a shared-memory
  buffer and committed it as the layer surface contents.
- `S1_BLUR_REQUEST_SENT` — `org_kde_kwin_blur_manager` was advertised by the
  compositor; the spike bound it, created a blur object on the surface, set a
  region covering the whole surface and committed it.
- `S1_BLUR_UNAVAILABLE: <reason>` — the compositor does not advertise the KDE
  blur protocol. This is a **spike finding** (background blur is not requestable
  through the Slint/wlr-layer-shell path on such compositors), not a crash.
- `S1_LAYERSHELL_UNAVAILABLE: <reason>` — the compositor does not advertise
  `zwlr_layer_shell_v1` at all; also a spike finding.

Exit code 0 if the layer-surface path completed (including the
`S1_BLUR_UNAVAILABLE` finding), 1 on fatal errors (timeouts, protocol errors).

### Spike S2 (`spike-s2-smithay-headless`)

- `S2_SOCKET_READY: wayland-0` — the Smithay display listens on
  `$XDG_RUNTIME_DIR/wayland-0`.
- `S2_CLIENT_MAPPED` — a client surface committed with a buffer attached
  (via `with_states`/`SurfaceAttributes.buffer`). This is the "window is
  shown" precondition; actual pixel presentation requires a DRM/output backend
  and is intentionally out of scope for a headless runner.
- `S2_SPIKE_DONE` — clean exit 0, ~3 s after the first mapped surface so the
  client can finish its own protocol exchange.

### Client (`spike-client`)

- `CLIENT_WINDOW_MAPPED` — the xdg toplevel was configured, acked, and a
  100x100 XRGB8888 SHM buffer was attached and committed.

## Run locally (Linux, under any Wayland compositor with layer-shell support)

```sh
cd spikes
cargo build --release

# S1 against your desktop compositor (sway, labwc, KWin, ...):
WAYLAND_DISPLAY=wayland-0 ./target/release/spike-s1-slint-layershell

# S2 + client (self-contained; S2 binds wayland-0 in $XDG_RUNTIME_DIR):
spikes/target/release/spike-s2-smithay-headless &   # wait for S2_SOCKET_READY
WAYLAND_DISPLAY=wayland-0 spikes/target/release/spike-client
```

Note: S1 running against S2 works because Smithay advertises
`zwlr_layer_shell_v1`; S2 does not advertise the KDE blur protocol, so S1 will
report `S1_BLUR_UNAVAILABLE` in that configuration. To see
`S1_BLUR_REQUEST_SENT`, run S1 against KWin (or another compositor that
implements `org_kde_kwin_blur_manager`).

## Run in CI

`.github/workflows/graphics-spikes.yml` builds all three binaries on
`ubuntu-latest` (Linux x86-64, the AetherOS target) and:

1. starts `spike-s2-smithay-headless` in a fresh `XDG_RUNTIME_DIR`,
2. runs `spike-client` against it (expects `CLIENT_WINDOW_MAPPED` +
   `S2_CLIENT_MAPPED`),
3. restarts S2 and runs `spike-s1-slint-layershell` against it (expects
   `S1_LAYERSHELL_SURFACE_OK`; blur is expected to be reported as
   `S1_BLUR_UNAVAILABLE` on this host — still a valid spike result),
4. always uploads all logs as a workflow artifact.

Dependencies are pure Rust (no libwayland linkage); only
`libxkbcommon`/`pkg-config` are installed for Smithay's transitive
`xkbcommon` crate.

## Implementation notes / known simplifications

- S1 uses `slint::platform::{Platform, WindowAdapter}` +
  `MinimalSoftwareWindow` + `SoftwareRenderer::render` into a
  `Rgb565Pixel` staging buffer, converted to XRGB8888 for wl_shm.
- S2 uses Smithay 0.7 with only the `wayland_frontend` feature (no
  `backend_drm`/`backend_udev`/`backend_libinput`/renderer features) — the
  headless scope is: Wayland server + client + surface mapping, not rendering.
- No `unwrap`/`expect` in the spike binaries; all errors exit with a clear
  message and a non-zero status.
