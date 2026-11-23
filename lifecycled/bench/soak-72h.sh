#!/usr/bin/env bash
# P5-T08 — 72-hour soak harness (G-PM5).
#
# Full run:   SOAK_HOURS=72 ./soak-72h.sh
# CI smoke:   SOAK_HOURS=0.01 CYCLES=3 ./soak-72h.sh
#
# The harness launches 50 apps per cycle, switches focus randomly, injects
# memory pressure, and checks that no foreground app is killed and that
# inactive memory returns to baseline after every cycle.
set -euo pipefail

APPS=${APPS:-50}
SOAK_HOURS=${SOAK_HOURS:-72}
CYCLE_SECONDS=${CYCLE_SECONDS:-900}
OUT_DIR=${OUT_DIR:-docs/$(date +%Y/%m/%d)/results}
REPORT="$OUT_DIR/p5-t08-soak.md"

mkdir -p "$OUT_DIR"
CYCLES=$(python3 - "$SOAK_HOURS" "$CYCLE_SECONDS" <<'PY'
import sys, math
hours = float(sys.argv[1]); cycle = float(sys.argv[2])
print(max(1, math.ceil(hours * 3600 / cycle)))
PY
)

echo "aether soak: apps=$APPS hours=$SOAK_HOURS cycles=$CYCLES"
aether-bench \
  --apps "$APPS" \
  --cycles "$CYCLES" \
  --soak-hours "$SOAK_HOURS" \
  --report "$REPORT"

echo "P5T08_SOAK_DONE apps=$APPS cycles=$CYCLES report=$REPORT"
