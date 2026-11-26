# D6 — P3-T09 DRM/KMS + libinput backend (compositor on real hardware)

- Date: 2026-11-27 (IST)
- Session: S2 — `session - 2 (Nov26)`
- Task: P3-T09 (compositor backend: DRM/KMS output + evdev input; roadmap P3
  "real desktop" prerequisite)
- Territory: `compositor/`, results
- Status: **Implemented + type-checked for x86_64-unknown-linux-gnu; headless
  path green. Runtime/hardware validation on the Dell is gated (hardware
  testing paused).**

## 1. What was added

### `compositor/src/backend/mod.rs` (new)

- `BackendKind { Headless, Drm }`, `AETHER_COMPOSITOR_BACKEND`
  (`headless` | `drm` | `kms` | `auto`), `select_backend(requested,
  drm_available, drm_compiled)` and `is_unknown_backend`.
- Rules: `headless` always wins; explicit `drm`/`kms` is honoured even with no
  device (fail loud, never a silent black desktop); `auto`/unset follows
  `/dev/dri/card*`; an unknown value falls back to headless and is reported;
  DRM requires the `drm` cargo feature.
- `drm_available()` (device-node probe) and `layer_location()` (anchored layer
  surface placement, pure).

### `compositor/src/backend/drm.rs` (new, `#[cfg(all(target_os = "linux", feature = "drm"))]`)

- **Seat/session**: `LibSeatSession` (seatd / logind), pause/resume →
  `DrmDevice::pause/activate` + full-output repaint.
- **Device discovery**: `UdevBackend` device list + `primary_gpu`; hotplug
  events logged.
- **Scanout**: first connected connector with modes → preferred mode + CRTC →
  `DrmSurface`; `GbmAllocator` (`RENDERING|SCANOUT`) +
  `GbmFramebufferExporter` (render node for client-buffer direct scanout) →
  `DrmCompositor` with `FrameFlags::DEFAULT`, Xrgb8888 primary, hardware cursor
  plane (`Some(gbm)`).
- **Rendering**: `EGLDisplay` on the GBM device + `EGLContext` + `GlesRenderer`;
  per due frame builds back-to-front elements (Background/Bottom layers →
  active-workspace toplevels at their managed rect → Top/Overlay layers →
  software cursor), renders and queues; `frame_submitted()` on the CRTC VBlank,
  then `complete_frame()` answers `wl_surface.frame`.
- **Input**: libinput over the seat → `wl_seat` keyboard + pointer; pointer
  motion clamping, hit-testing (Top/Overlay layers first, then windows topmost
  first), click-to-focus, axis frames, client cursor surface.
- **XWayland**: spawned like the headless path (P3-T03), so X11 apps still work
  on hardware.
- Markers: `DRM_SESSION`, `DRM_DEVICE_LIST`, `DRM_PRIMARY_GPU`, `DRM_DEVICE`,
  `DRM_CONNECTOR`, `DRM_RENDERER`, `DRM_OUTPUT`, `DRM_VBLANK`,
  `DRM_SESSION_EVENT`, `DRM_HOTPLUG`, `DRM_STATS`, `SEAT`, `SEAT_FOCUS`,
  `INPUT`, `INPUT_KEY`, `INPUT_BUTTON`.

### `compositor/src/lib.rs` (wired, headless behaviour unchanged)

- `AetherCompositor::new_with_outputs(dh, specs)`; `new(dh)` = the previous
  two-monitor headless layout, `new_for_drm(dh)` = no synthetic outputs;
  `register_output()` advertises the KMS connector and seeds the WM size.
- Frame scheduling split: `begin_frame()` (due frame + damage) and
  `complete_frame(frame)` (markers, glass plan, tier policy, frame-gap logger,
  `wl_surface.frame` callbacks). `frame_tick()` = both, so the headless path
  behaves exactly as before; the DRM backend calls them around the page flip.
- New state: `seat`, `pointer_location`, `cursor_status`, `serials`,
  `drm` (feature-gated). `delegate_seat!` + `SeatHandler::focus_changed` /
  `cursor_image`. A newly mapped toplevel takes keyboard focus when a seat
  exists (no-op headless).
- `damage_whole_output()` / `damage_pointer()` helpers.

### `compositor/Cargo.toml`, `compositor/scripts/`, `compositor/pc-stubs/`

- `drm` cargo feature → `smithay/{backend_drm,backend_gbm,backend_udev,
  backend_libinput,backend_session_libseat,backend_egl,renderer_gl,desktop}`.
  Opt-in so CI/macOS never link libgbm/libinput/libudev/libseat.
- `scripts/check-linux-target.sh`: cross type-check from the dev host
  (`--target x86_64-unknown-linux-gnu --features drm`); puts the rustup
  toolchain on `PATH` (Homebrew `rustc` has no Linux std) and points
  `PKG_CONFIG_LIBDIR` at stub `.pc` files for `libudev-sys`/`libseat-sys`
  (bindings are pre-generated; `cargo check` does not link).

## 2. Verification (run in this session)

| Command | Result |
|---|---|
| `cargo test -p aether-compositor` (macOS, `LIBRARY_PATH=/opt/homebrew/lib`) | **pass** — 54 unit + 5 existing integration suites + new `p3_t09_backend_selection` (9 tests) |
| `cargo check -p aether-compositor --target x86_64-unknown-linux-gnu` | **pass** (headless, no feature) |
| `./compositor/scripts/check-linux-target.sh` (= same + `--features drm`) | **pass** — DRM/KMS + libinput module type-checks for the device target |
| `cargo check -p aether-compositor --tests` | **pass**, no warnings |

Tests added (`compositor/tests/p3_t09_backend_selection.rs`, plus `backend`
unit tests):

- `headless` always wins; explicit `drm` honoured without a device; `auto`
  follows device presence; DRM needs the cargo feature; unknown values fall back
  to headless and are reported; backend names round-trip; layer placement maths;
  the real binary prints `AETHER_COMPOSITOR_BACKEND: headless`, keeps
  `OUTPUT_COUNT: 2`, never emits a `DRM_` marker, and reports an unknown value
  while still booting headless.

## 3. Hardware-gated residual (not claimed)

Hardware testing on the Dell is paused (network/subnet mismatch, per the
2026-11-26 kickoff). Therefore nothing below is asserted as working:

1. EGL/GLES context creation on i915 + first modeset (no GPU here).
2. Real page flips/VBlank cadence at 60 Hz (P3-T05/P3-T06 hardware acceptance).
3. Direct scanout landing on the primary plane (P3-T07 KMS half).
4. libinput keyboard/pointer/touchpad (P2-T05) and the software cursor.
5. Session pause/resume (VT switch) repaint.
6. Second KMS output / hotplug reconfiguration (single connector today; the
   headless backend already proves the two-output layout).
7. KMS HiDPI (advertised scale is 1.0; placement maths is scale-aware).
8. Glass shader passes (blur/tint/refraction/shadow) are still modelled only;
   the GLES renderer draws client buffers + solid clear colour.

Run on the device: `cargo build -p aether-compositor --features drm` then
`AETHER_COMPOSITOR_BACKEND=drm ./target/debug/aether-compositor`
(see `compositor/README.md`). Requirements: libseat (seatd/logind), libgbm,
libdrm, libinput, libudev, a DRM device on the session seat and a connected
display.

The graded version of that run is `compositor/scripts/kms-checklist.sh`: it sets
`AETHER_COMPOSITOR_BACKEND=drm` and `AETHER_COMPOSITOR_RUN_SECS`, captures every
marker into a log and scores EGL/GLES context, modeset, page flips, input, seat,
glass programs, tier/blur, `failures=0`, damage-scoped frost and damage-scoped
backdrop capture. See the damage-scope sections of
`docs/2026/11/27/results/p3-t05-gles-glass-passes.md`.
