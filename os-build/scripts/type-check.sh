#!/usr/bin/env bash
# type-check.sh — local Linux type-check recipe for the Rust workspace/spikes.
#
# Dev host is macOS; AetherOS targets Linux x86-64. This script reproduces the
# CI type-check locally using rustup's linux-gnu toolchain, as recorded in
# docs/2026/11/17/os-implementation/plan.md (2026-11-18 entry).
#
# Prerequisites (one-time):
#   brew install rustup
#   rustup toolchain install stable -t x86_64-unknown-linux-gnu
#
# Usage: type-check.sh [crate-path]   (default: repo root workspace)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET="${1:-$ROOT_DIR}"

RUSTUP_BIN="$(brew --prefix rustup 2>/dev/null)/bin" 2>/dev/null || RUSTUP_BIN=""
if [ -z "$RUSTUP_BIN" ] || [ ! -x "$RUSTUP_BIN/cargo" ]; then
  # Fall back to a rustup on PATH
  if command -v rustup >/dev/null 2>&1; then
    RUSTUP_BIN="$(dirname "$(command -v rustup)")"
  fi
fi

if [ -z "$RUSTUP_BIN" ] || [ ! -x "$RUSTUP_BIN/cargo" ]; then
  echo "ERROR: rustup not found. Install with: brew install rustup" >&2
  exit 2
fi

TOOLCHAIN_DIR="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin"
if [ ! -d "$TOOLCHAIN_DIR" ]; then
  # Try any stable toolchain
  TOOLCHAIN_DIR="$(ls -d "$HOME/.rustup/toolchains"/stable* 2>/dev/null | head -1)"
fi
RUSTC="$TOOLCHAIN_DIR/bin/rustc"
if [ ! -x "$RUSTC" ]; then
  echo "ERROR: stable rustc not found under ~/.rustup/toolchains" >&2
  exit 2
fi

echo "Using rustup: $RUSTUP_BIN"
echo "Using rustc:  $RUSTC"
echo "Target:       x86_64-unknown-linux-gnu"
echo "Crate:        $TARGET"

export RUSTUP_TOOLCHAIN="stable-aarch64-apple-darwin"
export RUSTC="$RUSTC"
cd "$TARGET"
"$RUSTUP_BIN/cargo" check --target x86_64-unknown-linux-gnu "$@"
echo "type-check OK"