#!/usr/bin/env bash
# spike-s3.sh — P0-T09 Spike S3 driver (session implementation-2, 2026-11-18).
#
# Boots the Alpine x86-64 live ISO in QEMU and verifies on the guest kernel:
#   1. PSI available: /proc/pressure/{cpu,memory,io} report stall values
#   2. cgroup v2 cgroup.freeze: a frozen process stops making progress
#   3. memory.reclaim: writing a reclaim target shrinks memory.current
#
# Usage: spike-s3.sh <alpine-iso>   (7zz, qemu-system-x86_64 and expect required)
# Prints the guest transcript and a D6-format report; exits non-zero on failure.

set -euo pipefail

ISO="${1:?usage: spike-s3.sh <alpine-iso>}"
DIR="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
TRANSCRIPT="$WORK/transcript.log"
trap 'kill "${SPAWN_PID:-}" 2>/dev/null || true; rm -rf "$WORK"' EXIT

# Extract kernel and initramfs from the ISO once.
7zz x -o"$WORK/extract" "$ISO" >/dev/null
KERNEL="$WORK/extract/boot/vmlinuz-virt"
INITRD="$WORK/extract/boot/initramfs-virt"
[ -f "$KERNEL" ] || { echo "FAIL: kernel not found in ISO"; exit 2; }

export ISO KERNEL INITRD TRANSCRIPT
expect "$DIR/spike-s3.exp"
SPAWN_PID=""

echo "=== Guest transcript ==="
cat "$TRANSCRIPT"
echo "=== End transcript ==="

PASS=1
grep -q "S3_PSI_OK" "$TRANSCRIPT" || { echo "FAIL: PSI check"; PASS=0; }
grep -q "S3_FREEZE_OK" "$TRANSCRIPT" || { echo "FAIL: freeze check"; PASS=0; }
grep -q "S3_RECLAIM_OK" "$TRANSCRIPT" || { echo "FAIL: memory.reclaim check"; PASS=0; }

echo "Gate: P0-T09 Spike S3 (cgroup.freeze, memory.reclaim, PSI)"
echo "Date: $(date +%Y-%m-%d)"
echo "Machine: $(sysctl -n machdep.cpu.brand_string), $(($(sysctl -n hw.memsize) / 1073741824)) GB RAM host; QEMU 11.1.3 TCG, 1 GB guest RAM"
echo "Kernel: $(grep -m1 'uname' "$TRANSCRIPT" || true)"
echo "Command: os-build/test/spike-s3.sh $ISO"
echo "Values: see transcript markers S3_PSI_OK / S3_FREEZE_OK / S3_RECLAIM_OK"
if [ "$PASS" = "1" ]; then
  echo "Result: Pass"
else
  echo "Result: Fail"
  echo "Notes: inspect transcript above; retry budget: 2 (team retry rule)"
fi
[ "$PASS" = "1" ]
