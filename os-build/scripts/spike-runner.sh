#!/usr/bin/env bash
# spike-runner.sh — reusable harness for running a spike script with timeout,
# log capture, and marker verification. Supports S2 spike scripts and S4's
# "test harnesses" goal. Does not modify spike scripts (S2 territory).
#
# Usage:
#   spike-runner.sh <script> [args...] --expect MARKER1 [MARKER2 ...]
#   spike-runner.sh os-build/test/spike-s3.sh iso.iso --expect S3_PSI_OK S3_FREEZE_OK
#
# Environment:
#   TIMEOUT  seconds (default 300)
#   LOGDIR   directory for logs (default os-build/.cache/spike-logs)
set -u

TIMEOUT="${TIMEOUT:-300}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OS_BUILD_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$OS_BUILD_DIR/.." && pwd)"
LOGDIR="${LOGDIR:-$OS_BUILD_DIR/.cache/spike-logs}"
mkdir -p "$LOGDIR"

# Parse --expect
EXPECT=()
ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --expect) shift; while [ $# -gt 0 ] && [ "${1:0:1}" != "-" ]; do EXPECT+=("$1"); shift; done ;;
    *) ARGS+=("$1"); shift ;;
  esac
done

if [ ${#ARGS[@]} -eq 0 ]; then
  echo "Usage: $0 <script> [args...] --expect MARKER..." >&2
  exit 2
fi

SCRIPT="${ARGS[0]}"
ARGS=("${ARGS[@]:1}")
NAME="$(basename "$SCRIPT" .sh)"
LOG="$LOGDIR/${NAME}-$(date +%Y%m%d-%H%M%S).log"

echo "Running: $SCRIPT ${ARGS[*]}"
echo "Timeout: ${TIMEOUT}s  Log: $LOG"

set +e
timeout "$TIMEOUT" bash "$SCRIPT" "${ARGS[@]}" >"$LOG" 2>&1
RC=$?
set -e

echo "=== tail ==="
tail -20 "$LOG"
echo "=== exit code: $RC ==="

FAIL=0
if [ $RC -ne 0 ]; then
  echo "FAIL: script exited non-zero ($RC)"
  FAIL=1
fi
for m in "${EXPECT[@]}"; do
  if grep -q "$m" "$LOG"; then
    echo "  marker OK: $m"
  else
    echo "  marker MISSING: $m"
    FAIL=1
  fi
done

if [ $FAIL -eq 0 ]; then
  echo "spike-runner: PASS"
  exit 0
else
  echo "spike-runner: FAIL (log: $LOG)"
  exit 1
fi