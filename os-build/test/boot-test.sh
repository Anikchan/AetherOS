#!/usr/bin/env bash
# boot-test.sh — P0-T06 boot test for the Linux test environment (QEMU on macOS).
#
# Boots an x86-64 Linux ISO in QEMU in BIOS (SeaBIOS) or UEFI (edk2/OVMF) mode
# using direct kernel boot with a serial console, and passes if the kernel
# reaches the target's login prompt within the timeout.
#
# Usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]
#
# Results are printed in the D6 results-log format (addendum 1 D6 section 7).

set -euo pipefail

ISO="${1:?usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]}"
MODE="${2:?usage: boot-test.sh <iso> <bios|uefi> [timeout-seconds]}"
TIMEOUT="${3:-180}"

QEMU="qemu-system-x86_64"
QEMU_SHARE="$(brew --prefix qemu)/share/qemu"
WORK="$(mktemp -d)"
LOG="$WORK/serial.log"
trap 'kill "${QEMU_PID:-}" 2>/dev/null || true; rm -rf "$WORK"' EXIT

# 1. Extract kernel and initramfs from the ISO (7zz; hdiutil cannot mount ISO9660).
SEVENZIP="$(command -v 7zz || echo 7zz)"
"$SEVENZIP" x -o"$WORK/extract" "$ISO" >/dev/null
KERNEL="$WORK/extract/boot/vmlinuz-virt"
INITRD="$WORK/extract/boot/initramfs-virt"
[ -f "$KERNEL" ] || { echo "FAIL: kernel not found in ISO"; exit 2; }
cp "$KERNEL" "$WORK/vmlinuz"
cp "$INITRD" "$WORK/initramfs"

# 2. Build the QEMU command for the requested firmware mode.
QEMU_ARGS=(-m 512 -nographic -no-reboot
  -kernel "$WORK/vmlinuz" -initrd "$WORK/initramfs"
  -append "console=ttyS0 modules=loop,squashfs,sd-mod,usb-storage"
  -cdrom "$ISO")
if [ "$MODE" = "uefi" ]; then
  OVMF="$QEMU_SHARE/edk2-x86_64-code.fd"
  [ -f "$OVMF" ] || { echo "FAIL: OVMF firmware not found at $OVMF"; exit 2; }
  # QEMU 11 rejects the edk2 images via -bios; pflash is the supported interface.
  QEMU_ARGS+=(-drive if=pflash,format=raw,readonly=on,file="$OVMF")
fi

# 3. Boot and watch the serial log for the login prompt.
"$QEMU" "${QEMU_ARGS[@]}" >"$LOG" 2>&1 &
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

# 4. Report in the D6 results-log format.
echo "Gate: P0-T06 Boot test ($MODE)"
echo "Date: $(date +%Y-%m-%d)"
echo "Machine: $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.memsize | awk '{print $1/1073741824 " GB RAM"}'), macOS $(sw_vers -productVersion), QEMU TCG emulation"
echo "Kernel: $(file "$WORK/vmlinuz" | sed 's/.*version /linux /;s/,.*//')"
echo "Command: boot-test.sh $ISO $MODE $TIMEOUT"
echo "Values: login prompt ${RESULT} within ${TIMEOUT}s budget; serial log lines: $(wc -l <"$LOG" | tr -d ' ')"
echo "Result: $RESULT"
if [ "$RESULT" != "Pass" ]; then
  echo "Notes: serial log tail follows."
  tail -30 "$LOG"
fi
[ "$RESULT" = "Pass" ]
