# `aether-compositor`

Smithay-based Wayland compositor for AetherOS (roadmap P3). Two output backends
share one window-system/render state:

| Backend | What it does | Where it runs |
|---|---|---|
| `headless` | Two synthetic `wl_output`s (1920×1080 @ scale 1, 2880×1800 @ scale 1.5), no rendering, no input | macOS dev host, CI runners, any run without a GPU |
| `drm` | DRM/KMS page flips on the primary plane via GBM + EGL/GLES, libinput on a libseat udev seat | the target machine (Intel/AMD GPU), needs `--features drm` |

## Backend selection

`AETHER_COMPOSITOR_BACKEND` = `headless` | `drm` | `kms` | `auto` (default
`auto`). Rules (`src/backend/mod.rs::select_backend`):

1. `headless` always wins.
2. `drm`/`kms` requests KMS even if no device node was found — the backend then
   fails with a diagnosable error instead of showing a black headless desktop.
3. `auto`/unset picks DRM when `/dev/dri/card*` exists, headless otherwise.
4. An unknown value falls back to headless and is reported on stderr.
5. DRM is only selectable when the binary was built with `--features drm`.

The chosen backend is printed first as `AETHER_COMPOSITOR_BACKEND: <kind>`.

## Run

Headless (CI, dev host, tests):

```sh
cargo run -p aether-compositor
# or force it on a machine that has a GPU
AETHER_COMPOSITOR_BACKEND=headless cargo run -p aether-compositor
```

Hardware (the Dell, or any Linux box with a DRM device and a seat):

```sh
cargo build -p aether-compositor --features drm
AETHER_COMPOSITOR_BACKEND=drm ./target/debug/aether-compositor
```

Requirements for the DRM backend:

- `libseat` (`seatd`, or systemd-logind on a TTY — no root needed when seatd or
  logind grants the seat),
- `libgbm`, `libdrm`, `libinput`, `libudev` (runtime),
- the `drm` cargo feature (above),
- a DRM device on the session's seat (`/dev/dri/card*`) and a connected display.

### Type-checking the DRM backend from the macOS dev host

The backend is Linux-only, so on macOS it is not compiled into the default
build. To prove it still compiles for the device target:

```sh
./compositor/scripts/check-linux-target.sh
# = cargo check -p aether-compositor --target x86_64-unknown-linux-gnu --features drm
```

The script puts the rustup toolchain on `PATH` (the Homebrew `rustc` has no
Linux std) and points `PKG_CONFIG_LIBDIR` at `compositor/pc-stubs/`, which holds
stub `.pc` files for the two crates whose build scripts probe pkg-config
(`libudev-sys`, `libseat-sys`). `cargo check` does not link, so no Linux
sysroot or native library is needed and the stub never affects a real Linux
build.

## What the DRM backend wires into the existing state

- **Frame scheduling** (`scheduling::FrameScheduler`): `begin_frame()` takes the
  due frame and its damage; the KMS render uses it as its pacing signal, so an
  idle desktop submits no work. Frame callbacks are answered from
  `complete_frame()` only after the page flip (`DrmDevice` VBlank →
  `frame_submitted`), so clients throttle to real presentation.
- **Direct scanout** (`scanout::DirectScanout`): a single opaque full-screen
  window with no Top/Overlay layer surface is submitted as one element, letting
  `DrmCompositor` flip the client buffer on the primary plane (`passes=0`,
  `AETHER_COMPOSITOR_SCANOUT_FRAME`).
- **Effects** (`glass::GlassPipeline`): the tier/pipeline plan, cost model and
  marker stay wired (`AETHER_COMPOSITOR_GLASS_PLAN`, tier switches on frame
  budget). The blur/refraction/shadow *shader* passes themselves are not yet
  drawn by the GLES renderer — the DRM path currently composes client buffers
  plus a solid clear colour. That is the remaining graphics work (see D1 §4).
- **Window management** (`wm`, `P3-T04`) and layer shell: toplevels are drawn at
  their managed rect, layer surfaces on their anchored edge
  (`backend::layer_location`), shell chrome above windows, cursor on top.
- **Input** (`P3-T09`): libinput keyboard/pointer events feed the `wl_seat`
  created by the backend; pointer motion does hit-testing (Top/Overlay layer
  surfaces first, then windows, topmost first), a click moves keyboard focus,
  and the client cursor surface is drawn as a software cursor.

## Marker reference (DRM additions)

```
AETHER_COMPOSITOR_BACKEND: headless|drm
AETHER_COMPOSITOR_DRM_SESSION: seat=<name> backend=libseat
AETHER_COMPOSITOR_DRM_DEVICE_LIST: count=<n>
AETHER_COMPOSITOR_DRM_PRIMARY_GPU: path=/dev/dri/card0
AETHER_COMPOSITOR_DRM_DEVICE: path=… atomic=true cursor=64x64
AETHER_COMPOSITOR_DRM_CONNECTOR: name=eDP-1 mode=1920x1080@60000mhz crtc=…
AETHER_COMPOSITOR_DRM_RENDERER: backend=gles render_formats=<n>
AETHER_COMPOSITOR_DRM_OUTPUT: connector=eDP-1 mode=1920x1080 refresh_mhz=60000
AETHER_COMPOSITOR_DRM_VBLANK: n=<frame> time_ms=<t> total=<n>
AETHER_COMPOSITOR_DRM_SESSION_EVENT: state=paused|active
AETHER_COMPOSITOR_DRM_HOTPLUG: added|removed|changed id=<dev_t>
AETHER_COMPOSITOR_DRM_STATS: frames_queued=… vblanks=… direct_frames=…
AETHER_COMPOSITOR_SEAT: name=seat-0 keyboard=true pointer=true
AETHER_COMPOSITOR_SEAT_FOCUS: surface=yes|none
AETHER_COMPOSITOR_INPUT: backend=libinput seat=<name>
AETHER_COMPOSITOR_INPUT_KEY / _INPUT_BUTTON: …
```

## Hardware-gated residual (not claimed by the test suite)

`cargo test -p aether-compositor` never starts the DRM backend. It checks the
selection logic, the layer placement maths and that the real binary boots the
headless backend and reports its choice (`tests/p3_t09_backend_selection.rs`).
Still to be validated on the Dell:

1. EGL/GLES context creation on the Intel i915 driver and the first modeset.
2. Real page flips and `AETHER_COMPOSITOR_DRM_VBLANK` cadence at the panel's
   refresh rate (60 Hz) with three glass windows (P3-T05 acceptance).
3. Direct scanout actually landing on the primary plane (P3-T07 with KMS).
4. libinput keyboard/pointer (incl. touchpad, P2-T05) and the software cursor.
5. Session pause/resume (VT switch) and its full-output repaint.
6. Second KMS output / hotplug reconfiguration (headless already proves the
   two-output layout; the DRM path currently drives one connector).
7. HiDPI on KMS (`output` scale is 1.0 today; the layout maths is scale-aware).
8. The blur/tint/refraction shader passes of the glass pipeline.
