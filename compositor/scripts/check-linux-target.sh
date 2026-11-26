#!/usr/bin/env bash
# Type-check the DRM/KMS + libinput backend for the device target from the
# macOS dev host (P3-T09).
#
# The backend is Linux-only and hardware-gated: this script never runs it, it
# only proves the code compiles for x86_64-unknown-linux-gnu, which is the gate
# the task asks for. On the Dell / Linux CI use plain
# `cargo check -p aether-compositor --features drm`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

# The Homebrew rustc on this host ships no Linux std; the rustup toolchain does.
if command -v rustup >/dev/null 2>&1; then
  toolchain_bin="$HOME/.rustup/toolchains/$(rustup show active-toolchain | awk '{print $1}')/bin"
  if [ -d "$toolchain_bin" ]; then
    export PATH="$toolchain_bin:$PATH"
  fi
fi

# `libudev-sys` probes pkg-config from its build script even for a type-check.
# Its bindings are pre-generated and `cargo check` does not link, so a stub .pc
# is sufficient and no Linux sysroot is needed.
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR="$root/pc-stubs"

exec cargo check -p aether-compositor --target x86_64-unknown-linux-gnu --features drm "$@"
