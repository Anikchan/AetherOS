#!/usr/bin/env bash
# ci-local.sh — run the same checks as .github/workflows/ci.yml locally.
# Exits non-zero on any failure.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

echo "=== cargo build --workspace ==="
cargo build --workspace

echo "=== cargo test --workspace ==="
cargo test --workspace

echo "=== check-budgets (sample) ==="
python3 os-build/check-budgets.py os-build/metrics/sample.txt

echo "=== ci-local: all checks passed ==="