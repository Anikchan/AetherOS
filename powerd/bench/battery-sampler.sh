#!/usr/bin/env bash
# P5-T14 — battery benchmark sampler driver (04 section 11.2).
#
# W1: idle at Balanced for 1 hour, lid open.
# W2: scripted browsing for 1 hour.
# W3: local video playback for 1 hour.
# Repeat each run 3 times and take the median; compare with Windows 11 on the
# same laptop (dual boot). Results from machines without a battery do not count.
set -euo pipefail

LABEL=${1:-W1}
HOURS=${HOURS:-1}
INTERVAL=${INTERVAL:-300}
OUT_DIR=${OUT_DIR:-docs/$(date +%Y/%m/%d)/results}
OUT="$OUT_DIR/p5-t14-${LABEL,,}.csv"

mkdir -p "$OUT_DIR"
echo "battery sampler: label=$LABEL hours=$HOURS interval=${INTERVAL}s"
aether-battery-sampler \
  --label "$LABEL" \
  --hours "$HOURS" \
  --interval "$INTERVAL" \
  --out "$OUT"
echo "P5T14_SAMPLER_DONE label=$LABEL out=$OUT"
