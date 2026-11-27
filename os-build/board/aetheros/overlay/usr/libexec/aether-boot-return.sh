#!/bin/sh
# aether-boot-return — one-shot systemd job on AetherOS (every boot).
#
# Hands the *next* boot back to the internal disk (Ubuntu) exactly once, so the
# Dell ping-pongs between Ubuntu and the AetherOS USB stick without pressing
# F12:
#
#   Ubuntu:   aether-boot-switch aether && reboot   -> AetherOS (one boot)
#   AetherOS: this unit sets BootNext -> Ubuntu
#   reboot                                          -> Ubuntu
#
# Persistent variant: if `aether-boot-switch aether --persistent` was used, a
# BootOrder change is armed and recorded in /var/lib/aether-boot/original-order.
# This unit then restores that recorded order instead of writing BootNext, and
# keeps the existing escapes working. The trade-off of the persistent mode lives
# in docs/2026/11/28/results/boot-switch-ubuntu-aetheros.md (if AetherOS dies
# before restoring, the machine keeps booting AetherOS).
#
# The switch itself lives in /usr/bin/aether-boot-switch (one implementation,
# shared with the Ubuntu host helper). BootNext is deleted by the firmware once
# it has been used, so a failure here can never loop, and BootOrder is only
# touched when a restore record exists (every write is verified and rolled back
# on a mismatch).
#
# Opt out for this boot only:
#   - add aether.boot=stay to the kernel command line, or
#   - run: aether-boot-switch stay   (creates /run/aether-boot-stay)
#
# Exit status: 0 for "did it" and for every documented no-op (only one OS
# entry, no Ubuntu entry, efibootmgr missing, stay requested); 1 only when a
# BootNext write was attempted and could not be confirmed.
#
# Environment overrides (used by os-build/test/boot-switch-selftest.sh):
#   AETHER_SWITCH   path to aether-boot-switch (default /usr/bin/aether-boot-switch)
#   AETHER_RUN_DIR  stay-flag directory (default /run)
#   AETHER_CMDLINE  kernel command line file (default /proc/cmdline)
#   AETHER_LOG      log file, "-" disables it (default /var/log/aether-boot.log)

set -u

SWITCH=${AETHER_SWITCH:-/usr/bin/aether-boot-switch}
RUN_DIR=${AETHER_RUN_DIR:-/run}
STAY_FLAG=$RUN_DIR/aether-boot-stay
CMDLINE=${AETHER_CMDLINE:-/proc/cmdline}
LOG=${AETHER_LOG-/var/log/aether-boot.log}
PROG=aether-boot-return

# Journal (stdout/stderr, captured by systemd) plus a file the owner can read
# without journalctl. A failed file write is never fatal.
logfile() {
    [ -n "$LOG" ] && [ "$LOG" != "-" ] || return 0
    { printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$1" >>"$LOG"; } 2>/dev/null || true
}

log() {
    line="$PROG: $*"
    printf '%s\n' "$line"
    logfile "$line"
}

# 1. explicit opt-out on the kernel command line.
if [ -r "$CMDLINE" ]; then
    cmdline=$(cat "$CMDLINE" 2>/dev/null) || cmdline=""
    case " $cmdline " in
        *" aether.boot=stay "*)
            log "aether.boot=stay on the kernel command line; the next boot is left as it is."
            exit 0
            ;;
    esac
fi

# 2. explicit opt-out for this boot (aether-boot-switch stay).
if [ -e "$STAY_FLAG" ]; then
    log "stay flag present ($STAY_FLAG); the next boot is left as it is."
    exit 0
fi

# 3. degraded: no efibootmgr / no switch installed -> documented no-op.
if ! command -v "$SWITCH" >/dev/null 2>&1; then
    log "boot switch not installed ($SWITCH); nothing to do."
    exit 0
fi

# The switch's own output is already structured; keep its marker line verbatim
# so `grep '^AETHER_BOOT_SWITCH:'` works on both the journal and the log file.
relay() {
    printf '%s\n' "$1" | while IFS= read -r line; do
        [ -n "$line" ] || continue
        case "$line" in
            AETHER_BOOT_SWITCH:*)
                printf '%s\n' "$line"
                logfile "$line"
                ;;
            *)
                log "$line"
                ;;
        esac
    done
}

ORDER_FILE=${AETHER_STATE_DIR:-/var/lib/aether-boot}/original-order

# 4. persistent mode: the live BootOrder starts with the AetherOS entry (armed
# from Ubuntu with --persistent). Undo the order change instead of arming
# another BootNext, so the machine returns to Ubuntu and the table is what it
# was before the round. The record file is honoured when it is reachable; on
# AetherOS /var/lib is tmpfs, so the live BootOrder is the authority and
# `ubuntu --persistent` rebuilds the restore order from the table itself.
mode=$("$SWITCH" mode 2>/dev/null | sed -n 's/^AETHER_BOOT_MODE: //p')
case "$mode" in
    persistent)
        log "persistent BootOrder in effect (first entry is AetherOS); restoring the internal-disk order."
        if [ -e "$ORDER_FILE" ]; then
            log "restore record: $ORDER_FILE"
        else
            log "no restore record visible (AetherOS keeps /var/lib on tmpfs); rebuilding from the table."
        fi
        out=$("$SWITCH" ubuntu --persistent 2>&1)
        rc=$?
        relay "$out"
        case "$rc" in
            0)
                log "persistent switch undone: the internal disk is first again."
                exit 0
                ;;
            *)
                log "persistent restore did not complete (exit $rc); falling back to a one-shot BootNext return."
                ;;
        esac
        ;;
    *)
        # Not persistent. A record left on disk is stale (the order no longer
        # points at AetherOS), so drop it; otherwise it would sit there for ever.
        if [ -e "$ORDER_FILE" ]; then
            if rm -f "$ORDER_FILE" 2>/dev/null; then
                log "stale restore record removed (BootOrder no longer starts with AetherOS)."
            fi
        fi
        ;;
esac

out=$("$SWITCH" ubuntu 2>&1)
rc=$?
relay "$out"

case "$rc" in
    0) log "next boot handed back to the internal disk (BootNext)." ;;
    1) log "no internal-disk entry to return to (or only one OS entry exists); nothing written." ;;
    3) log "efibootmgr or the boot table is unavailable; nothing written (build with BR2_PACKAGE_EFIBOOTMGR=y)." ;;
    4) log "the write was not confirmed; check 'aether-boot-switch status' on the next boot."
       exit 1 ;;
    *) log "unexpected exit $rc from $SWITCH; nothing was assumed."
       exit 1 ;;
esac

exit 0
