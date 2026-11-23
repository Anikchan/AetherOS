#!/usr/bin/env bash
# P5-T08 — 50-app benchmark driver (04 section 4, gates B1..B6, G-PM1/G-PM2/
# G-PM4). Runs on AetherOS (real cgroups) or on the dev host (modelled memory,
# policy-level checks only). Evidence is written in D6 format.
set -euo pipefail

APPS=${APPS:-50}
CYCLES=${CYCLES:-1}
OUT_DIR=${OUT_DIR:-docs/$(date +%Y/%m/%d)/results}
REPORT="$OUT_DIR/p5-t08-bench-50.md"
JSON="$OUT_DIR/p5-t08-bench-50.json"

mkdir -p "$OUT_DIR"
echo "aether-bench: apps=$APPS cycles=$CYCLES"
aether-bench \
  --apps "$APPS" \
  --cycles "$CYCLES" \
  --report "$REPORT" \
  --json "$JSON"

echo "P5T08_BENCH_DONE apps=$APPS report=$REPORT"
