#!/usr/bin/env bash
# P5-T08 — 72-hour soak harness (G-PM5).
#
# Full run:    SOAK_HOURS=72 ./soak-72h.sh
# CI smoke:    SOAK_HOURS=0.01 CYCLES=3 CYCLE_SECONDS=0 ./soak-72h.sh
# Reference:   REFERENCE_HARDWARE=1 SOAK_HOURS=72 B2_RESTORE_MS=380 B6_SHARE=2.1 ./soak-72h.sh
#
# One cycle launches 50 apps, switches focus, injects memory pressure and
# confirms that no foreground app is killed and that inactive memory returns to
# the cycle baseline. MemAvailable is sampled before and after every cycle so
# the gate report can prove the baseline return instead of assuming it.
#
# Reporting (P5-T08): every cycle keeps its own `aether-bench --json` file under
# `$OUT_DIR/p5-t08-soak-cycles/`, the merged file is
# `$OUT_DIR/p5-t08-soak.json`, and the samples TSV records the cycle exit code
# so `gate-report.py` can fail G-PM5 when any cycle errored instead of only
# returning a non-zero exit.
set -euo pipefail

APPS=${APPS:-50}
SOAK_HOURS=${SOAK_HOURS:-72}
CYCLE_SECONDS=${CYCLE_SECONDS:-900}
OUT_DIR=${OUT_DIR:-docs/$(date +%Y/%m/%d)/results}
REPORT="$OUT_DIR/p5-t08-soak.md"
GATE_REPORT="$OUT_DIR/p5-t08-gate-report.md"
SAMPLES="$OUT_DIR/p5-t08-soak-samples.tsv"
CYCLE_JSON_DIR="$OUT_DIR/p5-t08-soak-cycles"
MERGED_JSON="$OUT_DIR/p5-t08-soak.json"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$OUT_DIR" "$CYCLE_JSON_DIR"
if [ "${CYCLES:-}" != "" ]; then
  CYCLES=${CYCLES}
else
  CYCLES=$(python3 - "$SOAK_HOURS" "$CYCLE_SECONDS" <<'PY'
import sys, math
hours = float(sys.argv[1]); cycle = max(1.0, float(sys.argv[2]))
print(max(1, math.ceil(hours * 3600 / cycle)))
PY
)
fi


mem_available_kb() {
  if [ -r /proc/meminfo ]; then
    awk '/MemAvailable/ {print $2; exit}' /proc/meminfo
  else
    echo 0
  fi
}

echo "aether soak: apps=$APPS hours=$SOAK_HOURS cycles=$CYCLES cycle_seconds=$CYCLE_SECONDS"
: > "$SAMPLES"
rm -f "$CYCLE_JSON_DIR"/cycle-*.json
START=$(date +%s)
FAILED_CYCLES=0
for ((cycle = 1; cycle <= CYCLES; cycle++)); do
  before=$(mem_available_kb)
  cycle_json=$(printf '%s/cycle-%03d.json' "$CYCLE_JSON_DIR" "$cycle")
  # A failing cycle must not abort the soak: the gate report records the
  # failure and returns a non-zero exit at the end.
  set +e
  aether-bench \
    --apps "$APPS" \
    --cycles 1 \
    --json "$cycle_json" \
    --report "$REPORT"
  cycle_status=$?
  set -e
  if ((cycle_status != 0)); then
    FAILED_CYCLES=$((FAILED_CYCLES + 1))
  fi
  echo "cycle $cycle exit=$cycle_status json=$cycle_json"
  sleep 2
  after=$(mem_available_kb)
  printf '%s\t%s\t%s\t%s\n' "$cycle" "$before" "$after" "$cycle_status" >> "$SAMPLES"
  echo "soak cycle $cycle/$CYCLES complete (MemAvailable ${before} kB -> ${after} kB)"
  if ((cycle < CYCLES)); then
    sleep "$CYCLE_SECONDS"
  fi
done
END=$(date +%s)
ELAPSED_HOURS=$(python3 - "$START" "$END" <<'PY'
import sys
print(max(0.0, (int(sys.argv[2]) - int(sys.argv[1])) / 3600.0))
PY
)

# Merge the per-cycle bench JSON files so the gate report sees every cycle.
python3 - "$CYCLE_JSON_DIR" "$MERGED_JSON" "$APPS" <<'PY'
import glob, json, os, sys
cycle_dir, out, apps = sys.argv[1], sys.argv[2], int(sys.argv[3])
cycles = []
for path in sorted(glob.glob(os.path.join(cycle_dir, "cycle-*.json"))):
    with open(path, "r", encoding="utf-8") as handle:
        cycles.extend(json.load(handle).get("cycles_detail", []))
with open(out, "w", encoding="utf-8") as handle:
    json.dump({"apps": apps, "cycles": len(cycles), "cycles_detail": cycles}, handle, indent=2)
print(f"merged {len(cycles)} cycles -> {out}")
PY

GATE_ARGS=(--bench-json "$MERGED_JSON" --gate all
           --host-label "$(uname -s)/$(uname -m)"
           --soak-hours "$ELAPSED_HOURS"
           --soak-samples "$SAMPLES"
           --out "$GATE_REPORT"
           --json "${GATE_REPORT%.md}.json")
[ "${REFERENCE_HARDWARE:-0}" = "1" ] && GATE_ARGS+=(--reference-hardware)
[ -n "${B2_RESTORE_MS:-}" ] && GATE_ARGS+=(--b2-restore-ms "$B2_RESTORE_MS")
[ -n "${B6_SHARE:-}" ] && GATE_ARGS+=(--b6-share "$B6_SHARE")

set +e
python3 "$SCRIPT_DIR/gate-report.py" "${GATE_ARGS[@]}"
GATE_STATUS=$?
set -e

echo "P5T08_SOAK_DONE apps=$APPS cycles=$CYCLES hours=$ELAPSED_HOURS failed_cycles=$FAILED_CYCLES report=$REPORT gate=$GATE_REPORT"
# PENDING (0) is a valid model-host result; FAIL (1) and INCOMPLETE (2) surface
# as a non-zero exit so CI and the release checklist cannot ignore them.
exit "$GATE_STATUS"
