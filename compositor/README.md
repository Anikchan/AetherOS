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
- **Effects** (`glass::GlassPipeline` + `glass_shader` + `backend::glass_gles`):
  the tier resolves to a GLES program chain that runs on the KMS renderer —
  dual-Kawase blur, tint, edge refraction, edge highlight, corner mask and the
  Minimal-tier solid fallback. See "Glass shader passes (P3-T05)" below.
- **Window management** (`wm`, `P3-T04`) and layer shell: toplevels are drawn at
  their managed rect, layer surfaces on their anchored edge
  (`backend::layer_location`), shell chrome above windows, cursor on top.
- **Input** (`P3-T09`): libinput keyboard/pointer events feed the `wl_seat`
  created by the backend; pointer motion does hit-testing (Top/Overlay layer
  surfaces first, then windows, topmost first), a click moves keyboard focus,
  and the client cursor surface is drawn as a software cursor.

## Glass shader passes (P3-T05)

`glass::GlassPipeline` resolves a tier into an `EffectPlan` (pass kinds, damaged
pixels, modelled cost). `glass_shader` turns that plan into the concrete program
sequence and uniform values, and `backend/glass_gles` runs it on the GLES
renderer that drives the KMS primary plane. The split keeps the mapping pure and
unit-tested on any host, while only the `drm` feature compiles the GL code.

| Tier | Blur chain | Composite | Tint | Edge |
|---|---|---|---|---|
| Full | 5 down + 5 up, full resolution, radius 24 px | saturation 1.2, refraction band 24 px, 1 px rim | 18 % white/black | 40 % (+20 % on the top edge) |
| Reduced | 4 down + 4 up, half resolution, radius 16 px | saturation 1.1, no refraction | same | 30 % |
| Minimal | none | `fill` mode, no backdrop sample | solid 92 % panel | 20 % |

Shader sources (`src/shaders/`, compiled by `GlesRenderer::compile_custom_texture_shader`):

| File | Program | Uniforms |
|---|---|---|
| `glass_downsample.frag` | dual-Kawase down (5-tap) | `sample_step` |
| `glass_upsample.frag` | dual-Kawase up (8-tap tent) | `sample_step` |
| `glass_composite.frag` | saturation + tint + refraction + rim + corner mask + Minimal fill | `saturation`, `tint`, `edge`, `surface`, `region`, `refraction`, `fill` |

Per frame the backend:

1. renders the Background/Bottom layer surfaces (wallpaper) into one offscreen
   backdrop texture covering the union of the glass surfaces;
2. runs the blur chain per surface over that surface's **damage-scoped region**
   (the frame damage clipped to the surface, grown by a blur halo), not the
   whole surface;
3. composites saturation/tint/edge/refraction into a frost texture and pushes it
   as a `DrmRenderElement::Frost` element *under* the surface's own content;
4. draws shell chrome (Top/Overlay layer surfaces) and windows through the same
   path, so a translucent client buffer shows the frosted backdrop.

Failure policy: if the programs fail to compile, or any pass fails at runtime,
the surface is composited plainly for that frame and the failure is counted in
`AETHER_COMPOSITOR_GLASS_STATS`. A broken shader must never take the desktop
down.

Damage scope: every frame lands in a different KMS swapchain slot, so a
whole-surface frost element would repaint the entire window each frame.
`glass_shader::frost_region` therefore resolves the strip to rewrite — the
damage clipped to the surface plus `halo_px` (blur radius + refraction band) on
every side so the blur never samples a clamped strip edge — and the frost
element covers exactly that region. `DrmCompositor::render_frame` adds each
element's damage to the buffer-age damage of the slot it draws into, so the rest
of the slot keeps the strip written the last time that slot was used. When the
grown region covers half the surface (`FULL_FROST_RATIO`) the whole surface is
repainted in one pass, which is cheaper than scoping it. The frost element keeps
one stable `Id` per `wl_surface` so the tracker diffs instances between frames
instead of treating every frost as a brand-new element.

Evidence: `AETHER_COMPOSITOR_FRAME: … damage_px=… frost_px=…` (both backends)
and `AETHER_COMPOSITOR_GLASS_SHADER: … frost_px=… surface_px=…` (DRM) report the
rewritten area; `AETHER_COMPOSITOR_GLASS_STATS` accumulates `frosted_px` against
`surface_px`. Headless acceptance: `tests/p3_t05_t06_effects_frames.rs` (three
1152×648 windows with a 320×100 damage strip each → `frost_px` ≈ 7 % of
`covered_px`, and exactly `damage_px × 3` at the Minimal tier, which has no
halo).

### Checking the shaders without a GPU

```sh
./compositor/scripts/check-shaders.sh
```

Compiles and links all three fragment stages against the smithay texture vertex
stage. On macOS it uses the system OpenGL framework (desktop GLSL 1.20 with the
ES-only `precision` qualifiers stripped); on Linux it uses `glslangValidator`
when installed. This proves syntax, declarations and types, not driver behaviour.

## Marker reference (DRM additions)

```
AETHER_COMPOSITOR_BACKEND: headless|drm
AETHER_COMPOSITOR_DRM_SESSION: seat=<name> backend=libseat
AETHER_COMPOSITOR_DRM_DEVICE_LIST: count=<n>
AETHER_COMPOSITOR_DRM_PRIMARY_GPU: path=/dev/dri/card0
AETHER_COMPOSITOR_DRM_DEVICE: path=… atomic=true cursor=64x64
AETHER_COMPOSITOR_DRM_CONNECTOR: name=eDP-1 mode=1920x1080@60000mhz crtc=…
AETHER_COMPOSITOR_DRM_RENDERER: backend=gles render_formats=<n>
AETHER_COMPOSITOR_GLASS_SHADER: programs=3 backend=gles shaders=kawase-down,kawase-up,glass-composite
AETHER_COMPOSITOR_GLASS_SHADER: tier=… blur=dual-kawase|none frosted=<n> frosted_total=<n> damage_px=<n> frost_px=<n> surface_px=<n> backdrop=<WxH>|none
AETHER_COMPOSITOR_GLASS_STATS: surfaces=… passes=… textures=… failures=… frosted_px=… surface_px=…
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
8. The glass shader chain on the real GPU: shader compilation through the i915
   driver, the dual-Kawase chain's visual result (blur radius, tint, rim
   highlight on the top edge, refraction band at Full tier), and the 60 fps
   budget with three windows (`AETHER_COMPOSITOR_GLASS_SHADER`).
   `./compositor/scripts/check-shaders.sh` only proves the sources compile.
9. Damage-scoped frost on the real swapchain: `frosted_px` must stay well below
   `surface_px` across a run (`AETHER_COMPOSITOR_GLASS_STATS`) and no strip may
   show a stale or torn frost after a window drag.

### On-device checklist (the Dell)

```sh
./compositor/scripts/kms-checklist.sh            # build + 15 s Full, 6 s Minimal
./compositor/scripts/kms-checklist.sh --duration 60 --client 'foot'   # with a window
./compositor/scripts/kms-checklist.sh --grade kms-checklist-*/primary-full.log
./compositor/scripts/kms-checklist.sh --selftest  # score the fixtures, no device
```

The script sets `AETHER_COMPOSITOR_BACKEND=drm`, bounds the run with
`AETHER_COMPOSITOR_RUN_SECS` so the compositor prints its exit markers, writes
`compositor.log`/`.err` per run, and grades: EGL/GLES context, modeset, page
flips, libinput input, seat, glass programs, effect tier + blur, `failures=0`,
damage-scoped frost (`frosted_px < surface_px`), frame stats and clean shutdown.
Exit code 0 = all checks passed (keypress-dependent input checks are warnings,
never passes). `--grade` re-scores a captured log on any host; `--selftest`
scores `scripts/fixtures/*.log` (one good Full, one good Minimal, one broken) so
the grader itself is verifiable without hardware.
