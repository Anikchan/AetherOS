#!/usr/bin/env bash
# metrics-export.sh — export a metrics file to stdout in the D6 fixed text format.
# Wraps check-budgets output and the source file for manual export
# (nexactl metrics export is P7-T03; this is the interim helper).
#
# Usage: metrics-export.sh <metrics-file>

set -euo pipefail

FILE="${1:?usage: metrics-export.sh <metrics-file>}"
if [ ! -f "$FILE" ]; then
  echo "ERROR: file not found: $FILE" >&2
  exit 2
fi

echo "# AetherOS metrics export"
echo "# Source: $FILE"
echo "# Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo
cat "$FILE"
echo
echo "# Budget check:"
python3 "$(dirname "$0")/../check-budgets.py" "$FILE" || true