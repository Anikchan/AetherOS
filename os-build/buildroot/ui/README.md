# os-build/buildroot/ui — how the graphical session gets into the image

Written 2026-11-28 by the `ui-image` workstream. This directory holds the
contract between the Buildroot image and the Rust workspace: **which binaries
the image carries and where they are installed from**. It deliberately does not
contain a Buildroot package (see "Why there is no package" below).

Files:

| File | Purpose |
| --- | --- |
| `binaries.txt` | One tab-separated line per installed binary: cargo package, installed binary, `core` or `app`. Read by `os-build/post-build.sh` and asserted by `os-build/scripts/check-ui-image.sh`. |
| `README.md` | This file. |

## The route used tonight: post-build, not a cargo package

`aetheros_defconfig` sets

```
BR2_ROOTFS_POST_BUILD_SCRIPT="../../post-build.sh"
```

so Buildroot runs `os-build/post-build.sh $TARGET_DIR` after every package and
after the rootfs overlay, and before the filesystem images are made. That script

1. makes sure a Rust toolchain exists on the build host (rustup, minimal stable,
   installed only if `cargo` is missing),
2. installs the same dev packages `ci.yml` uses (`libfontconfig1-dev`,
   `libfreetype-dev`, `libxkbcommon-dev`, `libudev-dev`, `libseat-dev`,
   `libdrm-dev`, `libgbm-dev`, `libinput-dev`),
3. runs `cargo build --release` for `aether-compositor --features drm`,
   `aether-shell` (which also produces `aether-greeter`) and the app crates,
4. copies each binary from `<repo>/target/release/` to `$TARGET_DIR/usr/bin/`,
5. writes `/etc/aether/ui-build.txt` in the image (toolchain, features,
   installed and missing binaries) and prints `AETHER_UI_IMAGE*` markers plus
   GitHub annotations.

### Why not Buildroot's cargo infrastructure

- `aether-compositor` must be built with `--features drm`; the other crates
  must not be. Buildroot's `cargo-package` infra builds one package with one
  feature set and does not model a 25-member workspace with path dependencies
  cleanly (its cargo support expects a self-contained vendored crate set).
- The workspace needs `fontconfig`, `freetype`, `libseat`, `libgbm`, `libdrm`,
  `libinput` and `libudev` at *build* time, i.e. a full host dependency set
  inside Buildroot plus `BR2_PACKAGE_HOST_RUSTC` and a rustup target for the
  target triple. That is a large, unverifiable change from the macOS dev host.
- The image glibc (Buildroot 2025.11, glibc 2.41) is newer than the Ubuntu
  22.04 build host (glibc 2.35), so host-built binaries load on the device.
  The trade-off is that the image binaries are not rebuilt by the cross
  toolchain; when the image gains a C library bump *below* 2.35, this route has
  to become a real package.

### Migrating to a real Buildroot package later

The migration is contained: replace the `BR2_ROOTFS_POST_BUILD_SCRIPT` line with
a `$(eval $(cargo-package))` recipe under this directory, carry `binaries.txt`
over as the install list (`AETHER_UI_CARGO_INSTALL_BINS` or an explicit install
loop), and keep `aether-session.service` / `aether-shell.service` unchanged
because they only reference `/usr/bin/<binary>`.

## What must stay true

- `binaries.txt` binary names equal the app ids the launcher spawns
  (`/usr/bin/<id>`, `shell/src/model.rs`, `shell/src/search.rs`).
- The three `core` entries exist: `aether-compositor`, `aether-shell`,
  `aether-greeter`. If none of them can be produced, `post-build.sh` fails the
  image build instead of shipping the old CLI-only image silently.
- `os-build/scripts/check-ui-image.sh` passes. It runs on macOS (POSIX `sh`,
  no Buildroot needed) and asserts the defconfig symbols, the unit wiring and
  this list against `post-build.sh`.
