#!/usr/bin/env bash
# boot-test.sh — P0-T06 boot test for the Linux test environment (QEMU on macOS).
#
# Boots an x86-64 Linux ISO in QEMU in BIOS (SeaBIOS) or UEFI (edk2/OVMF) mode
# using direct kernel boot with a serial console, and passes if the kernel
# reaches the target's login prompt within the timeout.
#
# Usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]
#
# Environment:
#   QEMU_BIN        override qemu binary (default: qemu-system-x86_64)
#   QEMU_SHARE      override QEMU share dir (default: $(brew --prefix qemu)/share/qemu)
#   KEEP_WORK=1     keep the temp work dir for debugging
#   EXTRA_APPEND    extra kernel cmdline tokens
#
# Results are printed in the D6 results-log format (addendum 1 D6 section 7).

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]

Environment:
  QEMU_BIN       qemu binary (default qemu-system-x86_64)
  QEMU_SHARE     QEMU share dir
  KEEP_WORK=1    keep temp work dir for debugging
  EXTRA_APPEND   extra kernel cmdline tokens
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

ISO="${1:?usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]}"
MODE="${2:?usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]}"
TIMEOUT="${3:-180}"

case "$MODE" in
  bios|uefi) ;;
  *) echo "FAIL: MODE must be 'bios' or 'uefi' (got '$MODE')" >&2; exit 2 ;;
esac
[ -f "$ISO" ] || { echo "FAIL: ISO not found: $ISO" >&2; exit 2; }

QEMU_BIN="${QEMU_BIN:-qemu-system-x86_64}"
QEMU_SHARE="${QEMU_SHARE:-$(brew --prefix qemu 2>/dev/null)/share/qemu}"
command -v "$QEMU_BIN" >/dev/null 2>&1 || { echo "FAIL: qemu not found: $QEMU_BIN" >&2; exit 2; }

SEVENZIP="$(command -v 7zz || command -v 7z || true)"
[ -n "$SEVENZIP" ] || { echo "FAIL: 7zz/7z not found (brew install sevenzip)" >&2; exit 2; }

WORK="$(mktemp -d)"
LOG="$WORK/serial.log"
trap 'if [ "${KEEP_WORK:-0}" != "1" ]; then rm -rf "$WORK"; fi' EXIT

START_TS=$(date +%s)

# 1. Extract kernel and initramfs from the ISO (7zz; hdiutil cannot mount ISO9660).
mkdir -p "$WORK/extract"
"$SEVENZIP" x -y -o"$WORK/extract" "$ISO" >/dev/null 2>&1 || {
  echo "FAIL: could not extract ISO: $ISO" >&2
  exit 2
}

KERNEL="$WORK/extract/boot/vmlinuz-virt"
INITRD="$WORK/extract/boot/initramfs-virt"
# Fallback names
[ -f "$KERNEL" ] || KERNEL="$(find "$WORK/extract" -name 'vmlinuz*' | head -1)"
[ -f "$INITRD" ] || INITRD="$(find "$WORK/extract" -name 'initramfs*' -o -name 'initrd*' | head -1)"

[ -f "$KERNEL" ] || { echo "FAIL: kernel not found in ISO" >&2; exit 2; }
cp "$KERNEL" "$WORK/vmlinuz"
if [ -n "$INITRD" ] && [ -f "$INITRD" ]; then
  cp "$INITRD" "$WORK/initramfs"
  HAVE_INITRD=1
else
  HAVE_INITRD=0
fi

# 2. Build the QEMU command for the requested firmware mode.
QEMU_ARGS=(-m 512 -nographic -no-reboot
  -kernel "$WORK/vmlinuz"
  -append "console=ttyS0 modules=loop,squashfs,sd-mod,usb-storage ${EXTRA_APPEND:-}"
  -cdrom "$ISO")
if [ "$HAVE_INITRD" = "1" ]; then
  QEMU_ARGS+=(-initrd "$WORK/initramfs")
fi
if [ "$MODE" = "uefi" ]; then
  OVMF="$QEMU_SHARE/edk2-x86_64-code.fd"
  [ -f "$OVMF" ] || { echo "FAIL: OVMF firmware not found at $OVMF" >&2; exit 2; }
  # QEMU 11 rejects the edk2 images via -bios; pflash is the supported interface.
  QEMU_ARGS+=(-drive if=pflash,format=raw,readonly=on,file="$OVMF")
fi

# 3. Boot and watch the serial log for the login prompt.
"$QEMU_BIN" "${QEMU_ARGS[@]}" >"$LOG" 2>&1 &
QEMU_PID=$!

DEADLINE=$((SECONDS + TIMEOUT))
RESULT="Fail"
while [ $SECONDS -lt $DEADLINE ]; do
  if ! kill -0 "$QEMU_PID" 2>/dev/null; then
    break
  fi
  if grep -qiE "login:|Welcome to Alpine" "$LOG" 2>/dev/null; then
    RESULT="Pass"
    break
  fi
  sleep 2
done
kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

END_TS=$(date +%s)
ELAPSED=$((END_TS - START_TS))

# 4. Report in the D6 results-log format.
echo "Gate: P0-T06 Boot test ($MODE)"
echo "Date: $(date +%Y-%m-%d)"
echo "Machine: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown), $( (sysctl -n hw.memsize 2>/dev/null || echo 0) | awk '{print $1/1073741824 " GB RAM"}'), macOS $(sw_vers -productVersion 2>/dev/null || echo unknown), QEMU TCG emulation"
echo "Kernel: $(file "$WORK/vmlinuz" | sed 's/.*version /linux /;s/,.*//')"
echo "Command: boot-test.sh $ISO $MODE $TIMEOUT"
echo "Values: login prompt ${RESULT} within ${TIMEOUT}s budget; elapsed ${ELAPSED}s; serial log lines: $(wc -l <"$LOG" | tr -d ' ')"
if [ "${KEEP_WORK:-0}" = "1" ]; then
  echo "Work dir kept: $WORK"
fi
echo "Result: $RESULT"
if [ "$RESULT" != "Pass" ]; then
  echo "Notes: serial log tail follows."
  tail -30 "$LOG"
fi
[ "$RESULT" = "Pass" ]