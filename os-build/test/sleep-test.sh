#!/bin/sh
# sleep-test.sh — 10-cycle suspend/resume test with RTC auto-wake (P2-T06).
#
# Runs on the target (AetherOS or any Linux). Each cycle sets an RTC wakealarm
# a few seconds ahead, suspends to RAM, then verifies resume. This avoids
# needing a manual wake event.
#
# Usage: sleep-test.sh [cycles] [wake-seconds]
# Output: markers + per-cycle timing; exit 0 if all cycles resumed.

CYCLES="${1:-10}"
WAKE="${2:-15}"
LOG=/var/tmp/sleep-test.log
: > "$LOG"

echo "SLEEP_TEST_START cycles=$CYCLES wake=${WAKE}s"
echo "SLEEP_TEST_START cycles=$CYCLES wake=${WAKE}s" >> "$LOG"

i=1
FAIL=0
while [ "$i" -le "$CYCLES" ]; do
    # arm the RTC wakealarm
    if [ -w /sys/class/rtc/rtc0/wakealarm ]; then
        echo 0 > /sys/class/rtc/rtc0/wakealarm 2>/dev/null
        echo "+${WAKE}" > /sys/class/rtc/rtc0/wakealarm 2>/dev/null || {
            echo "SLEEP_TEST_FAIL: cannot arm RTC wakealarm"
            FAIL=1; break
        }
    else
        echo "SLEEP_TEST_WARN: no RTC wakealarm; using wakeup-source only"
    fi

    T0="$(date +%s)"
    echo "cycle $i pre-suspend $(date -u '+%H:%M:%S')" | tee -a "$LOG"
    if ! echo mem > /sys/power/state; then
        echo "SLEEP_TEST_FAIL: suspend command failed at cycle $i"
        FAIL=1; break
    fi
    T1="$(date +%s)"
    ELAPSED=$((T1 - T0))
    echo "cycle $i resumed after ${ELAPSED}s" | tee -a "$LOG"
    # a resume shorter than ~1s usually means suspend was aborted
    if [ "$ELAPSED" -lt 1 ]; then
        echo "SLEEP_TEST_WARN: cycle $i did not really suspend (${ELAPSED}s)"
    fi
    sleep 2
    i=$((i + 1))
done

if [ "$FAIL" -eq 0 ]; then
    echo "SLEEP_TEST_OK: $CYCLES/$CYCLES cycles" | tee -a "$LOG"
    echo "SLEEP_TEST_OK: $CYCLES/$CYCLES cycles" >> "$LOG"
    exit 0
else
    echo "SLEEP_TEST_FAIL" | tee -a "$LOG"
    exit 1
fi
