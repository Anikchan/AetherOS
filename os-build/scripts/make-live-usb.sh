#!/usr/bin/env bash
# P8-T01 — build the AetherOS live USB image.
#
# The script assembles a GPT disk image from the Buildroot artifacts without
# root and without loop mounts, so it also runs inside CI:
#
#   p1  64 MiB FAT32 ESP      GRUB2 EFI + bzImage + live grub.cfg
#   p2  rootfs.squashfs       read-only live root (label AETHEROS-ROOT)
#   p3  persistence ext4      writable overlay store (label AETHEROS-PERSIST)
#
# Usage:
#   make-live-usb.sh [--binaries DIR] [--out FILE] [--persist-mb N]
#                    [--rootfs FILE] [--check] [--dry-run]
#
# Linux host tools: sgdisk (gdisk), mkfs.vfat + mcopy (dosfstools/mtools),
# mke2fs (e2fsprogs), truncate, dd. The finished image boots in QEMU with
# `os-build/test/disk-image-test.sh <image> uefi` and can be written to a USB
# stick with `dd if=<image> of=/dev/sdX bs=4M status=progress`.
#
# BIOS boot is not part of P8-T01 (the UEFI chain is the shipped live path);
# `aetheros-bios.img` from `os-build/board` remains the P1 legacy test image.

set -euo pipefail

BINARIES_DIR="${BINARIES_DIR:-$(cd "$(dirname "$0")/../buildroot/output/images" 2>/dev/null && pwd || echo "")}"
OUT=""
ROOTFS=""
PERSIST_MB=${PERSIST_MB:-1024}
ESP_MB=64
DRY_RUN=0
CHECK_ONLY=0

usage() {
  sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while (($#)); do
  case "$1" in
    --binaries) BINARIES_DIR="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --persist-mb) PERSIST_MB="$2"; shift 2 ;;
    --rootfs) ROOTFS="$2"; shift 2 ;;
    --check) CHECK_ONLY=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage 0 ;;
    *) echo "unknown argument: $1" >&2; usage 2 ;;
  esac
done

[ -n "$BINARIES_DIR" ] && [ -d "$BINARIES_DIR" ] || {
  echo "ERROR: --binaries DIR is required (Buildroot output/images)" >&2
  exit 2
}
[ -n "$OUT" ] || OUT="$BINARIES_DIR/aetheros-live.img"
[ -n "$ROOTFS" ] || ROOTFS="$BINARIES_DIR/rootfs.squashfs"

for artifact in "$BINARIES_DIR/bzImage" "$ROOTFS"; do
  [ -f "$artifact" ] || {
    echo "ERROR: missing $artifact (run the Buildroot build first)" >&2
    exit 1
  }
done
ESP_SOURCE="$BINARIES_DIR/efi-part/EFI/BOOT/BOOTX64.EFI"
[ -f "$ESP_SOURCE" ] || {
  echo "ERROR: $ESP_SOURCE is missing; post-image.sh must run before the live image" >&2
  exit 1
}

# Deterministic partition UUIDs so grub.cfg and the docs never drift.
ESP_UUID="a0e50001-0000-4000-8000-000000000001"
ROOT_UUID="a0e50002-0000-4000-8000-000000000002"
PERSIST_UUID="a0e50003-0000-4000-8000-000000000003"

ROOTFS_BYTES=$(wc -c <"$ROOTFS" | tr -d ' ')
ROOTFS_SECTORS=$(( (ROOTFS_BYTES + 511) / 512 ))
ESP_SECTORS=$(( ESP_MB * 1024 * 1024 / 512 ))
PERSIST_SECTORS=$(( PERSIST_MB * 1024 * 1024 / 512 ))
ALIGN=2048
P1_START=$ALIGN
P1_END=$(( P1_START + ESP_SECTORS - 1 ))
P2_START=$(( ((P1_END + 1 + ALIGN - 1) / ALIGN) * ALIGN ))
P2_END=$(( P2_START + ROOTFS_SECTORS - 1 ))
P3_START=$(( ((P2_END + 1 + ALIGN - 1) / ALIGN) * ALIGN ))
P3_END=$(( P3_START + PERSIST_SECTORS - 1 ))
IMG_SECTORS=$(( P3_END + 1 + 34 + ALIGN ))
IMG_BYTES=$(( IMG_SECTORS * 512 ))

cat <<EOF
AetherOS live USB plan
  binaries:   $BINARIES_DIR
  output:     $OUT
  p1 ESP:     ${ESP_MB} MiB @ sector $P1_START
  p2 root:    $((ROOTFS_SECTORS * 512 / 1024 / 1024)) MiB squashfs @ sector $P2_START
  p3 persist: ${PERSIST_MB} MiB ext4 @ sector $P3_START
  image:      $((IMG_BYTES / 1024 / 1024)) MiB
EOF
if [ "$DRY_RUN" = "1" ]; then
  echo "LIVE_USB_DRY_RUN output=$OUT"
  exit 0
fi

missing=()
for tool in sgdisk mkfs.vfat mcopy mke2fs truncate dd awk; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if ((${#missing[@]})); then
  echo "Missing tools: ${missing[*]}" >&2
  echo "  Debian/Ubuntu: sudo apt-get install gdisk dosfstools mtools e2fsprogs" >&2
  echo "  Fedora: sudo dnf install gdisk dosfstools mtools e2fsprogs" >&2
  exit 1
fi
if [ "$CHECK_ONLY" = "1" ]; then
  echo "LIVE_USB_TOOLS_OK binaries=$BINARIES_DIR"
  exit 0
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/aether-live.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

# --- p1: ESP ---------------------------------------------------------------
mkdir -p "$WORK/esp/EFI/BOOT" "$WORK/esp/boot/grub"
cp "$ESP_SOURCE" "$WORK/esp/EFI/BOOT/BOOTX64.EFI"
cp "$BINARIES_DIR/bzImage" "$WORK/esp/bzImage"
cat >"$WORK/esp/boot/grub/grub.cfg" <<GRUB
set timeout=5
set default=0
serial --unit=0 --speed=115200
terminal_input serial console
terminal_output serial console

menuentry "AetherOS Live" {
    linux /bzImage root=PARTUUID=${ROOT_UUID} rootfstype=squashfs rootwait \\
          live=1 persistence=PARTUUID=${PERSIST_UUID} earlycon=efi \\
          console=tty0 console=ttyS0,115200n8 net.ifnames=0 biosdevname=0
}

menuentry "AetherOS Live (safe graphics)" {
    linux /bzImage root=PARTUUID=${ROOT_UUID} rootfstype=squashfs rootwait \\
          live=1 persistence=PARTUUID=${PERSIST_UUID} nomodeset \\
          console=tty0 console=ttyS0,115200n8 net.ifnames=0
}
GRUB
ESP_IMG="$WORK/esp.vfat"
truncate -s "$((ESP_MB))M" "$ESP_IMG"
mkfs.vfat -F 32 -n AETHEROS "$ESP_IMG" >/dev/null
MTOOLS_SKIP_CHECK=1 mcopy -s -i "$ESP_IMG" "$WORK/esp/EFI" "::/"
MTOOLS_SKIP_CHECK=1 mcopy -s -i "$ESP_IMG" "$WORK/esp/boot" "::/"
MTOOLS_SKIP_CHECK=1 mcopy -i "$ESP_IMG" "$WORK/esp/bzImage" "::/"

# --- p3: persistence -------------------------------------------------------
mkdir -p "$WORK/persist/.aether-persistence"
cat >"$WORK/persist/.aether-persistence/README" <<'TXT'
AetherOS live persistence. The live overlay service mounts this filesystem and
stores user data, app state and updates here. Remove this directory to reset.
TXT
PERSIST_IMG="$WORK/persist.ext4"
truncate -s "$((PERSIST_MB))M" "$PERSIST_IMG"
mke2fs -q -F -t ext4 -L AETHEROS-PERSIST -d "$WORK/persist" "$PERSIST_IMG"

# --- assemble the GPT image ------------------------------------------------
rm -f "$OUT"
truncate -s "$IMG_BYTES" "$OUT"
sgdisk --clear \
  --set-alignment="$ALIGN" \
  --new=1:${P1_START}:${P1_END} --typecode=1:ef00 --change-name=1:AETHEROS-ESP --partition-guid=1:${ESP_UUID} \
  --new=2:${P2_START}:${P2_END} --typecode=2:8300 --change-name=2:AETHEROS-ROOT --partition-guid=2:${ROOT_UUID} \
  --new=3:${P3_START}:${P3_END} --typecode=3:8300 --change-name=3:AETHEROS-PERSIST --partition-guid=3:${PERSIST_UUID} \
  "$OUT" >/dev/null

dd if="$ESP_IMG" of="$OUT" bs=512 seek="$P1_START" conv=notrunc status=none
dd if="$ROOTFS" of="$OUT" bs=512 seek="$P2_START" conv=notrunc status=none
dd if="$PERSIST_IMG" of="$OUT" bs=512 seek="$P3_START" conv=notrunc status=none

echo "LIVE_USB_BUILT output=$OUT bytes=$IMG_BYTES"
echo "Write it to a USB stick: dd if=$OUT of=/dev/sdX bs=4M status=progress"
