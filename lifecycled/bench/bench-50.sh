#!/usr/bin/env bash
# P5-T08 — 50-app benchmark driver (04 section 4, gates B1..B6, G-PM1/G-PM2/
# G-PM4). Runs on AetherOS (real cgroups) or on the dev host (modelled memory,
# policy-level checks only). Evidence is written in D6 format.
#
# Reference-machine run:
#   REFERENCE_HARDWARE=1 B2_RESTORE_MS=380 B6_SHARE=2.1 ./bench-50.sh
set -euo pipefail

APPS=${APPS:-50}
CYCLES=${CYCLES:-1}
OUT_DIR=${OUT_DIR:-docs/$(date +%Y/%m/%d)/results}
REPORT="$OUT_DIR/p5-t08-bench-50.md"
JSON="$OUT_DIR/p5-t08-bench-50.json"
GATE_REPORT="$OUT_DIR/p5-t08-bench-gate.md"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$OUT_DIR"
echo "aether-bench: apps=$APPS cycles=$CYCLES"
aether-bench \
  --apps "$APPS" \
  --cycles "$CYCLES" \
  --report "$REPORT" \
  --json "$JSON"

GATE_ARGS=(--bench-json "$JSON" --gate G-PM1
           --host-label "$(uname -s)/$(uname -m)"
           --out "$GATE_REPORT"
           --json "${GATE_REPORT%.md}.json")
[ "${REFERENCE_HARDWARE:-0}" = "1" ] && GATE_ARGS+=(--reference-hardware)
[ -n "${B2_RESTORE_MS:-}" ] && GATE_ARGS+=(--b2-restore-ms "$B2_RESTORE_MS")
[ -n "${B6_SHARE:-}" ] && GATE_ARGS+=(--b6-share "$B6_SHARE")

set +e
python3 "$SCRIPT_DIR/gate-report.py" "${GATE_ARGS[@]}"
GATE_STATUS=$?
set -e

echo "P5T08_BENCH_DONE apps=$APPS report=$REPORT gate=$GATE_REPORT"
exit "$GATE_STATUS"
