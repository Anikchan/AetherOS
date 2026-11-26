#!/usr/bin/env bash
# P9-T01 — build a RAUC update bundle from the built rootfs image.
#
# Usage:
#   rauc-bundle.sh --image FILE --version V [--out FILE] [--cert PEM] [--key PEM]
#                  [--dry-run]
#
# The bundle layout is the one described by update/config/manifest.raucm.tmpl:
#
#   out/aetheros-<version>.raucb
#     manifest.raucm
#     rootfs.squashfs
#
# Requires `rauc` on the build host (not on the OS image) and a signing
# certificate/key. In production the key is offline (P9-T05); development keys
# are described in update/config/README.md.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/../.." && pwd)
IMAGE=""
VERSION=""
OUT=""
CERT="${RAUC_CERT:-/etc/rauc/dev-signing.cert.pem}"
KEY="${RAUC_KEY:-/etc/rauc/dev-signing.key}"
DRY_RUN=0

while (($#)); do
  case "$1" in
    --image) IMAGE="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --cert) CERT="$2"; shift 2 ;;
    --key) KEY="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help)
      sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[ -n "$IMAGE" ] && [ -f "$IMAGE" ] || { echo "ERROR: --image FILE is required" >&2; exit 2; }
[ -n "$VERSION" ] || { echo "ERROR: --version V is required" >&2; exit 2; }
[ -n "$OUT" ] || OUT="out/aetheros-${VERSION}.raucb"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

DIGEST=$(sha256_of "$IMAGE")
SIZE=$(wc -c <"$IMAGE" | tr -d ' ')
IMAGE_NAME=$(basename "$IMAGE")

cat <<EOF
RAUC bundle plan
  image:   $IMAGE ($SIZE bytes)
  sha256:  $DIGEST
  version: $VERSION
  out:     $OUT
  cert:    $CERT
EOF
if [ "$DRY_RUN" = "1" ]; then
  echo "RAUC_BUNDLE_DRY_RUN out=$OUT"
  exit 0
fi

[ -f "$CERT" ] || { echo "ERROR: signing certificate not found: $CERT" >&2; exit 1; }
[ -f "$KEY" ] || { echo "ERROR: signing key not found: $KEY" >&2; exit 1; }
command -v rauc >/dev/null 2>&1 || { echo "ERROR: rauc is not installed on this host" >&2; exit 1; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/aether-rauc.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/bundle"
cp "$IMAGE" "$WORK/bundle/$IMAGE_NAME"
sed -e "s|@VERSION@|$VERSION|g" \
    -e "s|@DESCRIPTION@|AetherOS $VERSION|g" \
    -e "s|@IMAGE@|$IMAGE_NAME|g" \
    -e "s|@IMAGE_SHA256@|$DIGEST|g" \
    -e "s|@IMAGE_SIZE@|$SIZE|g" \
    "$ROOT_DIR/update/config/manifest.raucm.tmpl" >"$WORK/bundle/manifest.raucm"

mkdir -p "$(dirname "$OUT")"
rauc bundle --cert="$CERT" --key="$KEY" "$WORK/bundle" "$OUT"
rauc info "$OUT"

echo "RAUC_BUNDLE_BUILT out=$OUT version=$VERSION sha256=$DIGEST"
