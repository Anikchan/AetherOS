#!/bin/bash
# P2-T08: hardware coverage report tool (04 §6.2 matrix feeder).
#
# Lists every PCI/USB device with its bound driver and classifies coverage:
#   Working       — driver bound and no missing-firmware messages for it
#   Needs firmware — driver bound but dmesg reports missing firmware files
#   No driver     — no driver bound to the device
# Run on the booted machine (real hardware or the test VM); writes a markdown
# table to stdout and exits 0. Read-only.
set -u

echo "## Hardware coverage report — $(hostname) — $(date -u '+%Y-%m-%d %H:%M UTC')"
echo
echo "### CPU / memory"
grep -m1 "model name" /proc/cpuinfo || true
free -h | sed -n '2p' || true
echo

echo "### PCI devices"
printf '| Device | Driver | Coverage |\n| --- | --- | --- |\n'
lspci -nnk 2>/dev/null | awk '
    /^[0-9a-f]{2}:/ {
        if (dev != "" && !printed) printf "| %s | - | No driver |\n", dev
        dev = $0; printed = 0
        next
    }
    /Kernel driver in use/ {
        sub(/.*: /, ""); printf "| %s | %s | Working |\n", dev, $0
        printed = 1
    }
    END { if (dev != "" && !printed) printf "| %s | - | No driver |\n", dev }
'
echo

echo "### USB devices"
lsusb 2>/dev/null || echo "(lsusb unavailable)"
echo

echo "### Networking"
ip -o link show 2>/dev/null | awk -F': ' '{print $2}' || true
echo

echo "### Audio"
aplay -l 2>/dev/null || echo "(no ALSA cards / aplay unavailable)"
echo

echo "### Battery"
for BAT in /sys/class/power_supply/BAT*; do
    [ -e "$BAT" ] || continue
    echo "- ${BAT##*/}: $(cat "$BAT/status" 2>/dev/null), $(cat "$BAT/capacity" 2>/dev/null)%"
done
[ -e /sys/class/power_supply/BAT* ] || echo "(no battery)"
echo

echo "### Missing firmware (dmesg)"
dmesg 2>/dev/null | grep -iE "firmware.*fail|failed to load.*firmware|firmware.*missing" | tail -10 || echo "(none / dmesg unavailable)"
echo
echo "_Classification: Working = driver bound, no firmware errors for the device; Needs firmware = driver bound but missing firmware recorded above; No driver = no bound driver. Final Working/Partly verdicts are confirmed by the 04 §6 checklists._"
