#!/usr/bin/env bash
# P9-T05 — image checksums for Personal Edition builds.
#
# Creates or verifies SHA-256 sidecar files for ISO/IMG artifacts. Signing is
# deferred to the public release (signed pipeline not in the Personal Edition).
#
# Usage:
#   image-checksum.sh create IMAGE [IMAGE...]
#   image-checksum.sh verify IMAGE [IMAGE...]
#   image-checksum.sh verify-dir DIR
set -euo pipefail

usage() {
  echo "usage: $0 <create|verify> IMAGE... | $0 verify-dir DIR" >&2
  exit 2
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "no SHA-256 tool (sha256sum/shasum) found" >&2
    exit 1
  fi
}

create() {
  local image="$1"
  if [[ ! -f "$image" ]]; then
    echo "missing image: $image" >&2
    exit 1
  fi
  local digest
  digest=$(hash_file "$image")
  printf '%s  %s\n' "$digest" "$(basename "$image")" > "${image}.sha256"
  echo "CHECKSUM_CREATED $image sha256=$digest"
}

verify() {
  local image="$1"
  local sidecar="${image}.sha256"
  if [[ ! -f "$sidecar" ]]; then
    echo "CHECKSUM_MISSING $sidecar" >&2
    return 1
  fi
  local expected actual
  expected=$(awk '{print $1}' "$sidecar")
  actual=$(hash_file "$image")
  if [[ "$expected" != "$actual" ]]; then
    echo "CHECKSUM_MISMATCH $image expected=$expected actual=$actual" >&2
    return 1
  fi
  echo "CHECKSUM_OK $image"
}

[[ $# -ge 2 ]] || usage
command="$1"
shift

case "$command" in
  create)
    for image in "$@"; do create "$image"; done
    ;;
  verify)
    status=0
    for image in "$@"; do verify "$image" || status=1; done
    exit "$status"
    ;;
  verify-dir)
    status=0
    for image in "$1"/*; do
      case "$image" in
        *.iso|*.img|*.raw) verify "$image" || status=1 ;;
      esac
    done
    exit "$status"
    ;;
  *)
    usage
    ;;
esac
