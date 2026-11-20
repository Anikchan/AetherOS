#!/usr/bin/env bash
# disk-image-test.sh — boot a built AetherOS disk image in QEMU and check that
# it reaches a login prompt (or a caller-supplied marker).
#
# Usage: disk-image-test.sh <image> <bios|uefi> [timeout-seconds] [marker]
#
# <image> is a raw disk image produced by the build (aetheros-efi.img or
# aetheros-bios.img). The image is booted as a raw drive; firmware mode must
# match the image's boot chain (uefi for ESP/UKI, bios for MBR/GRUB).
#
# Environment:
#   QEMU_BIN     qemu binary (default qemu-system-x86_64)
#   QEMU_SHARE   QEMU share dir
#   KEEP_WORK=1  keep temp work dir
#   MARKER       override success marker (regex, default "login:|Welcome")

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: disk-image-test.sh <image> <bios|uefi> [timeout-seconds] [marker]

Environment:
  QEMU_BIN     qemu binary (default qemu-system-x86_64)
  QEMU_SHARE   QEMU share dir
  KEEP_WORK=1  keep temp work dir
  MARKER       success marker regex (default "login:|Welcome")
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

IMAGE="${1:?usage: disk-image-test.sh <image> <bios|uefi> [timeout-seconds] [marker]}"
MODE="${2:?usage: disk-image-test.sh <image> <bios|uefi> [timeout-seconds] [marker]}"
TIMEOUT="${3:-300}"
MARKER="${4:-${MARKER:-login:|Welcome}}"

case "$MODE" in
  bios|uefi) ;;
  *) echo "FAIL: MODE must be 'bios' or 'uefi' (got '$MODE')" >&2; exit 2 ;;
esac
[ -f "$IMAGE" ] || { echo "FAIL: image not found: $IMAGE" >&2; exit 2; }

QEMU_BIN="${QEMU_BIN:-qemu-system-x86_64}"
QEMU_SHARE="${QEMU_SHARE:-$(brew --prefix qemu 2>/dev/null)/share/qemu}"
command -v "$QEMU_BIN" >/dev/null 2>&1 || { echo "FAIL: qemu not found: $QEMU_BIN" >&2; exit 2; }

WORK="$(mktemp -d)"
LOG="$WORK/serial.log"
trap 'if [ "${KEEP_WORK:-0}" != "1" ]; then rm -rf "$WORK"; fi' EXIT

START_TS=$(date +%s)

QEMU_ARGS=(-m 1024 -nographic -no-reboot
  -drive "file=$IMAGE,format=raw,if=ide"
  -serial stdio)
# Route serial to file via -serial file:... instead of stdio to capture
QEMU_ARGS=(-m 1024 -nographic -no-reboot
  -drive "file=$IMAGE,format=raw,if=ide")

if [ "$MODE" = "uefi" ]; then
  OVMF="$QEMU_SHARE/edk2-x86_64-code.fd"
  [ -f "$OVMF" ] || { echo "FAIL: OVMF firmware not found at $OVMF" >&2; exit 2; }
  QEMU_ARGS+=(-drive if=pflash,format=raw,readonly=on,file="$OVMF")
fi
# Capture serial console to log
QEMU_ARGS+=(-serial "file:$LOG")

"$QEMU_BIN" "${QEMU_ARGS[@]}" >"$WORK/qemu.out" 2>&1 &
QEMU_PID=$!

DEADLINE=$((SECONDS + TIMEOUT))
RESULT="Fail"
REASON=""
while [ $SECONDS -lt $DEADLINE ]; do
  if ! kill -0 "$QEMU_PID" 2>/dev/null; then
    REASON="qemu exited early"
    break
  fi
  if grep -qiE "$MARKER" "$LOG" 2>/dev/null; then
    RESULT="Pass"
    break
  fi
  sleep 2
done

if [ "$RESULT" != "Pass" ] && [ -z "$REASON" ]; then
  REASON="marker not seen within ${TIMEOUT}s"
fi

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

END_TS=$(date +%s)
ELAPSED=$((END_TS - START_TS))

echo "Gate: disk-image-test ($MODE)"
echo "Date: $(date +%Y-%m-%d)"
echo "Image: $IMAGE ($(du -h "$IMAGE" | cut -f1))"
echo "Machine: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown), macOS $(sw_vers -productVersion 2>/dev/null || echo unknown), QEMU TCG emulation"
echo "Command: disk-image-test.sh $IMAGE $MODE $TIMEOUT"
echo "Marker: $MARKER"
echo "Values: result ${RESULT}; elapsed ${ELAPSED}s; serial log lines: $(wc -l <"$LOG" 2>/dev/null | tr -d ' ' || echo 0)"
if [ "$KEEP_WORK" = "1" ]; then
  echo "Work dir kept: $WORK"
fi
echo "Result: $RESULT"
if [ "$RESULT" != "Pass" ]; then
  echo "Reason: $REASON"
  echo "Notes: qemu output tail:"
  tail -20 "$WORK/qemu.out" 2>/dev/null || true
  echo "serial log tail:"
  tail -30 "$LOG" 2>/dev/null || true
fi
[ "$RESULT" = "Pass" ]