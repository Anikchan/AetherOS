#!/bin/bash
# Spike S5 (P0-T12): one Windows test program through Wine on a Linux VM.
# Builds the test exe with mingw-w64, runs it under Wine (headless console),
# and measures end-to-end startup (process start to exit).
# Markers (quoted in expect-style tests elsewhere; here echoed plainly):
#   S5_WINE_RUN_OK, S5_STARTUP_MS, S5_WINE_VERSION
set -euo pipefail

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "S5_WINE_VERSION: $(wine --version 2>/dev/null || wine64 --version)"

x86_64-w64-mingw32-gcc -O2 -o "$WORK/spike-s5-hello.exe" \
    "$(dirname "$0")/spike-s5-hello.c"
test -f "$WORK/spike-s5-hello.exe"

# First wine invocation initializes the prefix; time the real run separately.
export WINEDEBUG=-all WINEDLLOVERRIDES="mscoree=d;mshtml=d"
export WINEPREFIX="$WORK/prefix"
wine64 wineboot -i >/dev/null 2>&1 || wine wineboot -i >/dev/null 2>&1

RUN="wine64"
command -v wine64 >/dev/null || RUN="wine"

STARTUP_MS=$( { /usr/bin/time -f '%e' "$RUN" "$WORK/spike-s5-hello.exe" \
    2> "$WORK/time.txt"; } >/dev/null; cat "$WORK/time.txt" )

echo "S5_STARTUP_MS: $(awk "{print int(\$1*1000)}" <<< "$STARTUP_MS")"
echo 'S5_WINE_RUN_OK'
