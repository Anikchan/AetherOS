#!/bin/sh
# AetherOS one-shot hardware validation (P2 / 04 §6).
# Run as root after boot:  sh /root/test.sh [--wifi-ssid NAME --wifi-password PASS]
#
# Wi-Fi credentials (P2-T03) are taken from arguments, then from the environment,
# then asked interactively when a terminal is attached:
#   sh /root/test.sh "MyNetwork" "my passphrase"
#   sh /root/test.sh --wifi-ssid MyNetwork --wifi-password 'my passphrase'
#   WIFI_SSID=MyNetwork WIFI_PSK='my passphrase' sh /root/test.sh
# The passphrase is only ever written to a root-only file under /run (tmpfs)
# and is removed on exit; it is never printed into the log.
#
# Writes a full log to /var/log/aether-test.log (tmpfs) and prints it to the console.
# I (the AI/SSH) read the log afterwards and continue from the markers.
#
# Optional env:
#   SLEEP_CYCLES=10   suspend/resume cycles (0 to skip)
#   SLEEP_WAKE=15     RTC auto-wake seconds per cycle

SLEEP_CYCLES="${SLEEP_CYCLES:-10}"
SLEEP_WAKE="${SLEEP_WAKE:-15}"
WIFI_SSID="${WIFI_SSID:-}"
WIFI_PSK="${WIFI_PSK:-${WIFI_PASSWORD:-}}"
WIFI_CONF=/run/aether-wpa.conf
WIFI_RESULT=skipped
LOG=/var/log/aether-test.log

usage() {
    cat <<EOF
usage: sh /root/test.sh [SSID PASSWORD]
       sh /root/test.sh --wifi-ssid SSID --wifi-password PASSWORD
       sh /root/test.sh --no-wifi

  SSID / PASSWORD   connect to that Wi-Fi network (P2-T03) and record the result
  --no-wifi         skip the connect step, keep the scan-only report
  -h, --help        this text

Environment: WIFI_SSID / WIFI_PSK, SLEEP_CYCLES, SLEEP_WAKE
The passphrase is never written to the log.
EOF
}

err() { printf '%s\n' "$*" >&2; }

# Positional order: SSID then passphrase. A value that starts with '-' has to
# follow `--` (a passphrase is allowed to look like an option).
set_positional() {
    if [ -z "$WIFI_SSID" ]; then
        WIFI_SSID="$1"
    elif [ -z "$WIFI_PSK" ]; then
        WIFI_PSK="$1"
    else
        err "unexpected argument: $1"; usage; exit 2
    fi
}

while [ $# -gt 0 ]; do
    case "$1" in
        --wifi-ssid|--ssid)
            [ $# -ge 2 ] || { err "missing value for $1"; exit 2; }
            WIFI_SSID="$2"; shift 2 ;;
        --wifi-password|--wifi-pass|--wifi-psk|--psk)
            [ $# -ge 2 ] || { err "missing value for $1"; exit 2; }
            WIFI_PSK="$2"; shift 2 ;;
        --no-wifi)
            WIFI_SSID=""; WIFI_PSK=""; WIFI_NO_WIFI=1; shift ;;
        -h|--help)
            usage; exit 0 ;;
        --)
            shift
            while [ $# -gt 0 ]; do set_positional "$1"; shift; done ;;
        -*)
            err "unknown option: $1"; usage; exit 2 ;;
        *)
            set_positional "$1"; shift ;;
    esac
done

# Credentials are collected FIRST, before the log file exists and before any
# output is written, so nothing typed here can end up in a transcript or a log.
# The passphrase is read with the terminal echo disabled.
ask_secret() {
    # $1 = prompt; the value is left in $_secret
    printf '%s' "$1" >&2
    if stty -echo 2>/dev/null; then
        IFS= read -r _secret
        stty echo 2>/dev/null
        printf '\n' >&2
    else
        IFS= read -r _secret || _secret=""
    fi
}

if [ -z "${WIFI_NO_WIFI:-}" ]; then
    if [ -z "$WIFI_SSID" ] && [ -t 0 ]; then
        printf 'Wi-Fi SSID (blank = skip the connect test): ' >&2
        IFS= read -r WIFI_SSID || WIFI_SSID=""
    fi
    if [ -n "$WIFI_SSID" ] && [ -z "$WIFI_PSK" ] && [ -t 0 ]; then
        ask_secret "Wi-Fi passphrase for \"$WIFI_SSID\" (not echoed, not logged): "
        WIFI_PSK="$_secret"
        unset _secret
    fi
fi
unset _secret 2>/dev/null || true

# The log is created only after the credentials are read.
# /var/log is tmpfs on AetherOS; the fallback keeps the script usable elsewhere
# (and quiet about it) without changing where the device writes its log.
if ! ( : > "$LOG" ) 2>/dev/null; then
    LOG=/tmp/aether-test.log
fi
( : > "$LOG" ) 2>/dev/null || true
trap 'rm -f "$WIFI_CONF"' EXIT HUP INT TERM

say() { printf '%s\n' "$*" | tee -a "$LOG"; }
run() { say "\$ $*"; "$@" 2>&1 | tee -a "$LOG"; }

if [ -n "${WIFI_NO_WIFI:-}" ]; then
    : # no credentials were asked for
elif [ -n "$WIFI_SSID" ] && [ -z "$WIFI_PSK" ]; then
    say "wifi: SSID given without a passphrase; the connect step will be skipped"
fi

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
say "-- native modes --"
for m in /sys/class/drm/card*-*/modes; do
    [ -e "$m" ] && { say "$m:"; head -3 "$m" | tee -a "$LOG"; }
done
say "-- framebuffer --"
ls /dev/fb* 2>/dev/null | tee -a "$LOG"
command -v glxinfo >/dev/null 2>&1 && glxinfo -B 2>&1 | grep -iE "renderer|vendor|OpenGL" | tee -a "$LOG"
command -v vulkaninfo >/dev/null 2>&1 && vulkaninfo 2>&1 | grep -iE "deviceName|driverName|apiVersion" | head -10 | tee -a "$LOG"

say ""
say "### AUDIO (P2-T04)"
command -v aplay >/dev/null 2>&1 && aplay -l 2>&1 | tee -a "$LOG"
cat /proc/asound/cards 2>/dev/null | tee -a "$LOG"
pgrep_ok() {
    # The image's busybox has no pgrep; pidof is the portable check here.
    if command -v pidof >/dev/null 2>&1; then
        pidof "$1" >/dev/null 2>&1
    else
        ps 2>/dev/null | grep -v grep | grep -q "$1"
    fi
}
pgrep_ok pipewire && say "pipewire: running" || say "pipewire: not running"
pgrep_ok wireplumber && say "wireplumber: running" || say "wireplumber: not running"
if command -v systemctl >/dev/null 2>&1; then
    for u in pipewire pipewire-pulse wireplumber; do
        say "$u.service: $(systemctl is-active "$u" 2>/dev/null)/$(systemctl is-enabled "$u" 2>/dev/null)"
    done
fi
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
say "### WIFI (P2-T03)"

wifi_iface() {
    # SYS_CLASS_NET exists so the script can be exercised on a host without
    # sysfs; on the device it is /sys/class/net.
    net="${SYS_CLASS_NET:-/sys/class/net}"
    for d in "$net"/*/wireless; do
        [ -e "$d" ] && { basename "$(dirname "$d")"; return 0; }
    done
    for d in "$net"/*; do
        case "$(basename "$d")" in
            wlan*|wlp*|wlx*) basename "$d"; return 0 ;;
        esac
    done
    return 1
}

# Escape a value for a quoted wpa_supplicant string.
wpa_escape() { printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }

IFACE=$(wifi_iface) || IFACE=""
if [ -n "$IFACE" ]; then
    say "wifi: interface $IFACE"
    ip link set "$IFACE" up 2>/dev/null
    sleep 1
    iw dev 2>&1 | tee -a "$LOG"
    iw dev "$IFACE" scan 2>&1 | grep -E "SSID|signal" | head -20 | tee -a "$LOG"
else
    say "wifi: no wireless interface found"
fi
dmesg 2>/dev/null | grep -iE "ath10k|cfg80211|regulatory" | tail -8 | tee -a "$LOG"

if [ -n "${WIFI_NO_WIFI:-}" ]; then
    say "WIFI_CONNECT_SKIPPED: --no-wifi"
elif [ -z "$WIFI_SSID" ] || [ -z "$WIFI_PSK" ]; then
    say "WIFI_CONNECT_SKIPPED: no SSID/passphrase given (sh /root/test.sh SSID PASSWORD)"
elif [ -z "$IFACE" ]; then
    WIFI_RESULT=fail
    say "WIFI_CONNECT_FAIL: no wireless interface found"
elif ! command -v wpa_supplicant >/dev/null 2>&1; then
    WIFI_RESULT=fail
    say "WIFI_CONNECT_FAIL: wpa_supplicant is not installed"
else
    say "-- connect --"
    say "wifi: ssid=\"$WIFI_SSID\" passphrase=<redacted>"
    # Root-only tmpfs file; removed by the EXIT trap. The passphrase never
    # goes to the log, to argv of a logged command, or to persistent storage.
    umask 077
    {
        printf 'ctrl_interface=/run/wpa_supplicant\n'
        printf 'network={\n'
        printf '    ssid="%s"\n' "$(wpa_escape "$WIFI_SSID")"
        printf '    psk="%s"\n' "$(wpa_escape "$WIFI_PSK")"
        printf '    key_mgmt=WPA-PSK\n'
        printf '}\n'
    } > "$WIFI_CONF"
    chmod 600 "$WIFI_CONF"
    umask 022

    pkill -f "wpa_supplicant.*$IFACE" 2>/dev/null
    sleep 1
    if wpa_supplicant -B -i "$IFACE" -c "$WIFI_CONF" -D nl80211 2>>"$LOG" \
        || wpa_supplicant -B -i "$IFACE" -c "$WIFI_CONF" -D wext 2>>"$LOG"; then
        say "wifi: wpa_supplicant started"
    else
        say "wifi: wpa_supplicant failed to start"
    fi

    # Association: up to 30 s.
    i=0
    while [ "$i" -lt 30 ]; do
        LINK=$(iw dev "$IFACE" link 2>/dev/null)
        case "$LINK" in *"Connected to"*) break ;; esac
        i=$((i+1))
        sleep 1
    done
    if [ "$i" -ge 30 ]; then
        WIFI_RESULT=fail
        say "WIFI_CONNECT_FAIL: no association after 30 s"
        iw dev "$IFACE" link 2>&1 | tee -a "$LOG"
        dmesg 2>/dev/null | grep -iE "wlan|ath10k|assoc|auth" | tail -10 | tee -a "$LOG"
    else
        say "WIFI_ASSOC: $(printf '%s' "$LINK" | tr '\n' ' ')"
        # Address: busybox udhcpc ships with the image and configures
        # /etc/resolv.conf through its default script.
        if command -v udhcpc >/dev/null 2>&1; then
            udhcpc -i "$IFACE" -t 8 -T 3 -q -n >>"$LOG" 2>&1
            say "wifi: udhcpc exit $?"
        fi
        ip -4 addr show "$IFACE" 2>&1 | tee -a "$LOG"
        GW=$(ip route 2>/dev/null | awk '/default/ {print $3; exit}')
        IP=$(ip -4 addr show "$IFACE" 2>/dev/null | awk '/inet / {print $2; exit}')
        say "WIFI_IP: ${IP:-none}"
        say "WIFI_GW: ${GW:-none}"
        if [ -n "$GW" ] && ping -c 2 -W 3 "$GW" >/dev/null 2>&1; then
            say "WIFI_PING: gateway ok"
        elif ping -c 2 -W 3 1.1.1.1 >/dev/null 2>&1; then
            say "WIFI_PING: gateway failed, internet ok"
        else
            say "WIFI_PING: no reply"
        fi
        if [ -n "$IP" ] && { ping -c 2 -W 3 1.1.1.1 >/dev/null 2>&1 || nslookup example.com >/dev/null 2>&1; }; then
            WIFI_RESULT=ok
            say "WIFI_CONNECT_OK: $IFACE ssid=\"$WIFI_SSID\""
        else
            WIFI_RESULT=fail
            say "WIFI_CONNECT_FAIL: associated but no usable connection"
        fi
    fi
    rm -f "$WIFI_CONF"
fi

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

if [ "$WIFI_RESULT" = ok ] && [ -n "$IFACE" ]; then
    say "-- wifi after ${SLEEP_CYCLES} suspend cycle(s) --"
    LINK=$(iw dev "$IFACE" link 2>/dev/null)
    case "$LINK" in
        *"Connected to"*) say "WIFI_AFTER_SUSPEND: still connected" ;;
        *) say "WIFI_AFTER_SUSPEND: link down (reconnect after resume needs the daemon)" ;;
    esac
fi

say ""
say "### MANUAL (cannot be automated from SSH)"
say "  - Touchpad: run 'libinput debug-events' and tap/scroll/3-finger; expect POINTER/GESTURE events."
say "  - Brightness: press Fn+Up/Down; check /sys/class/backlight/intel_backlight/brightness changes."
say "  - Volume: press Fn+Up/Down; check 'amixer sget Master' or speaker output."
say "  - External display: plug HDMI/USB-C; check /sys/class/drm/card0-HDMI-A-1/status = connected."
say "  - Reboot to Windows and back (dual-boot coexistence)."

say ""
say "### SUMMARY"
say "wifi connect: $WIFI_RESULT"
say "log: $LOG"
say "AETHER_TEST_DONE"
