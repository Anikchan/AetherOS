#!/bin/bash
# Spike S5 GUI (P0-T12 residual): a Windows GUI program through Wine, under a
# virtual display (Xvfb) on a Linux host. Complements the headless console
# spike (spike-s5.sh). Measures time from process start to window creation and
# confirms the window is created (S5_GUI_WINDOW_CREATED_MS marker).
# Markers:
#   S5_GUI_WINE_VERSION, S5_GUI_BUILD_OK, S5_GUI_WINDOW_CREATED_MS,
#   S5_GUI_DONE, S5_GUI_RUN_OK
set -u

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "S5_GUI_WINE_VERSION: $(wine --version 2>/dev/null || wine64 --version 2>/dev/null || echo unknown)"

# Build the GUI test program.
if ! x86_64-w64-mingw32-gcc -O2 -mwindows -o "$WORK/spike-s5-gui.exe" \
        "$(dirname "$0")/spike-s5-gui.c" 2>"$WORK/build.log"; then
    echo "S5_GUI_BUILD_FAIL"
    cat "$WORK/build.log"
    exit 1
fi
test -f "$WORK/spike-s5-gui.exe" && echo "S5_GUI_BUILD_OK"

export WINEDEBUG=-all WINEDLLOVERRIDES="mscoree=d;mshtml=d"
export WINEPREFIX="$WORK/prefix"
wine64 wineboot -i >/dev/null 2>&1 || wine wineboot -i >/dev/null 2>&1 || true

RUN="wine64"; command -v wine64 >/dev/null || RUN="wine"

# Provide a virtual display if Xvfb is available (CI runners have it).
XVFB_PID=""
if command -v Xvfb >/dev/null 2>&1; then
    Xvfb :99 -screen 0 1024x768x24 >/dev/null 2>&1 &
    XVFB_PID=$!
    export DISPLAY=:99
    sleep 1
fi
cleanup_xvfb() { [ -n "$XVFB_PID" ] && kill "$XVFB_PID" 2>/dev/null || true; }
trap 'cleanup_xvfb; rm -rf "$WORK"' EXIT

LOG="$WORK/gui.log"
if timeout 20 "$RUN" "$WORK/spike-s5-gui.exe" > "$LOG" 2>&1; then
    RC=0
else
    RC=$?
fi
cat "$LOG"

if grep -q "S5_GUI_WINDOW_CREATED_MS" "$LOG"; then
    echo "S5_GUI_RUN_OK (exit $RC)"
else
    echo "S5_GUI_RUN_FAIL: no window-created marker (exit $RC)"
    exit 1
fi
grep -q "S5_GUI_DONE" "$LOG" || echo "S5_GUI_DONE_MISSING (window may have been destroyed early)"
echo "S5_GUI_SPIKE_DONE"