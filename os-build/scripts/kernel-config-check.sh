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

echo "Extracting $TARBALL ..."
tar xf "$TARBALL" -C "$WORK"
KDIR="$WORK/linux-${VERSION}"
cd "$KDIR"

echo "Merging defconfig + fragment ..."
make olddefconfig >/dev/null 2>&1
"$KDIR/scripts/kconfig/merge_config.sh" -m .config "$OS_BUILD_DIR/kernel/aether.fragment" >/dev/null
make olddefconfig >/dev/null 2>&1

echo "Verifying required options ..."
fail=0
for opt in PSI CGROUPS CGROUP_FREEZER MEMCG ZRAM SQUASHFS EXT4_FS BTRFS_FS EFI EFI_STUB DRM DRM_I915 DRM_AMDGPU NO_HZ_IDLE SUSPEND VIRTIO ANDROID_BINDER_IPC; do
  grep -q "^CONFIG_${opt}=y" .config || { echo "MISSING: CONFIG_${opt}"; fail=1; }
done
grep -q '^CONFIG_ANDROID_BINDER_DEVICES="binder,hwbinder,vndbinder"' .config || { echo "MISSING: binder devices"; fail=1; }

if [ "$fail" = "1" ]; then
  echo "kernel-config-check: FAIL"
  exit 1
fi
echo "kernel-config-check: OK (all required options present)"