#!/usr/bin/env bash
# artifact-clean.sh — remove local build artifacts and cached downloads.
# Does NOT touch git-tracked files.
#
# Usage: artifact-clean.sh [--all]   (--all also removes the kernel cache)

set -euo pipefail

OS_BUILD_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT_DIR="$(cd "$OS_BUILD_DIR/.." && pwd)"

echo "Cleaning cargo target/ ..."
rm -rf "$ROOT_DIR/target"

echo "Cleaning os-build temp/cache (except kernel cache) ..."
find "$OS_BUILD_DIR" -name '*.tmp' -delete 2>/dev/null || true
rm -rf "$OS_BUILD_DIR/.cache/tmp" 2>/dev/null || true

if [ "${1:-}" = "--all" ]; then
  echo "Removing kernel cache (--all) ..."
  rm -rf "$OS_BUILD_DIR/.cache/kernel"
fi

echo "Done."