# App window backend — a launched app can create a Wayland window (2026-11-28)

Mission: close the gap that would make the first graphical boot show the desktop
but no app windows. Before this change the workspace pinned

```toml
slint = { version = "1.19.0", default-features = false, features = [
    "compat-1-18", "renderer-software", "software-renderer-systemfonts", "std",
] }
```

for every crate — software rendering only, **no window backend**. The shell is
fine (it paints its own layer surfaces through its pure-Rust `wayland-client`
host), but when it spawns `<exec> --launch <id>` the app process had no way to
create a `wl_surface`, and every app main printed its usage and exited.

Status: **implemented; app runtime window creation verified only on Linux** (the
new test skips with a printed line on macOS, and cannot be executed on this
macOS dev host — see "Verification"). Everything that can be checked on macOS
(workspace build, all app tests, `--render`, feature scoping, Linux cross-check
of the Wayland path) is green.

## The change

| Path | Purpose |
| --- | --- |
| `apps/*/Cargo.toml` (11 GUI apps) | opt in to `backend-winit` + `backend-winit-wayland` on `slint` |
| `apps/*/src/main.rs` (10 apps) | no-flag launch path: build the controller, `show()`, `run_event_loop()` |
| `apps/files/tests/window_wayland.rs` | Linux runtime proof on the repo's headless compositor |
| `Cargo.toml` | scoping note on the workspace dependency (no feature change) |
| `Cargo.lock` | winit / smithay-client-toolkit / sctk-adwaita / softbuffer graph |

The shell and `ui` manifests are untouched; `compositor`, `os-build`, `test.sh`
and `shell/**` were not modified.

### Feature matrix

| Crate(s) | Slint features | Window backend at runtime |
| --- | --- | --- |
| `shell`, `ui` | `compat-1-18`, `renderer-software`, `software-renderer-systemfonts`, `std` | none compiled in. Shell: its own `wayland-client` layer host + `MinimalSoftwareWindow`. Tests: `i-slint-backend-testing` (dev-dependency) |
| `apps/archive`, `browser`, `calculator`, `calendar`, `documents`, `files`, `images`, `player`, `taskmanager`, `terminal`, `text` | the four above **plus** `backend-winit`, `backend-winit-wayland` | Slint winit backend → Wayland → softbuffer → `wl_shm` (software renderer, no GPU). `backend-winit` also enables winit's X11 half; the image has no X server, and `WAYLAND_DISPLAY` makes winit choose Wayland |

`backend-winit-wayland` alone would be enough for the image (it activates
`i-slint-backend-winit` through `i-slint-backend-selector`); `backend-winit` is
kept because the brief asked for it and it leaves the XWayland fallback in the
binary without extra build steps.

`taskmanager` gets the features so its manifest is uniform with the other apps,
but its launch path is still its CLI default (`aether-taskmanager` prints the
process report). Adding an `ActivityManagerWindow` launch is a follow-up; the
window binding code already exists in `apps/taskmanager/src/main.rs::render_window`.

### What the app binaries do now

`--launch <id>` is stripped by `aether-launcher-registry::handle_launch_arg`, so
the shell's spawn reaches the app with no flags. Previously that arm printed
usage; now it is the window path:

```
aether-files --render <dir>   → headless PNG (unchanged: aether_ui::render::init() first)
aether-files --help           → usage
aether-files                  → real window, Slint event loop until closed
```

`--render` is unchanged in every app: `aether_ui::render::init()` installs the
headless `MinimalSoftwareWindow` platform **before** any component is created,
so the winit backend is never selected on that path. The app tests keep using
`i-slint-backend-testing::init_integration_test_with_system_time()`, which calls
`slint::platform::set_platform` explicitly; winit being linked in does not
change which platform the tests install.

## Scoping evidence (macOS host, cargo 1.100.0)

Features are per-crate, and a build that selects several packages unifies the
features of a shared dependency. Cargo's resolver `"2"` (already set in
`Cargo.toml`) does **not** prevent unification of *normal* dependencies; it only
separates build-dependencies/target-specific/dev graphs. The evidence below
shows the two cases honestly.

Per-package builds — the scoping the image uses:

```
$ cargo tree -p aether-shell -e features -i slint
slint v1.19.0
├── slint feature "compat-1-18"
├── slint feature "renderer-software"
├── slint feature "software-renderer-systemfonts"
└── slint feature "std"
    (no backend-winit anywhere)

$ cargo tree -p aether-shell -i winit
warning: nothing to print.          # the shell does not depend on winit at all

$ cargo tree -p aether-files -e features -i slint
slint feature "backend-winit"
slint feature "backend-winit-wayland"
slint feature "compat-1-18"
slint feature "renderer-software"
slint feature "software-renderer-systemfonts"
slint feature "std"

$ cargo tree -p aether-files -i winit
winit v0.30.13
└── i-slint-backend-winit v1.19.0
    └── i-slint-backend-selector v1.19.0
        └── slint v1.19.0
```

Cargo's own artifact record for a single-package build:

```
$ cargo check -p aether-shell --message-format=json  (slint artifact)
slint features: compat-1-18, renderer-software, software-renderer-systemfonts, std

$ cargo check -p aether-files --message-format=json  (slint artifact)
slint features: backend-winit, backend-winit-wayland, compat-1-18, renderer-software,
                software-renderer-systemfonts, std
```

A `--workspace` build unifies (one `slint` package in the resolve):

```
$ cargo check --workspace --message-format=json  (slint artifact, one unit)
slint features: backend-winit, backend-winit-wayland, compat-1-18, renderer-software,
                software-renderer-systemfonts, std
```

Consequence, stated plainly: `cargo build --workspace` **does** compile
`slint` once with winit and every workspace binary that links `slint` — including
`aether-shell` — links that one rlib. The shell's behaviour is unchanged (it
never calls into winit; it installs its own platform / its layer host), but if
"the shell binary must not contain winit" is a hard requirement, build it on its
own:

```
cargo build -p aether-shell
cargo build -p aether-files -p aether-text ...   # apps
```

This is how the image and the shell's own tests are built (`-p`), and the
`cargo tree` / artifact output above proves the scoping there. There is no
Cargo mechanism to keep winit out of a *workspace* build while the apps need it:
same package id, same feature resolution.

## Runtime proof (Linux only)

`apps/files/tests/window_wayland.rs`:

1. creates a private `0700` `XDG_RUNTIME_DIR`;
2. starts `aether-compositor` with `AETHER_COMPOSITOR_BACKEND=headless` and
   `AETHER_COMPOSITOR_GRACE_MS=20000` (no DRM, no GPU, no seat), stdout to
   `compositor.log`;
3. waits for `wayland-0`, then starts `env!("CARGO_BIN_EXE_aether-files")` with
   `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY=wayland-0` and no arguments (the
   `--launch` path);
4. polls the compositor log for `AETHER_COMPOSITOR_XDG_TOPLEVEL`,
   `AETHER_COMPOSITOR_WM_WINDOW` and `AETHER_COMPOSITOR_CLIENT_MAPPED` (the
   last one is the softbuffer → `wl_shm` half: a real SHM buffer was committed),
   fails with both logs if the app exits or 45 s pass, and asserts the app is
   still alive after mapping.

On macOS the test prints
`SKIP launched_app_opens_a_toplevel_on_headless_wayland: runtime window creation is verified on Linux only (Wayland client + softbuffer)`
and passes.

Exact command (also the CI step to add — `.github/workflows/ci.yml` is another
session's territory, handed to S1):

```bash
cargo build -p aether-compositor --bin aether-compositor
cargo test -p aether-files --test window_wayland -- --nocapture
```

The test builds the compositor itself through `$CARGO` if the binary is not next
to `target/<profile>/aether-files`, so a bare `cargo test --workspace` on a Linux
runner is enough as well; the explicit build step only makes the first run fast.

The runner's existing "Install system dependencies" step already carries what
this path needs: `libfontconfig1-dev`/`libfreetype-dev` (Slint system fonts,
runtime `fontconfig`), `libxkbcommon-dev`, and the `libwayland-client` link that
the shell already requires. If the app link ever reports
`ld: cannot find -lwayland-client`, add `libwayland-dev` there; at runtime the
app only needs `libwayland-client.so.0` and fontconfig, both on `ubuntu-latest`.

## Verification run in this session (macOS)

| Command | Result |
| --- | --- |
| `cargo check --workspace --all-targets` | ok (only pre-existing warnings; one was fixed in the new test) |
| `LIBRARY_PATH=/opt/homebrew/lib cargo build --workspace` | ok, all bins link |
| `cargo build --workspace` (no `LIBRARY_PATH`) | fails at `aether-compositor` link: `ld: library 'xkbcommon' not found`. Pre-existing macOS/Homebrew search-path issue: `xkbcommon` is only used by `smithay ← aether-compositor` (`cargo tree -i xkbcommon`), untouched by this change, and the compositor does not build on macOS without the Homebrew lib dir either |
| `cargo test -p aether-archive -p aether-browser -p aether-calculator -p aether-calendar -p aether-documents -p aether-files -p aether-images -p aether-player -p aether-terminal -p aether-text -p aether-taskmanager` | all pass (incl. every `tests/ui.rs` that installs the testing backend with winit linked in) |
| `cargo test -p aether-shell` | **179 passed, 0 failed** (re-run after the shell session committed its layer-paint work; the earlier `cannot find function specs` compile error was their uncommitted tree) |
| `cargo test -p aether-shell -p aether-files` (one build graph, so `slint` unifies to `backend-winit, backend-winit-wayland, …, std`) | **196 passed, 0 failed** — the shell's UI/render tests still select `i-slint-backend-testing` and pass with winit compiled into the same `slint` rlib |
| `./target/debug/aether-files --render /tmp/...` (also `aether-text`, `aether-calculator`) | PNGs written, same sizes/markers as before |
| Linux cross-check: `RUST_FONTCONFIG_DLOPEN`/stub `fontconfig.pc` + `rustup run stable cargo check --target x86_64-unknown-linux-gnu -p aether-files` | `Finished dev profile` — winit's Wayland path, sctk, softbuffer and slint's backend selector compile for Linux |

The Linux cross-check needs two workarounds on this host because there is no
Linux sysroot:

```bash
# /tmp/aether-pc/fontconfig.pc is a stub .pc with empty Cflags and -lfontconfig,
# because yeslogic-fontconfig-sys runs pkg-config in its build script and
# RUST_FONTCONFIG_DLOPEN=1 does not compile with this fontique version.
export PKG_CONFIG_PATH=/tmp/aether-pc PKG_CONFIG_ALLOW_CROSS=1
export RUSTC="$(rustup which rustc --toolchain stable)"
export RUSTDOC="$(rustup which rustdoc --toolchain stable)"
rustup run stable cargo check --target x86_64-unknown-linux-gnu -p aether-files
```

That is a type-check of the Wayland path, **not** a runtime proof. Runtime window
creation is verified only by the Linux test above (CI or the device). This
session had no Docker daemon and no Linux machine, so it could not execute it.

## Risks

- **winit version**: `i-slint-backend-winit 1.19.0` pins `winit 0.30.2..0.31`;
  the lock resolved `0.30.13`. A future Slint bump can move winit; keep the
  Slint pin (`1.19.0`) for shell and apps together.
- **softbuffer + Wayland**: the software renderer on winit uses softbuffer over
  `wl_shm`. The headless compositor advertises `Argb8888`/`Abgr8888` (plus 10-bit
  formats) and that is what softbuffer asks for; the DRM backend must keep
  advertising an `Argb8888` shm format, otherwise app windows would fall back to
  an unsupported format.
- **No seat in headless mode**: the headless compositor advertises no `wl_seat`
  unless `AETHER_COMPOSITOR_INJECT_KEYS` is set. winit tolerates that (the window
  maps and commits buffers), which is all the test needs, but keyboard/pointer
  input into app windows can only be tested with a seat present — use the
  injection env or the DRM backend for that.
- **fontconfig at runtime**: `software-renderer-systemfonts` reads system fonts.
  The image needs fontconfig + at least one font package; on a Linux host
  without fontconfig the app fails at startup, not at link time.
- **X11 half of `backend-winit`**: compiled in (x11rb/`x11-dl`, no system libs at
  build time). Harmless without an X server; it only grows the binary. If image
  size matters, drop to `backend-winit-wayland` alone — it still activates the
  winit backend.
- **`--render` unchanged**: it installs the headless `aether-ui` platform first,
  so the winit backend never initialises. All `--render` evidence paths keep
  producing the same PNGs (checked for files, text, calculator).
- **Workspace unification** (see above): `cargo build --workspace` links winit
  into the shell binary. Behaviour is unchanged; build the shell with `-p` when
  the separation matters.
- **macOS dev host**: running an app with no flags now opens a native Cocoa
  window instead of printing usage. `--help` is the way to get the usage text
  without a display.

## Follow-ups

- S1: add the two-line step above to `.github/workflows/ci.yml`
  (`ubuntu-latest`, before `cargo test --workspace`) and mark the test required.
- Activity Manager (`aether-taskmanager`) still launches as a CLI; add a
  `--window`/no-flag window path that reuses `ActivityManagerWindow`.
- Execute the Linux runtime test on CI or the device: it is the only part of
  this change this session could not execute (no Docker daemon, no Linux host).
