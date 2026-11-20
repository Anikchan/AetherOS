#!/usr/bin/env bash
# fetch-kernel.sh — download and cache the upstream kernel tarball used by the
# kernel-config workflow. Avoids re-downloading ~150 MB on every run.
#
# Cache location: os-build/.cache/kernel/ (gitignored).
# Usage: fetch-kernel.sh [version]   (default: 6.18)

set -euo pipefail

VERSION="${1:-6.18}"
CACHE_DIR="$(cd "$(dirname "$0")/.." && pwd)/.cache/kernel"
mkdir -p "$CACHE_DIR"

TARBALL="linux-${VERSION}.tar.xz"
URL="https://cdn.kernel.org/pub/linux/kernel/v6.x/${TARBALL}"
DEST="$CACHE_DIR/$TARBALL"

if [ -f "$DEST" ]; then
  echo "Cache hit: $DEST"
  echo "size: $(du -h "$DEST" | cut -f1)"
  exit 0
fi

echo "Downloading $URL ..."
if command -v curl >/dev/null 2>&1; then
  curl -fL --progress-bar -o "$DEST.tmp" "$URL"
elif command -v wget >/dev/null 2>&1; then
  wget -O "$DEST.tmp" "$URL"
else
  echo "ERROR: need curl or wget" >&2
  exit 2
fi
mv "$DEST.tmp" "$DEST"
echo "Cached: $DEST ($(du -h "$DEST" | cut -f1))"