#!/bin/bash
# Spike S4 (P0-T10): battery state through UPower and sysfs; charge-limit controls.
# Runs on a booted AetherOS/Linux machine with a battery. Read-only.
# Markers (grep-friendly):
#   S4_BATTERY_PRESENT / S4_BATTERY_ABSENT
#   S4_SYSFS_OK / S4_SYSFS_FAIL
#   S4_UPOWER_OK / S4_UPOWER_FAIL / S4_UPOWER_UNAVAILABLE
#   S4_CHARGE_LIMIT_SUPPORTED / S4_CHARGE_LIMIT_UNSUPPORTED
#   S4_TIME_REMAINING_OK / S4_TIME_REMAINING_NA
#   S4_BATTERY_SPIKE_DONE
set -u

echo "=== Spike S4 — battery (P0-T10) ==="
echo "Host: $(hostname)  Date: $(date -u '+%Y-%m-%d %H:%M UTC')"
echo "Kernel: $(uname -r)"

# --- 1. Battery presence via sysfs ---
BAT_FOUND=0
for BAT in /sys/class/power_supply/BAT*; do
    [ -e "$BAT" ] || continue
    BAT_FOUND=1
done

if [ "$BAT_FOUND" = "0" ]; then
    echo "S4_BATTERY_ABSENT: no /sys/class/power_supply/BAT*"
    echo "S4_BATTERY_SPIKE_DONE"
    exit 0
fi
echo "S4_BATTERY_PRESENT"

# --- 2. sysfs readings ---
SYSFS_OK=0
for BAT in /sys/class/power_supply/BAT*; do
    [ -e "$BAT" ] || continue
    echo "--- ${BAT##*/} ---"
    for f in type status capacity capacity_level energy_now energy_full \
             energy_full_design power_now voltage_now charge_now charge_full \
             charge_full_design current_now cycle_count health; do
        if [ -r "$BAT/$f" ]; then
            printf '  %s = %s\n' "$f" "$(cat "$BAT/$f" 2>/dev/null)"
        fi
    done
    SYSFS_OK=1
done
[ "$SYSFS_OK" = "1" ] && echo "S4_SYSFS_OK" || echo "S4_SYSFS_FAIL"

# --- 3. UPower readings (if available) ---
if command -v upower >/dev/null 2>&1; then
    UPOWER_OUT="$(upower --dump 2>/dev/null || true)"
    if [ -n "$UPOWER_OUT" ]; then
        echo "S4_UPOWER_OK"
        echo "--- upower --dump (battery lines) ---"
        printf '%s\n' "$UPOWER_OUT" | grep -iE "battery|percentage|time to (empty|full)|state:|energy|capacity|voltage|power" | head -40
    else
        echo "S4_UPOWER_FAIL: upower --dump produced no output"
    fi
    # Per-device detail for the first battery.
    BAT_DEV="$(upower -e 2>/dev/null | grep -i battery | head -1)"
    if [ -n "$BAT_DEV" ]; then
        echo "--- $BAT_DEV ---"
        upower --show-info "$BAT_DEV" 2>/dev/null | grep -iE "state:|percentage:|time to|energy:|capacity:|voltage:|power draw|warning-level:" || true
    fi
else
    echo "S4_UPOWER_UNAVAILABLE: upower binary not found (install upower)"
fi

# --- 4. Charge-limit control probe ---
CL_FOUND=0
for BAT in /sys/class/power_supply/BAT*; do
    [ -e "$BAT" ] || continue
    for f in charge_control_end_threshold charge_control_start_threshold \
             charge_behaviour charge_control_limit; do
        if [ -e "$BAT/$f" ]; then
            CL_FOUND=1
            echo "  ${BAT##*/}/$f = $(cat "$BAT/$f" 2>/dev/null)  (writable: $([ -w "$BAT/$f" ] && echo yes || echo no))"
        fi
    done
done
if [ "$CL_FOUND" = "1" ]; then
    echo "S4_CHARGE_LIMIT_SUPPORTED"
else
    echo "S4_CHARGE_LIMIT_UNSUPPORTED: no charge_control_* sysfs attributes"
fi

# --- 5. Time-remaining sanity (UPower-reported, if present) ---
if command -v upower >/dev/null 2>&1; then
    TR="$(upower -e 2>/dev/null | grep -i battery | head -1)"
    if [ -n "$TR" ]; then
        INFO="$(upower --show-info "$TR" 2>/dev/null)"
        if printf '%s\n' "$INFO" | grep -qi "time to"; then
            echo "S4_TIME_REMAINING_OK: $(printf '%s\n' "$INFO" | grep -i 'time to' | head -1 | tr -s ' ')"
        else
            echo "S4_TIME_REMAINING_NA: no time-to-empty/full reported (device may be full or AC-only)"
        fi
    fi
fi

echo "S4_BATTERY_SPIKE_DONE"
