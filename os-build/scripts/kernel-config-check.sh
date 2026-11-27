#!/usr/bin/env bash
# kernel-config-check.sh — local/CI reimplementation of kernel-config.yml that
# uses the cached kernel tarball (fetch-kernel.sh) instead of re-downloading.
#
# This is an OPTIMIZATION PROPOSAL for S1 (who owns kernel-config.yml). S4
# cannot edit that file per the parallel-session territory rules, so this script
# is provided as a drop-in replacement for the workflow's "Download upstream
# kernel" + "Merge defconfig + fragment" steps.
#
# Usage: kernel-config-check.sh [version]   (default: 6.18)

set -euo pipefail

VERSION="${1:-6.18}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OS_BUILD_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$OS_BUILD_DIR/.." && pwd)"
CACHE_DIR="$OS_BUILD_DIR/.cache/kernel"
TARBALL="$CACHE_DIR/linux-${VERSION}.tar.xz"

# Ensure cached tarball exists
if [ ! -f "$TARBALL" ]; then
  "$SCRIPT_DIR/fetch-kernel.sh" "$VERSION"
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# The kernel build requires GNU Make >= 4.0; macOS ships 3.81, so prefer gmake.
MAKE_BIN=make
if ! make --version 2>/dev/null | head -1 | grep -qE 'Make (4|5|6)\.'; then
  command -v gmake >/dev/null 2>&1 && MAKE_BIN=gmake
fi

echo "Extracting $TARBALL ..."
tar xf "$TARBALL" -C "$WORK"
KDIR="$WORK/linux-${VERSION}"
cd "$KDIR"

echo "Merging defconfig + fragment ..."
# Pin the target arch: on an arm64/macOS host Kbuild would otherwise detect arm64
# and merge the fragment into an arm64 tree.
export ARCH=x86_64
# Kbuild aborts with "Sorry, this linker is not supported" when `ld --version` is
# not GNU-style (Apple's ld). Only config targets run here, nothing is linked, so
# answer the version probe with a stub that forwards everything else.
LD_ARG=""
if ! "${CROSS_COMPILE:-}ld" --version 2>/dev/null | head -1 | grep -q "GNU ld"; then
  printf '#!/bin/sh\ncase "$1" in --version) echo "GNU ld (stub for kconfig merge) 2.40";;\n*) exec ld "$@";;\nesac\n' > "$WORK/ld-stub"
  chmod +x "$WORK/ld-stub"
  LD_ARG="LD=$WORK/ld-stub"
  echo "note: non-GNU ld detected, using a stub for Kbuild's version probe"
fi
# `olddefconfig` needs an existing .config, so start from the x86_64 defconfig.
if ! "$MAKE_BIN" ${LD_ARG:+"$LD_ARG"} defconfig >/dev/null 2>&1; then
  echo "kernel-config-check: could not run kconfig - needs GNU make >= 4 (brew install make)" >&2
  exit 2
fi
# merge_config.sh needs GNU sed; macOS ships BSD sed. Appending the fragment and
# running olddefconfig is the same merge for kconfig (later assignments win).
if ! "$KDIR/scripts/kconfig/merge_config.sh" -m .config "$OS_BUILD_DIR/kernel/aether.fragment" >/dev/null 2>&1; then
  echo "merge_config.sh unavailable (needs GNU sed) - appending fragment instead"
  cat "$OS_BUILD_DIR/kernel/aether.fragment" >> .config
fi
"$MAKE_BIN" ${LD_ARG:+"$LD_ARG"} olddefconfig >/dev/null 2>&1

echo "Verifying required options ..."
fail=0
for opt in PSI CGROUPS CGROUP_FREEZER MEMCG ZRAM SQUASHFS EXT4_FS BTRFS_FS EFI EFI_STUB DRM DRM_AMDGPU NO_HZ_IDLE SUSPEND VIRTIO ANDROID_BINDER_IPC; do
  grep -q "^CONFIG_${opt}=y" .config || { echo "MISSING: CONFIG_${opt}"; fail=1; }
done
# DRM_I915 may be built-in (=y) or a module (=m); both are valid.
grep -qE "^CONFIG_DRM_I915=(y|m)" .config || { echo "MISSING: CONFIG_DRM_I915"; fail=1; }
grep -q '^CONFIG_ANDROID_BINDER_DEVICES="binder,hwbinder,vndbinder"' .config || { echo "MISSING: binder devices"; fail=1; }

# P2 hardware-bring-up options (P2 run 3 fixes: regulatory.db needs cfg80211 as
# a module, touchpad IRQ needs the Tiger Lake pinctrl/GPIO block).
# Format: SYMBOL=expected-value. Documented in
# docs/2026/11/27/results/kernel-input-regdb-touchpad.md.
for want in \
  CFG80211=m MAC80211=m IWLWIFI=m ATH10K=m ATH10K_PCI=m \
  PINCTRL=y PINCTRL_TIGERLAKE=y GPIOLIB=y GPIO_ACPI=y GPIOLIB_IRQCHIP=y \
  I2C_HID=m I2C_HID_ACPI=m I2C_HID_CORE=m HID_MULTITOUCH=y \
  I2C_DESIGNWARE_PLATFORM=y; do
  grep -qx "CONFIG_${want}" .config || { echo "MISSING: CONFIG_${want}"; fail=1; }
done
# cfg80211 must not become built-in again: the regdb firmware is only reachable
# from module-load context, after the rootfs is mounted.
grep -q "^CONFIG_CFG80211=y" .config && { echo "REGRESSION: CONFIG_CFG80211=y (must be =m for regulatory.db)"; fail=1; }

if [ "$fail" = "1" ]; then
  echo "kernel-config-check: FAIL"
  exit 1
fi
echo "kernel-config-check: OK (all required options present)"