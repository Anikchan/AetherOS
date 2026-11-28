#!/bin/sh
# os-build/post-build.sh — Buildroot BR2_ROOTFS_POST_BUILD_SCRIPT for the
# AetherOS image (2026-11-28, UI image workstream).
#
# Buildroot runs this from the Buildroot top directory, after every package and
# after BR2_ROOTFS_OVERLAY has been copied, with $1 = $TARGET_DIR:
#
#   ../../post-build.sh output/target
#
# Why a post-build step instead of a Buildroot cargo package: the workspace
# needs per-package cargo features (aether-compositor must be built with
# --features drm) and resolves ~500 crates through path dependencies, which
# Buildroot's vendor-tarball cargo infrastructure does not model cleanly. This
# step builds the workspace on the build host, which is the same toolchain CI
# already validates, and installs the binaries into the target root. The image
# glibc (2.41 in Buildroot 2025.11) is newer than the Ubuntu 22.04 build host
# (2.35), so the binaries run unchanged. See os-build/buildroot/ui/README.md.
#
# Guards:
# - AETHER_SKIP_RUST_BUILD=1 skips the Rust build (image-only rebuilds).
# - A missing Rust toolchain, or a missing dev package, is reported as a
#   GitHub Actions annotation and in /etc/aether/ui-build.txt; only a total
#   failure to produce every `core` binary fails the build.

set -eu

TARGET_DIR="${1:?usage: post-build.sh <TARGET_DIR>}"
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
# Buildroot runs post-build scripts from its top directory, so $PWD/.config is
# the config it actually resolved (after defconfig + olddefconfig).
BUILDROOT_CONFIG="$PWD/.config"
LIST_FILE="$SCRIPT_DIR/buildroot/ui/binaries.txt"
MANIFEST_DIR="$TARGET_DIR/etc/aether"
MANIFEST="$MANIFEST_DIR/ui-build.txt"
BIN_DIR="$TARGET_DIR/usr/bin"
BUILD_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}/release"

log() { printf 'AETHER_UI_IMAGE: %s\n' "$*"; }
warn() {
    printf 'AETHER_UI_IMAGE_WARN: %s\n' "$*"
    printf '::warning title=AetherOS UI image::%s\n' "$*"
}
fail() {
    printf 'AETHER_UI_IMAGE_ERROR: %s\n' "$*"
    printf '::error title=AetherOS UI image::%s\n' "$*"
}

INSTALLED=""
MISSING=""
FAILED=""
COMPOSITOR_MODE="not-built"
RUSTC_VERSION="unavailable"
WIRING_GUARD="not-run"
RESULT="unknown"

mkdir -p "$MANIFEST_DIR"

write_manifest() {
    {
        printf 'aether_ui_image_manifest: 1\n'
        printf 'built_utc: %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
        printf 'rustc: %s\n' "$RUSTC_VERSION"
        printf 'profile: release (lto=false for the image build, strip=symbols)\n'
        printf 'compositor_features: %s\n' "$COMPOSITOR_MODE"
        printf 'wiring_guard: %s\n' "$WIRING_GUARD"
        printf 'installed: %s\n' "${INSTALLED:-none}"
        printf 'missing: %s\n' "${MISSING:-none}"
        printf 'build_failures: %s\n' "${FAILED:-none}"
        printf 'result: %s\n' "$RESULT"
    } >"$MANIFEST"

    log "manifest: $MANIFEST"
    log "RESULT: $RESULT"
    log "INSTALLED: ${INSTALLED:-none}"
    if [ -n "$MISSING" ]; then
        log "MISSING: $MISSING"
    fi
}

finish_error() {
    RESULT="failed"
    write_manifest
    fail "$1"
    exit 1
}

# --------------------------------------------------------------- rust toolchain ---

if [ "${AETHER_SKIP_RUST_BUILD:-0}" = "1" ]; then
    RESULT="skipped"
    log "AETHER_SKIP_RUST_BUILD=1: leaving the target root untouched"
    write_manifest
    exit 0
fi

# Buildroot exports its own PATH (host tools first); a runner-installed rustup
# lives in ~/.cargo/bin and has to be added explicitly.
case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) PATH="$HOME/.cargo/bin:$PATH" ;;
esac
export PATH

if ! command -v cargo >/dev/null 2>&1; then
    log "cargo not found; installing a minimal stable Rust toolchain with rustup"
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --profile minimal --default-toolchain stable >/dev/null 2>&1 || true
    fi
    if [ -x "$HOME/.cargo/bin/cargo" ]; then
        PATH="$HOME/.cargo/bin:$PATH"
        export PATH
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    finish_error "no Rust toolchain (cargo) available: the image would ship no aether-* binaries"
fi

RUSTC_VERSION="$(rustc --version 2>/dev/null || echo unavailable)"
log "using $RUSTC_VERSION"

# ------------------------------------------------------- build host dependencies ---

# Buildroot exports its own host tools on PATH, including its cross pkg-config
# wrapper, which points at the target staging sysroot. The Rust build must link
# against the *build host* libraries (exactly what ci.yml does), otherwise the
# -sys crates compile against target headers and link target .so files. Pin
# pkg-config to the system one and drop inherited cross settings *before* the
# dependency probe below, so the probe and the build agree.
if [ -x /usr/bin/pkg-config ]; then
    PKG_CONFIG=/usr/bin/pkg-config
    export PKG_CONFIG
fi
unset PKG_CONFIG_SYSROOT_DIR 2>/dev/null || true
unset PKG_CONFIG_LIBDIR 2>/dev/null || true
unset PKG_CONFIG_PATH 2>/dev/null || true

# The shell links fontconfig/freetype (Slint system fonts) and xkbcommon; the
# compositor's `drm` feature links libseat, libudev, libdrm, libgbm and
# libinput. ci.yml installs exactly this set for `cargo build --workspace`.
missing_pc() {
    pc="${PKG_CONFIG:-pkg-config}"
    command -v "$pc" >/dev/null 2>&1 || {
        echo pkg-config
        return 0
    }
    for module in fontconfig freetype2 xkbcommon libudev libseat libdrm gbm libinput; do
        "$pc" --exists "$module" 2>/dev/null || echo "$module"
    done
    return 0
}

MISSING_PC="$(missing_pc | tr '\n' ' ')"
if [ -n "$MISSING_PC" ]; then
    log "pkg-config modules missing: $MISSING_PC"
    if command -v apt-get >/dev/null 2>&1 && command -v sudo >/dev/null 2>&1; then
        log "installing build-host dev packages (same set as ci.yml)"
        if sudo apt-get install -y --no-install-recommends \
            pkg-config libfontconfig1-dev libfreetype-dev libxkbcommon-dev \
            libudev-dev libseat-dev libdrm-dev libgbm-dev libinput-dev \
            libegl-dev libgles-dev libwayland-dev libpixman-1-dev \
            >/dev/null 2>&1; then
            log "dev packages installed"
        else
            warn "could not install dev packages; the build below may fall back"
        fi
    else
        warn "no apt-get/sudo: assuming the build host already has the dev packages"
    fi
fi

# ---------------------------------------------------------------------- build ---

cd "$REPO_ROOT"

# `lto = true` in the workspace release profile costs 10+ minutes on a 2-core
# runner and would push the 90-minute `make` step over its timeout. The image
# binaries keep the same optimisations and stripping, only the link-time
# optimizer is off.
CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_LTO
CARGO_TERM_COLOR=never
export CARGO_TERM_COLOR
CARGO_NET_RETRY=5
export CARGO_NET_RETRY

COMPOSITOR_MODE="drm"
log "building aether-compositor with --features drm"
# The drm feature is mandatory for the image: a headless-only compositor cannot
# open /dev/dri, so the first boot would show a failed session (2026-11-28).
# The error is kept and printed, because the previous silent fallback shipped
# exactly that broken image.
if ! cargo build --release -p aether-compositor --features drm 2>&1 | tee /tmp/aether-compositor-build.log; then
    COMPOSITOR_MODE="not-built"
    FAILED="$FAILED aether-compositor"
    fail "aether-compositor --features drm failed; the image would have no KMS compositor"
    warn "last lines of the build log:"
    tail -20 /tmp/aether-compositor-build.log | while IFS= read -r line; do warn "  $line"; done
fi
if [ "$COMPOSITOR_MODE" = "drm" ]; then
    log "aether-compositor built (features: drm)"
fi

log "building aether-shell (also provides aether-greeter)"
if ! cargo build --release -p aether-shell; then
    FAILED="$FAILED aether-shell"
    fail "aether-shell failed to build; aether-session.service will have no shell"
fi

# Apps: one cargo invocation for all of them (dependencies are shared with the
# shell) and then a copy of whatever exists. A broken app must not break the
# image.
APP_PACKAGES="$(awk -F '\t' '$3 == "app" { print $1 }' "$LIST_FILE" | sort -u)"
if [ -n "$APP_PACKAGES" ]; then
    APP_ARGS=""
    for package in $APP_PACKAGES; do
        APP_ARGS="$APP_ARGS -p $package"
    done
    log "building apps:$APP_ARGS"
    # shellcheck disable=SC2086 # APP_ARGS is a deliberate -p list
    if ! cargo build --release $APP_ARGS; then
        warn "at least one app failed to build; continuing with the binaries that exist"
        FAILED="$FAILED apps"
    fi
fi

# ---------------------------------------------------------------- install ---

log "installing binaries from $LIST_FILE into $BIN_DIR"
mkdir -p "$BIN_DIR"
while IFS="$(printf '\t')" read -r package binary kind; do
    case "$package" in
        '' | '#'*) continue ;;
    esac
    [ -n "${binary:-}" ] || continue
    if [ -f "$BUILD_DIR/$binary" ]; then
        cp -f "$BUILD_DIR/$binary" "$BIN_DIR/$binary"
        chmod 0755 "$BIN_DIR/$binary"
        INSTALLED="$INSTALLED $binary"
    else
        MISSING="$MISSING $binary"
    fi
done <"$LIST_FILE"

if [ -z "$MISSING" ]; then
    RESULT="ok"
else
    RESULT="partial"
fi

# Fail only when the image would ship none of the session binaries at all:
# that is the regression this step exists to prevent.
SESSION_BINARIES=0
for binary in aether-compositor aether-shell aether-greeter; do
    if [ -f "$BIN_DIR/$binary" ]; then
        SESSION_BINARIES=$((SESSION_BINARIES + 1))
    fi
done
if [ "$SESSION_BINARIES" -eq 0 ]; then
    RESULT="failed"
    write_manifest
    fail "no session binary installed (aether-compositor, aether-shell, aether-greeter all missing)"
    exit 1
fi

log "session binaries installed: $SESSION_BINARIES/3"

# Prototype artefacts the session needs at runtime: the wrappers and the unit
# files come from the rootfs overlay, so only their presence is asserted here.
for path in usr/bin/aether-session usr/bin/aether-shell-host \
    etc/systemd/system/aether-session.service \
    etc/systemd/system/aether-shell.service; do
    if [ -e "$TARGET_DIR/$path" ]; then
        log "present: /$path"
    else
        warn "/$path missing from the rootfs overlay"
    fi
done

# ------------------------------- image wiring + defconfig silent-drop guard ---
# The Lua/WirePlumber bug (image build 46041689625: "Build wireplumber : NO"
# because BR2_PACKAGE_LUA was off and BR2_PACKAGE_LUA_5_4 was therefore dropped)
# is invisible in a normal build log. This runs the static checker against the
# .config Buildroot actually resolved, so any defconfig `=y` symbol that
# olddefconfig deleted is reported here, once per image build.
GUARD="$SCRIPT_DIR/scripts/check-ui-image.sh"
if [ -f "$GUARD" ] && [ -f "$BUILDROOT_CONFIG" ]; then
    GUARD_LOG="$(mktemp)"
    if sh "$GUARD" --config "$BUILDROOT_CONFIG" >"$GUARD_LOG" 2>&1; then
        WIRING_GUARD="ok"
        log "wiring guard: check-ui-image.sh passed (defconfig drops + unit wiring)"
    else
        WIRING_GUARD="failed"
        warn "wiring guard failed: check-ui-image.sh reported failures (defconfig drop or unit wiring)"
        grep 'FAIL' "$GUARD_LOG" | while IFS= read -r line; do
            warn "guard: $line"
        done
    fi
    rm -f "$GUARD_LOG"
elif [ ! -f "$GUARD" ]; then
    WIRING_GUARD="checker-missing"
    warn "wiring guard not run: $GUARD is missing"
else
    WIRING_GUARD="no-config"
    warn "wiring guard not run: $BUILDROOT_CONFIG not found (call it with --config output/.config)"
fi

write_manifest
exit 0
