#!/bin/sh
# AetherOS one-shot hardware validation (P2 / 04 §6).
# Run as root after boot:  sh /root/test.sh
# Writes a full log to /var/log/aether-test.log (tmpfs) and prints it to the console.
# I (the AI/SSH) read the log afterwards and continue from the markers.
#
# Optional env:
#   SLEEP_CYCLES=10   suspend/resume cycles (0 to skip)
#   SLEEP_WAKE=15     RTC auto-wake seconds per cycle

LOG=/var/log/aether-test.log
SLEEP_CYCLES="${SLEEP_CYCLES:-10}"
SLEEP_WAKE="${SLEEP_WAKE:-15}"
: > "$LOG" 2>/dev/null || LOG=/tmp/aether-test.log
: > "$LOG"

say() { printf '%s\n' "$*" | tee -a "$LOG"; }
run() { say "\$ $*"; "$@" 2>&1 | tee -a "$LOG"; }

say "================================================================"
say "AetherOS hardware validation — $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
say "================================================================"

say ""
say "### SYSTEM"
run uname -a
run cat /proc/cmdline
run cat /proc/uptime
run free -m 2>/dev/null
run df -h 2>/dev/null
run mount

say ""
say "### NETWORK"
run cat /proc/net/dev
ip -4 addr 2>/dev/null | tee -a "$LOG"
ip route 2>/dev/null | tee -a "$LOG"
cat /etc/resolv.conf 2>/dev/null | tee -a "$LOG"

say ""
say "### PCI / USB"
run lspci
run lsusb

say ""
say "### GRAPHICS (P2-T02)"
lspci -k 2>/dev/null | grep -A3 -E "VGA|3D" | tee -a "$LOG"
dmesg 2>/dev/null | grep -iE "i915|drm|fb0:" | tail -15 | tee -a "$LOG"
say "-- DRM connectors --"
for c in /sys/class/drm/card*-*/status; do
    [ -e "$c" ] && say "$c: $(cat "$c")"
done
say "-- native modes (first connector) --"
for m in /sys/class/drm/card0/card0-*/modes; do
    [ -e "$m" ] && { say "$m:"; head -3 "$m"; }
done
say "-- framebuffer --"
ls /dev/fb* 2>/dev/null | tee -a "$LOG"
command -v glxinfo >/dev/null 2>&1 && glxinfo -B 2>&1 | grep -iE "renderer|vendor|OpenGL" | tee -a "$LOG"
command -v vulkaninfo >/dev/null 2>&1 && vulkaninfo 2>&1 | grep -iE "deviceName|driverName|apiVersion" | head -10 | tee -a "$LOG"

say ""
say "### AUDIO (P2-T04)"
command -v aplay >/dev/null 2>&1 && aplay -l 2>&1 | tee -a "$LOG"
cat /proc/asound/cards 2>/dev/null | tee -a "$LOG"
pgrep -a pipewire >/dev/null 2>&1 && say "pipewire: running" || say "pipewire: not running"
pgrep -a wireplumber >/dev/null 2>&1 && say "wireplumber: running" || say "wireplumber: not running"
if command -v speaker-test >/dev/null 2>&1; then
    say "-- speaker-test (front left/right, 1 loop) --"
    speaker-test -t wav -c 2 -l 1 2>&1 | tail -6 | tee -a "$LOG"
fi
if command -v arecord >/dev/null 2>&1 && command -v aplay >/dev/null 2>&1; then
    say "-- microphone round-trip --"
    arecord -d 2 -f cd /tmp/mic.wav >/dev/null 2>&1 && aplay /tmp/mic.wav >/dev/null 2>&1 \
        && say "MIC_OK" || say "MIC_FAIL"
fi

say ""
say "### INPUT (P2-T05)"
run ls /dev/input
command -v libinput >/dev/null 2>&1 && libinput list-devices 2>&1 | grep -E "^Device|Kernel|Capabilities" | tee -a "$LOG"
say "-- backlight --"
for b in /sys/class/backlight/*; do
    [ -e "$b" ] && say "$b: $(cat "$b/brightness" 2>/dev/null)/$(cat "$b/max_brightness" 2>/dev/null)"
done
say "-- LEDs --"
ls /sys/class/leds/ 2>/dev/null | tee -a "$LOG"

say ""
say "### BATTERY / THERMAL"
command -v upower >/dev/null 2>&1 && upower -i $(upower -e 2>/dev/null | grep -i battery | head -1) 2>/dev/null | grep -iE "state:|percentage:|time to|energy:|capacity:" | tee -a "$LOG"
for f in capacity status; do [ -e "/sys/class/power_supply/BAT0/$f" ] && say "BAT0/$f: $(cat /sys/class/power_supply/BAT0/$f)"; done
for z in /sys/class/thermal/thermal_zone*; do
    [ -e "$z" ] && say "$(cat "$z/type" 2>/dev/null): $(cat "$z/temp" 2>/dev/null) mC"
done

say ""
say "### WEBCAM"
ls /dev/video* 2>/dev/null | tee -a "$LOG"
command -v v4l2-ctl >/dev/null 2>&1 && v4l2-ctl --list-devices 2>&1 | tee -a "$LOG"

say ""
say "### WIFI (driver/firmware/scan; connect needs credentials)"
command -v iw >/dev/null 2>&1 && { ip link set wlan0 up 2>/dev/null; sleep 2; iw dev 2>&1 | tee -a "$LOG"; iw dev wlan0 scan 2>&1 | grep -E "SSID|signal" | head -20 | tee -a "$LOG"; } || say "iw: not present"
dmesg 2>/dev/null | grep -iE "ath10k|cfg80211|regulatory" | tail -8 | tee -a "$LOG"

say ""
say "### BLUETOOTH"
ls /sys/class/bluetooth/ 2>/dev/null | tee -a "$LOG"
command -v bluetoothctl >/dev/null 2>&1 && bluetoothctl show 2>&1 | grep -iE "Name|Class|Powered|Pairable" | tee -a "$LOG"

say ""
say "### SLEEP / RESUME (P2-T06) — $SLEEP_CYCLES cycles, RTC auto-wake ${SLEEP_WAKE}s"
i=1
while [ "$i" -le "$SLEEP_CYCLES" ]; do
    echo 0 > /sys/class/rtc/rtc0/wakealarm 2>/dev/null
    echo "+${SLEEP_WAKE}" > /sys/class/rtc/rtc0/wakealarm 2>/dev/null
    T0=$(date +%s)
    say "cycle $i pre-suspend $(date -u '+%H:%M:%S')"
    echo mem > /sys/power/state
    T1=$(date +%s)
    say "cycle $i resumed after $((T1-T0))s"
    sleep 2
    i=$((i+1))
done
dmesg 2>/dev/null | tail -12 | tee -a "$LOG"

say ""
say "### MANUAL (cannot be automated from SSH)"
say "  - Touchpad: run 'libinput debug-events' and tap/scroll/3-finger; expect POINTER/GESTURE events."
say "  - Brightness: press Fn+Up/Down; check /sys/class/backlight/intel_backlight/brightness changes."
say "  - Volume: press Fn+Up/Down; check 'amixer sget Master' or speaker output."
say "  - External display: plug HDMI/USB-C; check /sys/class/drm/card0-HDMI-A-1/status = connected."
say "  - Reboot to Windows and back (dual-boot coexistence)."

say ""
say "### SUMMARY"
say "log: $LOG"
say "AETHER_TEST_DONE"
