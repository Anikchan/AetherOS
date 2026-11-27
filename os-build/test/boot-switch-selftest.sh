#!/usr/bin/env bash
# boot-switch-selftest.sh — prove the AetherOS <-> Ubuntu boot switching logic
# (one-shot BootNext and the opt-in persistent BootOrder mode) without UEFI
# hardware, so it is testable on macOS and in CI.
#
# It builds a fake `efibootmgr` backed by files (table, BootNext, BootOrder) and
# drives the real scripts:
#   overlay/usr/bin/aether-boot-switch        (the one implementation)
#   overlay/usr/libexec/aether-boot-return.sh (the every-boot systemd job)
#
# Covered:
#   1  status reports the table and a stable marker line
#   2  entry exists            -> BootNext written and read back
#   3  BootNext already right  -> idempotent, no write at all
#   4  ubuntu direction        -> BootNext written and read back
#   5  entry missing           -> created, then written
#   6  no Ubuntu entry         -> no-op with a clear message
#   7  one OS entry only       -> no-op ("nothing to switch back to")
#   8  aether.boot=stay        -> the return unit writes nothing
#   9  /run/aether-boot-stay   -> the return unit writes nothing
#  10  write failure           -> exit 4, BootNext unchanged
#  11  silent write failure    -> exit 4, BootNext unchanged (no partial state)
#  12  efibootmgr missing      -> exit 3 no-op; the unit still exits 0
#  13  ambiguous ubuntu entry  -> exit 1, refuses to guess
#  14  inactive AetherOS entry -> exit 1, refuses to use it
#  15  unreadable boot table   -> exit 3 no-op
#  16  clear                   -> BootNext deleted / already unset
#  17  the happy boot cycle    -> AetherOS boot hands the next boot to Ubuntu
#  18  BootOrder is untouched by a one-shot switch
#  19  sh -n / bash -n clean, host symlink resolves to the same file
#  20  a real efibootmgr -v dump parses (labels with spaces, UEFI: prefix)
#  21  --persistent armed: BootOrder written, original recorded, no BootNext
#  22  --persistent idempotent: second arm writes nothing
#  23  --persistent round trip: Ubuntu flips -> AetherOS restores, byte for byte
#  24  --persistent restore with no record: internal first, AetherOS last
#  25  persistent: no Ubuntu entry -> no-op, never fabricates an entry
#  26  persistent: AetherOS entry missing -> no-op, order untouched
#  27  persistent: record references a deleted entry -> dead index dropped
#  28  persistent: record with only dead entries -> refuse, table untouched
#  29  persistent: write not permitted -> exit 4, order unchanged, record kept
#  30  persistent: silent write failure -> exit 4, order unchanged
#  31  persistent: duplicate indexes in a record are collapsed
#  32  persistent: AetherOS already first, no record -> derived record, no write
#  33  persistent: AetherOS is the only entry -> refuse (cannot be undone)
#  34  status reports mode=next / persistent / default, and "not reported"
#  35  clear removes BootNext AND the recorded order
#  36  clear with a failing order write keeps the record and exits 4
#  37  unit: stale record -> record dropped, one-shot BootNext used instead
#
# Usage: os-build/test/boot-switch-selftest.sh [-v]
# Exit: 0 all cases pass, 1 a case failed.

set -uo pipefail

SELF_DIR=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$SELF_DIR/../.." && pwd)
OVERLAY=$REPO/os-build/board/aetheros/overlay
CANON=$OVERLAY/usr/bin/aether-boot-switch
RETURN=$OVERLAY/usr/libexec/aether-boot-return.sh
UNIT=$OVERLAY/etc/systemd/system/aether-boot-return.service
HOSTLINK=$REPO/os-build/scripts/aether-boot-switch.sh

VERBOSE=0
[ "${1:-}" = "-v" ] && VERBOSE=1

pass=0
fail=0

ok() {
    pass=$((pass + 1))
    printf 'ok %2d - %s\n' "$pass" "$1"
}

bad() {
    fail=$((fail + 1))
    printf 'not ok %2d - %s\n' "$((pass + fail))" "$1"
    [ -n "${2:-}" ] && printf '         %s\n' "$2"
}

expect_rc() { # desc actual expected
    if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "exit $2, expected $3"; fi
}

expect_eq() { # desc actual expected
    if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "got [$2], expected [$3]"; fi
}

expect_has() { # desc text needle
    if printf '%s\n' "$2" | grep -Fq -- "$3"; then ok "$1"; else bad "$1" "missing [$3] in: $2"; fi
}

expect_lacks() { # desc text needle
    if printf '%s\n' "$2" | grep -Fq -- "$3"; then bad "$1" "unexpected [$3] in: $2"; else ok "$1"; fi
}

# --- fixtures ---------------------------------------------------------------
T=""
setup() {
    [ -n "$T" ] && rm -rf "$T"
    T=$(mktemp -d "${TMPDIR:-/tmp}/aether-boot-switch.XXXXXX")
    export FAKE_STATE=$T/state
    mkdir -p "$FAKE_STATE" "$T/run"
    : >"$FAKE_STATE/entries"
    : >"$FAKE_STATE/calls"
    printf '0000\n' >"$FAKE_STATE/current"
    printf '0000\n' >"$FAKE_STATE/order"
    printf 'a0e50001-0000-4000-8000-000000000001\n' >"$FAKE_STATE/esp-uuid"
    printf 'root=PARTUUID=deadbeef console=ttyS0\n' >"$T/cmdline"

    export AETHER_EFIBOOTMGR=$T/fake-efibootmgr
    export AETHER_LOG=$T/boot.log
    export AETHER_RUN_DIR=$T/run
    export AETHER_CMDLINE=$T/cmdline
    export AETHER_STATE_DIR=$T/persist
    export AETHER_USB_PART=/dev/fake-usb1
    export AETHER_USB_DISK=/dev/fake-usb
    export AETHER_USB_PARTNUM=1
    export AETHER_USB_PARTUUID=a0e50001-0000-4000-8000-000000000001
    export AETHER_SWITCH=$CANON

    write_fake
}

cleanup() { [ -n "$T" ] && rm -rf "$T"; }
trap cleanup EXIT

write_fake() {
    cat >"$T/fake-efibootmgr" <<'FAKE'
#!/bin/sh
# fake-efibootmgr — state-file stand-in used by boot-switch-selftest.sh.
# State: entries (idx<TAB>marker<TAB>label<TAB>path), next, current, order, calls
STATE=${FAKE_STATE:?FAKE_STATE not set}
mkdir -p "$STATE" 2>/dev/null
ENTRIES=$STATE/entries
: >>"$ENTRIES"
printf '%s\n' "$*" >>"$STATE/calls"

mode=table
verbose=0
nextval=""
neworder=""
pnum=""
label=""
loader=""
while [ $# -gt 0 ]; do
    case "$1" in
        -v) verbose=1; shift ;;
        -n) nextval=${2:-}; mode=set; shift 2 ;;
        -N) mode=clear; shift ;;
        -c) mode=create; shift ;;
        -o) neworder=${2:-}; mode=order; shift 2 ;;
        -d) shift 2 ;;
        -p) pnum=${2:-}; shift 2 ;;
        -L) label=${2:-}; shift 2 ;;
        -l) loader=${2:-}; shift 2 ;;
        *) shift ;;
    esac
done

print_table() {
    printf 'BootCurrent: %s\n' "$(cat "$STATE/current" 2>/dev/null || echo 0000)"
    printf 'Timeout: 1 seconds\n'
    # Firmware without BootOrder support: the variable is simply not reported.
    [ -f "$STATE/suppress-order" ] || printf 'BootOrder: %s\n' "$(cat "$STATE/order" 2>/dev/null)"
    n=$(cat "$STATE/next" 2>/dev/null || true)
    [ -n "$n" ] && printf 'BootNext: %s\n' "$n"
    awk -F'\t' -v v="$verbose" '
        { if (v == 1) printf "Boot%s%s %s\t%s\n", $1, $2, $3, $4
          else        printf "Boot%s%s %s\n", $1, $2, $3 }' "$ENTRIES"
    return 0
}

case "$mode" in
    table)
        if [ -f "$STATE/fail-table" ]; then
            echo "EFI variables are not supported on this system." >&2
            exit 1
        fi
        print_table
        ;;
    set)
        if [ -f "$STATE/fail-next" ]; then
            echo "Could not set BootNext: Operation not permitted" >&2
            exit 1
        fi
        [ -f "$STATE/ignore-next" ] || printf '%s\n' "$nextval" >"$STATE/next"
        print_table
        ;;
    clear)
        rm -f "$STATE/next"
        print_table
        ;;
    order)
        if [ -f "$STATE/fail-order" ]; then
            echo "Could not set BootOrder: Operation not permitted" >&2
            exit 1
        fi
        [ -f "$STATE/ignore-order" ] || printf '%s\n' "$neworder" >"$STATE/order"
        print_table
        ;;
    create)
        if [ -f "$STATE/fail-create" ]; then
            echo "Could not create variable: No space left on device" >&2
            exit 1
        fi
        uuid=$(cat "$STATE/esp-uuid" 2>/dev/null || echo 0000-0000)
        newidx=$(awk -F'\t' '
            BEGIN { m = -1 }
            { v = $1; sub(/^0+/, "", v); if (v == "") v = 0; if (v + 0 > m) m = v + 0 }
            END { printf "%04X", m + 1 }' "$ENTRIES")
        printf '%s\t*\t%s\tHD(%s,GPT,%s)/File(%s)\n' \
            "$newidx" "$label" "$pnum" "$uuid" "$loader" >>"$ENTRIES"
        order=$(cat "$STATE/order" 2>/dev/null || true)
        if [ -n "$order" ]; then
            printf '%s,%s\n' "$newidx" "$order" >"$STATE/order"
        else
            printf '%s\n' "$newidx" >"$STATE/order"
        fi
        print_table
        ;;
esac
exit 0
FAKE
    chmod +x "$T/fake-efibootmgr"
}

# add_entry <idx> <on|off> <label> <path>
add_entry() {
    if [ "$2" = "on" ]; then marker='*'; else marker=' '; fi
    printf '%s\t%s\t%s\t%s\n' "$1" "$marker" "$3" "${4:-HD(1,GPT,a0e50001-0000-4000-8000-000000000001)/File(\\EFI\\BOOT\\BOOTX64.EFI)}" \
        >>"$FAKE_STATE/entries"
}

UBUNTU_PATH='HD(1,GPT,1111aaaa-2222-3333-4444-555566667777)/File(\EFI\ubuntu\shimx64.efi)'
AETHER_PATH='HD(1,GPT,a0e50001-0000-4000-8000-000000000001)/File(\EFI\BOOT\BOOTX64.EFI)'

std_fixture() { # ubuntu 0000 + AetherOS 0001, booted from ubuntu
    add_entry 0000 on ubuntu "$UBUNTU_PATH"
    add_entry 0001 on AetherOS "$AETHER_PATH"
    printf '0000,0001\n' >"$FAKE_STATE/order"
    printf '0000\n' >"$FAKE_STATE/current"
}

sw() { sh "$CANON" "$@"; }
ret() { sh "$RETURN"; }
next_now() { cat "$FAKE_STATE/next" 2>/dev/null || true; }
order_now() { cat "$FAKE_STATE/order" 2>/dev/null || true; }
record_now() { cat "$AETHER_STATE_DIR/original-order" 2>/dev/null || true; }
called() { grep -Fq -- "$1" "$FAKE_STATE/calls"; }
wrote_order() { grep -Fq -- '-o ' "$FAKE_STATE/calls"; }

# --- 19: syntax and the host symlink (cheap, run first) ---------------------
printf '# TAP-ish selftest for aether-boot-switch\n'
for f in "$CANON" "$RETURN"; do
    if sh -n "$f" 2>/dev/null; then ok "sh -n $(basename "$f")"; else bad "sh -n $(basename "$f")"; fi
    if bash -n "$f" 2>/dev/null; then ok "bash -n $(basename "$f")"; else bad "bash -n $(basename "$f")"; fi
done
if [ -L "$HOSTLINK" ]; then ok "os-build/scripts/aether-boot-switch.sh is a symlink (one implementation)"; else bad "host helper is not a symlink"; fi
if [ "$(readlink -f "$HOSTLINK" 2>/dev/null || python3 -c 'import os,sys;print(os.path.realpath(sys.argv[1]))' "$HOSTLINK")" = "$(readlink -f "$CANON" 2>/dev/null || python3 -c 'import os,sys;print(os.path.realpath(sys.argv[1]))' "$CANON")" ]; then
    ok "host symlink resolves to the image binary"
else
    bad "host symlink does not resolve to $CANON"
fi
if grep -Fq 'ExecStart=/usr/libexec/aether-boot-return.sh' "$UNIT"; then ok "unit runs the return script"; else bad "unit ExecStart missing"; fi
if grep -Fq 'RemainAfterExit=yes' "$UNIT" && grep -Fq 'After=local-fs.target' "$UNIT" && grep -Fq 'Type=oneshot' "$UNIT"; then
    ok "unit is oneshot, after local-fs, RemainAfterExit"
else
    bad "unit is missing oneshot/After/RemainAfterExit"
fi

# --- 1: status --------------------------------------------------------------
setup; std_fixture
out=$(sw status); rc=$?
expect_rc "status exits 0" "$rc" 0
expect_has "status marker line" "$out" "AETHER_BOOT_SWITCH: next=none label=none current=0000 result=status"
expect_has "status lists the ubuntu entry" "$out" "0000 on  ubuntu"
expect_has "status lists the AetherOS entry" "$out" "0001 on  AetherOS"
expect_has "status logs to the file" "$(cat "$AETHER_LOG" 2>/dev/null)" "AETHER_BOOT_SWITCH: next=none"
[ "$(next_now)" = "" ] && ok "status writes no BootNext" || bad "status wrote BootNext" "$(next_now)"

# --- 2: entry exists -> BootNext written and read back ----------------------
setup; std_fixture
out=$(sw aether); rc=$?
expect_rc "aether exits 0" "$rc" 0
expect_eq "aether writes BootNext=0001" "$(next_now)" 0001
expect_has "aether marker" "$out" "AETHER_BOOT_SWITCH: next=0001 label=AetherOS current=0000 result=switched"
expect_eq "aether leaves BootOrder alone" "$(order_now)" "0000,0001"

# --- 3: idempotent ----------------------------------------------------------
setup; std_fixture; printf '0001\n' >"$FAKE_STATE/next"
out=$(sw aether); rc=$?
expect_rc "idempotent aether exits 0" "$rc" 0
expect_has "idempotent aether says already" "$out" "result=already"
if called '-n'; then bad "idempotent aether still wrote BootNext"; else ok "idempotent aether performs no write"; fi

# --- 4: ubuntu direction ----------------------------------------------------
setup; std_fixture
printf '0001\n' >"$FAKE_STATE/current"
out=$(sw ubuntu); rc=$?
expect_rc "ubuntu exits 0" "$rc" 0
expect_eq "ubuntu writes BootNext=0000" "$(next_now)" 0000
expect_has "ubuntu marker" "$out" "AETHER_BOOT_SWITCH: next=0000 label=ubuntu current=0001 result=switched"

# --- 5: entry missing -> created then written -------------------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
printf '0000\n' >"$FAKE_STATE/order"; printf '0000\n' >"$FAKE_STATE/current"
out=$(sw aether); rc=$?
expect_rc "create path exits 0" "$rc" 0
expect_has "create called efibootmgr -c" "$(cat "$FAKE_STATE/calls")" "-c"
expect_has "create used -L AetherOS" "$(cat "$FAKE_STATE/calls")" "-L AetherOS"
expect_has "create used the USB disk" "$(cat "$FAKE_STATE/calls")" "-d /dev/fake-usb"
expect_has "create used partition 1" "$(cat "$FAKE_STATE/calls")" "-p 1"
expect_has "create used the BOOTX64 loader" "$(cat "$FAKE_STATE/calls")" '\EFI\BOOT\BOOTX64.EFI'
expect_eq "created entry gets BootNext" "$(next_now)" 0001
expect_has "create marker" "$out" "result=created"
expect_eq "create keeps the old BootOrder tail" "$(order_now)" "0001,0000"
if [ -f "$FAKE_STATE/entries" ] && grep -Fq 'AetherOS' "$FAKE_STATE/entries"; then
    ok "created entry landed in the table"
else
    bad "created entry missing from the table"
fi

# --- 6: no Ubuntu entry -> no-op with a clear message -----------------------
setup
add_entry 0000 on AetherOS "$AETHER_PATH"
add_entry 0002 on 'USB HDD' "$UBUNTU_PATH"
printf '0000,0002\n' >"$FAKE_STATE/order"; printf '0000\n' >"$FAKE_STATE/current"
out=$(sw ubuntu 2>&1); rc=$?
expect_rc "no ubuntu entry exits 1" "$rc" 1
expect_has "no ubuntu entry explains itself" "$out" "no internal-disk (Ubuntu) entry found"
expect_has "no ubuntu entry marker is noop" "$out" "AETHER_BOOT_SWITCH: next=none label=none current=0000 result=noop"
[ "$(next_now)" = "" ] && ok "no ubuntu entry leaves BootNext unset" || bad "no ubuntu entry wrote BootNext"

# --- 7: only one OS entry ---------------------------------------------------
setup
add_entry 0000 on AetherOS "$AETHER_PATH"
printf '0000\n' >"$FAKE_STATE/order"; printf '0000\n' >"$FAKE_STATE/current"
out=$(sw ubuntu 2>&1); rc=$?
expect_rc "single entry exits 1 (no-op)" "$rc" 1
expect_has "single entry explains itself" "$out" "only one OS entry exists"
[ "$(next_now)" = "" ] && ok "single entry writes nothing" || bad "single entry wrote BootNext"

# --- 8: aether.boot=stay ----------------------------------------------------
setup; std_fixture
printf 'root=/dev/sda2 aether.boot=stay quiet\n' >"$AETHER_CMDLINE"
out=$(ret 2>&1); rc=$?
expect_rc "stay via cmdline: unit exits 0" "$rc" 0
expect_has "stay via cmdline is logged" "$out" "aether.boot=stay"
[ "$(next_now)" = "" ] && ok "stay via cmdline writes no BootNext" || bad "stay via cmdline wrote BootNext"

# --- 9: /run/aether-boot-stay ----------------------------------------------
setup; std_fixture
: >"$T/run/aether-boot-stay"
out=$(ret 2>&1); rc=$?
expect_rc "stay via flag: unit exits 0" "$rc" 0
expect_has "stay via flag is logged" "$out" "stay flag present"
[ "$(next_now)" = "" ] && ok "stay via flag writes no BootNext" || bad "stay via flag wrote BootNext"

# --- 10: write failure ------------------------------------------------------
setup; std_fixture; printf '0000\n' >"$FAKE_STATE/next"
: >"$FAKE_STATE/fail-next"
out=$(sw aether 2>&1); rc=$?
expect_rc "failed write exits 4" "$rc" 4
expect_has "failed write is explained" "$out" "was not confirmed"
expect_has "failed write marker" "$out" "result=failed"
expect_eq "failed write leaves BootNext unchanged" "$(next_now)" 0000
expect_eq "failed write leaves BootOrder unchanged" "$(order_now)" "0000,0001"

# --- 11: silent write failure (no partial state) ---------------------------
setup; std_fixture; printf '0000\n' >"$FAKE_STATE/next"
: >"$FAKE_STATE/ignore-next"
out=$(sw aether 2>&1); rc=$?
expect_rc "unconfirmed write exits 4" "$rc" 4
expect_has "unconfirmed write says BootNext reads back as 0000" "$out" "BootNext reads back as 0000"
expect_eq "unconfirmed write leaves BootNext untouched" "$(next_now)" 0000

# --- 12: efibootmgr missing -------------------------------------------------
setup; std_fixture
export AETHER_EFIBOOTMGR=/nonexistent/efibootmgr
out=$(sw aether 2>&1); rc=$?
expect_rc "missing efibootmgr exits 3" "$rc" 3
expect_has "missing efibootmgr is a no-op" "$out" "result=noop"
out=$(ret 2>&1); rc=$?
expect_rc "unit survives a missing efibootmgr" "$rc" 0
expect_has "unit reports the degraded state" "$out" "efibootmgr is not available"
export AETHER_EFIBOOTMGR=$T/fake-efibootmgr

# --- 13: ambiguous ubuntu ---------------------------------------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
add_entry 0002 on 'Fedora' 'HD(1,GPT,99999999-0000-0000-0000-000000000000)/File(\EFI\fedora\shimx64.efi)'
add_entry 0003 on ubuntu-server 'HD(1,GPT,88888888-0000-0000-0000-000000000000)/File(\EFI\ubuntu\grubx64.efi)'
printf '0000,0002,0003\n' >"$FAKE_STATE/order"; printf '0000\n' >"$FAKE_STATE/current"
out=$(sw ubuntu 2>&1); rc=$?
expect_rc "ambiguous ubuntu exits 1" "$rc" 1
expect_has "ambiguous ubuntu refuses to guess" "$out" "refusing to guess"
expect_has "ambiguous ubuntu lists candidates" "$out" "0003* ubuntu-server"
[ "$(next_now)" = "" ] && ok "ambiguous ubuntu writes nothing" || bad "ambiguous ubuntu wrote BootNext"

# --- 14: inactive AetherOS entry -------------------------------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
add_entry 0001 off AetherOS "$AETHER_PATH"
printf '0000,0001\n' >"$FAKE_STATE/order"; printf '0000\n' >"$FAKE_STATE/current"
out=$(sw aether 2>&1); rc=$?
expect_rc "inactive AetherOS exits 1" "$rc" 1
expect_has "inactive AetherOS is explained" "$out" "deactivated"
expect_lacks "inactive AetherOS is not used" "$out" "result=switched"
[ "$(next_now)" = "" ] && ok "inactive AetherOS writes nothing" || bad "inactive AetherOS wrote BootNext"

# --- 15: unreadable boot table ---------------------------------------------
setup; std_fixture
: >"$FAKE_STATE/fail-table"
out=$(sw aether 2>&1); rc=$?
expect_rc "unreadable table exits 3" "$rc" 3
expect_has "unreadable table is a no-op" "$out" "result=noop"
out=$(ret 2>&1); rc=$?
expect_rc "unit survives an unreadable table" "$rc" 0

# --- 16: clear --------------------------------------------------------------
setup; std_fixture; printf '0001\n' >"$FAKE_STATE/next"
out=$(sw clear); rc=$?
expect_rc "clear exits 0" "$rc" 0
[ "$(next_now)" = "" ] && ok "clear deletes BootNext" || bad "clear left BootNext"
expect_has "clear marker" "$out" "result=cleared"
out=$(sw clear); rc=$?
expect_rc "clear when already unset exits 0" "$rc" 0
expect_has "clear when already unset is a no-op" "$out" "result=already"

# --- 17: the happy boot cycle ----------------------------------------------
setup; std_fixture
# Ubuntu side: one command, then reboot -> AetherOS
sw aether >/dev/null 2>&1
expect_eq "cycle: Ubuntu sets BootNext to AetherOS" "$(next_now)" 0001
# AetherOS boots, firmware consumes BootNext, the unit hands it back
rm -f "$FAKE_STATE/next"; printf '0001\n' >"$FAKE_STATE/current"
out=$(ret 2>&1); rc=$?
expect_rc "cycle: return unit exits 0" "$rc" 0
expect_eq "cycle: AetherOS sets BootNext back to ubuntu" "$(next_now)" 0000
expect_has "cycle: return marker is in the journal output" "$out" "AETHER_BOOT_SWITCH: next=0000 label=ubuntu current=0001 result=switched"
expect_has "cycle: return marker is in /var/log/aether-boot.log" "$(cat "$AETHER_LOG" 2>/dev/null)" "AETHER_BOOT_SWITCH: next=0000"
# Ubuntu boots again; nothing re-arms the switch
rm -f "$FAKE_STATE/next"; printf '0000\n' >"$FAKE_STATE/current"
[ "$(next_now)" = "" ] && ok "cycle: no BootNext left behind after the round trip" || bad "cycle: BootNext leaked"

# --- 18: BootOrder is never rewritten by a switch --------------------------
setup; std_fixture
before=$(order_now)
sw aether >/dev/null 2>&1
sw ubuntu >/dev/null 2>&1
expect_eq "switch never rewrites BootOrder" "$(order_now)" "$before"

# --- 20: a real efibootmgr -v dump parses (labels with spaces, UEFI: prefix) -
setup
cat >"$T/real-dump" <<'DUMP'
BootCurrent: 0003
Timeout: 1 seconds
BootOrder: 0003,0000,0001,0002
BootNext: 0002
Boot0000* ubuntu	HD(1,GPT,78a2b1c4-0000-4000-8000-0000000000aa,0x800,0x100000)/File(\EFI\ubuntu\shimx64.efi)
Boot0001* UEFI: SanDisk Extreme 0001	PciRoot(0x0)/Pci(0x14,0x0)/USB(0x2,0x0)/HD(1,GPT,a0e50001-0000-4000-8000-000000000001,0x800,0x20000)/File(\EFI\BOOT\BOOTX64.EFI)
Boot0002* Windows Boot Manager	HD(1,GPT,78a2b1c4-0000-4000-8000-0000000000aa,0x800,0x100000)/File(\EFI\Microsoft\Boot\bootmgfw.efi)WINDOWS
Boot0003* AetherOS	HD(1,GPT,a0e50001-0000-4000-8000-000000000001,0x800,0x20000)/File(\EFI\BOOT\BOOTX64.EFI)
DUMP
cat >"$T/fake-efibootmgr" <<'CATFAKE'
#!/bin/sh
# Replays a captured `efibootmgr -v` dump; only BootNext may change.
case " $* " in
    *" -n "*)
        sed "s/^BootNext: .*/BootNext: $2/" "$DUMP_FILE" >"$DUMP_FILE.new" &&
            mv "$DUMP_FILE.new" "$DUMP_FILE"
        exit 0
        ;;
    *" -c "*|*" -N "*)
        echo "fake: create/clear not expected in the dump test" >&2
        exit 9
        ;;
esac
cat "$DUMP_FILE"
CATFAKE
chmod +x "$T/fake-efibootmgr"
export DUMP_FILE=$T/real-dump
out=$(sw status); rc=$?
expect_rc "real dump: status exits 0" "$rc" 0
expect_has "real dump: marker" "$out" "AETHER_BOOT_SWITCH: next=0002 label=Windows Boot Manager current=0003 result=status"
expect_has "real dump: label with spaces kept" "$out" "0001 on  UEFI: SanDisk Extreme 0001"
expect_has "real dump: AetherOS entry kept" "$out" "0003 on  AetherOS"
out=$(sw ubuntu); rc=$?
expect_rc "real dump: ubuntu picked by label" "$rc" 0
expect_has "real dump: ubuntu marker" "$out" "next=0000 label=ubuntu current=0003 result=switched"
out=$(sw aether 2>&1); rc=$?
expect_rc "real dump: aether prefers the USB-disk entry" "$rc" 0
expect_has "real dump: AetherOS label chosen over 'UEFI: SanDisk'" "$out" "label=AetherOS"

# --- fake-create failure keeps the table intact ---------------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
printf '0000\n' >"$FAKE_STATE/order"
: >"$FAKE_STATE/fail-create"
out=$(sw aether 2>&1); rc=$?
expect_rc "failed create exits 4" "$rc" 4
expect_has "failed create is explained" "$out" "efibootmgr -c failed"
[ "$(next_now)" = "" ] && ok "failed create writes no BootNext" || bad "failed create wrote BootNext"

# ==========================================================================
# Persistent mode (--persistent): BootOrder instead of BootNext
# ==========================================================================

# --- 21: arm ---------------------------------------------------------------
setup; std_fixture            # order 0000,0001 (ubuntu, AetherOS), booted from ubuntu
out=$(sw aether --persistent); rc=$?
expect_rc "persistent arm exits 0" "$rc" 0
expect_eq "persistent arm puts AetherOS first" "$(order_now)" "0001,0000"
expect_eq "persistent arm records the original order" "$(record_now)" "0000,0001"
expect_has "persistent arm marker" "$out" "result=switched mode=persistent"
if called '-n'; then bad "persistent arm wrote BootNext"; else ok "persistent arm writes no BootNext"; fi
if wrote_order; then ok "persistent arm used efibootmgr -o"; else bad "persistent arm did not use -o"; fi
expect_has "persistent arm logs the record" "$(cat "$AETHER_LOG" 2>/dev/null)" "recorded the original BootOrder: 0000,0001"

# --- 22: idempotent arm ----------------------------------------------------
setup; std_fixture
sw aether --persistent >/dev/null 2>&1
: >"$FAKE_STATE/calls"
out=$(sw aether --persistent); rc=$?
expect_rc "second persistent arm exits 0" "$rc" 0
expect_has "second persistent arm says already" "$out" "result=already"
expect_eq "second persistent arm leaves the order" "$(order_now)" "0001,0000"
if wrote_order; then bad "second persistent arm rewrote BootOrder"; else ok "second persistent arm performs no write"; fi

# --- 23: full persistent round trip, byte for byte -------------------------
setup; std_fixture
before=$(order_now)                                   # 0000,0001
sw aether --persistent >/dev/null 2>&1                # Ubuntu flips the order
rm -f "$FAKE_STATE/next"; printf '0001\n' >"$FAKE_STATE/current"   # AetherOS boots
out=$(ret 2>&1); rc=$?
expect_rc "round trip: unit exits 0" "$rc" 0
expect_eq "round trip: order restored byte for byte" "$(order_now)" "$before"
expect_has "round trip: unit reports the restore" "$out" "persistent switch undone: the internal disk is first again"
expect_has "round trip: marker is result=restored" "$out" "result=restored mode=default"
[ "$(next_now)" = "" ] && ok "round trip: no BootNext armed" || bad "round trip: BootNext was written"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "round trip: record not removed" || ok "round trip: record removed"

# --- 24: restore with no record: internal first, AetherOS last -------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
add_entry 0001 on AetherOS "$AETHER_PATH"
add_entry 0002 on Fedora 'HD(1,GPT,99999999-0000-0000-0000-000000000000)/File(\EFI\fedora\shimx64.efi)'
printf '0001,0000,0002\n' >"$FAKE_STATE/order"; printf '0001\n' >"$FAKE_STATE/current"
out=$(sw ubuntu --persistent); rc=$?
expect_rc "no-record persistent ubuntu exits 0" "$rc" 0
expect_eq "internal disk first, AetherOS dropped to the end" "$(order_now)" "0000,0002,0001"
expect_has "no-record persistent ubuntu marker" "$out" "result=switched mode=default"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "no-record path created a record" || ok "no-record path creates no record"

# --- 25: persistent, no Ubuntu entry ---------------------------------------
setup
add_entry 0001 on AetherOS "$AETHER_PATH"
printf '0001\n' >"$FAKE_STATE/order"; printf '0001\n' >"$FAKE_STATE/current"
out=$(sw ubuntu --persistent 2>&1); rc=$?
expect_rc "persistent ubuntu without a target exits 1" "$rc" 1
expect_has "persistent ubuntu never invents an entry" "$out" "refusing to invent one"
expect_eq "persistent ubuntu leaves the order alone" "$(order_now)" "0001"
if wrote_order; then bad "persistent ubuntu wrote an order with no target"; else ok "persistent ubuntu writes no order"; fi

# --- 26: persistent arm with the AetherOS entry gone -----------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
printf '0000\n' >"$FAKE_STATE/order"; printf '0000\n' >"$FAKE_STATE/current"
unset AETHER_USB_PART AETHER_USB_DISK     # nothing to create the entry from
out=$(sw aether --persistent 2>&1); rc=$?
expect_rc "persistent arm without the entry exits 1" "$rc" 1
expect_has "persistent arm without the entry explains itself" "$out" "cannot find the AetherOS USB stick"
expect_eq "persistent arm without the entry leaves the order" "$(order_now)" "0000"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "failed arm left a record" || ok "failed arm records nothing"

# --- 27: recorded order references a deleted entry -------------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
add_entry 0001 on AetherOS "$AETHER_PATH"
printf '0001,0000\n' >"$FAKE_STATE/order"; printf '0001\n' >"$FAKE_STATE/current"
mkdir -p "$AETHER_STATE_DIR"; printf '0000,0009\n' >"$AETHER_STATE_DIR/original-order"
out=$(sw ubuntu --persistent); rc=$?
expect_rc "record with a dead index still restores" "$rc" 0
expect_eq "dead index dropped from the restored order" "$(order_now)" "0000"
expect_has "dead index restore marker" "$out" "result=restored"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "record not removed after a partial restore" || ok "record removed after a partial restore"

# --- 28: a record whose indexes are all dead ------------------------------
setup; std_fixture
mkdir -p "$AETHER_STATE_DIR"; printf '0009\n' >"$AETHER_STATE_DIR/original-order"
printf '0001,0000\n' >"$FAKE_STATE/order"     # as if a persistent switch was armed
out=$(sw ubuntu --persistent); rc=$?
expect_rc "record of only dead entries still restores" "$rc" 0
expect_eq "record of only dead entries -> reachable order, AetherOS dropped" "$(order_now)" "0000"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "record kept after a fallback restore" || ok "record removed after a fallback restore"

# --- 28b: nothing reachable at all -> refuse ------------------------------
setup
add_entry 0001 on AetherOS "$AETHER_PATH"
printf '0001\n' >"$FAKE_STATE/order"; printf '0001\n' >"$FAKE_STATE/current"
mkdir -p "$AETHER_STATE_DIR"; printf '0009\n' >"$AETHER_STATE_DIR/original-order"
out=$(sw ubuntu --persistent 2>&1); rc=$?
expect_rc "record with only dead entries exits 4" "$rc" 4
expect_has "record with only dead entries is explained" "$out" "no entry that still exists"
expect_eq "record with only dead entries leaves the table" "$(order_now)" "0001"
expect_eq "record with only dead entries keeps the record" "$(record_now)" "0009"

# --- 29: BootOrder reported but not writable -------------------------------
setup; std_fixture
: >"$FAKE_STATE/fail-order"
out=$(sw aether --persistent 2>&1); rc=$?
expect_rc "unwritable BootOrder exits 4" "$rc" 4
expect_has "unwritable BootOrder is explained" "$out" "was not confirmed"
expect_eq "unwritable BootOrder leaves the order" "$(order_now)" "0000,0001"
expect_eq "unwritable BootOrder keeps the record for a retry" "$(record_now)" "0000,0001"
expect_has "unwritable BootOrder marker is failed" "$out" "result=failed"

# --- 30: silent BootOrder write failure ------------------------------------
setup; std_fixture
: >"$FAKE_STATE/ignore-order"
out=$(sw aether --persistent 2>&1); rc=$?
expect_rc "silent BootOrder failure exits 4" "$rc" 4
expect_has "silent BootOrder failure reports the read-back" "$out" "BootOrder reads back as 0000,0001"
expect_eq "silent BootOrder failure leaves the order untouched" "$(order_now)" "0000,0001"

# --- 31: duplicates in a record are collapsed ------------------------------
setup; std_fixture
mkdir -p "$AETHER_STATE_DIR"; printf '0000,0000,0000\n' >"$AETHER_STATE_DIR/original-order"
out=$(sw ubuntu --persistent); rc=$?
expect_rc "duplicate record exits 0" "$rc" 0
expect_eq "duplicate indexes collapsed" "$(order_now)" "0000"
expect_has "duplicate record marker" "$out" "result=restored"

# --- 32: AetherOS already first, no record ---------------------------------
setup; std_fixture
printf '0001,0000\n' >"$FAKE_STATE/order"     # someone already set it by hand
: >"$FAKE_STATE/calls"
out=$(sw aether --persistent); rc=$?
expect_rc "already-first arm exits 0" "$rc" 0
expect_has "already-first arm says already" "$out" "result=already mode=persistent"
expect_eq "already-first arm writes nothing to the order" "$(order_now)" "0001,0000"
if wrote_order; then bad "already-first arm rewrote BootOrder"; else ok "already-first arm performs no write"; fi
expect_eq "already-first arm derives a restorable record" "$(record_now)" "0000,0001"
out=$(sw ubuntu --persistent); rc=$?
expect_rc "derived record restores" "$rc" 0
expect_eq "derived record ends the persistent mode" "$(order_now)" "0000,0001"

# --- 33: AetherOS is the only entry ---------------------------------------
setup
add_entry 0001 on AetherOS "$AETHER_PATH"
printf '0001\n' >"$FAKE_STATE/order"; printf '0001\n' >"$FAKE_STATE/current"
out=$(sw aether --persistent 2>&1); rc=$?
expect_rc "single-entry persistent arm exits 1" "$rc" 1
expect_has "single-entry persistent arm refuses" "$out" "no restore order can be derived"
expect_eq "single-entry persistent arm leaves the order" "$(order_now)" "0001"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "single-entry arm left a record" || ok "single-entry arm records nothing"

# --- 34: status reports the mode ------------------------------------------
setup; std_fixture
out=$(sw status)
expect_has "status says default when nothing is armed" "$out" "Mode        : default"
expect_has "status marker default" "$out" "result=status mode=default"
printf '0001\n' >"$FAKE_STATE/next"
out=$(sw status)
expect_has "status says BootNext when one is pending" "$out" "Mode        : BootNext=0001"
expect_has "status marker next" "$out" "result=status mode=next"
rm -f "$FAKE_STATE/next"
sw aether --persistent >/dev/null 2>&1
out=$(sw status)
expect_has "status says persistent when BootOrder is armed" "$out" "Mode        : persistent (BootOrder=0001,0000, original recorded: 0000,0001)"
expect_has "status shows the recorded order" "$out" "Recorded    : 0000,0001"
expect_has "status marker persistent" "$out" "result=status mode=persistent"
# a pending BootNext plus a record: BootNext is what the next boot does
printf '0000\n' >"$FAKE_STATE/next"
out=$(sw status)
expect_has "status prefers BootNext over a record" "$out" "result=status mode=next"
rm -f "$FAKE_STATE/next"

# --- 34b: firmware that does not report BootOrder -------------------------
setup; std_fixture
: >"$FAKE_STATE/suppress-order"
out=$(sw status)
expect_has "status says BootOrder is not reported" "$out" "not reported by this firmware"
expect_has "status falls back to BootCurrent" "$out" "BootCurrent=0000"
out=$(sw aether --persistent 2>&1); rc=$?
expect_rc "persistent arm without BootOrder exits 1" "$rc" 1
expect_has "persistent arm without BootOrder explains" "$out" "does not report BootOrder"
expect_eq "persistent arm without BootOrder writes nothing" "$(order_now)" "0000,0001"
out=$(sw ubuntu --persistent 2>&1); rc=$?
expect_rc "persistent ubuntu without BootOrder exits 1" "$rc" 1

# --- 35: clear removes BootNext and the record ----------------------------
setup; std_fixture
sw aether --persistent >/dev/null 2>&1
printf '0000\n' >"$FAKE_STATE/next"          # also a pending BootNext
out=$(sw clear); rc=$?
expect_rc "clear with a record exits 0" "$rc" 0
expect_eq "clear restores the recorded order" "$(order_now)" "0000,0001"
[ "$(next_now)" = "" ] && ok "clear removes BootNext" || bad "clear left BootNext"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "clear left the record" || ok "clear removes the record"
expect_has "clear marker" "$out" "result=cleared"
out=$(sw status)
expect_has "clear returns the mode to default" "$out" "Mode        : default"

# --- 36: clear when the order write fails ---------------------------------
setup; std_fixture
sw aether --persistent >/dev/null 2>&1
: >"$FAKE_STATE/fail-order"
out=$(sw clear 2>&1); rc=$?
expect_rc "clear with a failing order write exits 4" "$rc" 4
expect_has "clear reports the failed restore" "$out" "could not restore the recorded BootOrder"
expect_eq "clear leaves the order untouched on failure" "$(order_now)" "0001,0000"
expect_eq "clear keeps the record on failure" "$(record_now)" "0000,0001"

# --- 37: unit with a stale record -----------------------------------------
setup; std_fixture
sw aether --persistent >/dev/null 2>&1
sw ubuntu --persistent >/dev/null 2>&1      # order restored, record dropped
mkdir -p "$AETHER_STATE_DIR"; printf '0000,0001\n' >"$AETHER_STATE_DIR/original-order"  # stale copy
printf '0001\n' >"$FAKE_STATE/current"
rm -f "$FAKE_STATE/next"
out=$(ret 2>&1); rc=$?
expect_rc "unit with a stale record exits 0" "$rc" 0
expect_eq "unit with a stale record leaves the order as the record" "$(order_now)" "0000,0001"
expect_eq "unit with a stale record returns via BootNext" "$(next_now)" "0000"
[ -e "$AETHER_STATE_DIR/original-order" ] && bad "stale record not removed" || ok "stale record removed"

# --- 38: the unit's BootNext path is unchanged without a record -----------
setup; std_fixture
printf '0001\n' >"$FAKE_STATE/current"
out=$(ret 2>&1); rc=$?
expect_rc "unit without a record exits 0" "$rc" 0
expect_eq "unit without a record writes BootNext" "$(next_now)" "0000"
expect_eq "unit without a record leaves BootOrder" "$(order_now)" "0000,0001"
expect_has "unit without a record marker mode=next" "$out" "result=switched mode=next"

# --- 39: the `mode` query the unit uses -----------------------------------
setup; std_fixture
expect_eq "mode query: default" "$(sw mode)" "AETHER_BOOT_MODE: default"
printf '0000\n' >"$FAKE_STATE/next"
expect_eq "mode query: next" "$(sw mode)" "AETHER_BOOT_MODE: next"
rm -f "$FAKE_STATE/next"
sw aether --persistent >/dev/null 2>&1
expect_eq "mode query: persistent" "$(sw mode)" "AETHER_BOOT_MODE: persistent"

# --- 40: persistent round trip without a reachable record (AetherOS /var is
#         tmpfs, so the record written from Ubuntu is not visible there) -----
setup; std_fixture
sw aether --persistent >/dev/null 2>&1
rm -f "$AETHER_STATE_DIR/original-order"          # as AetherOS sees /var/lib
[ "$(order_now)" = "0001,0000" ] && ok "tmpfs case: order is still AetherOS first" || bad "tmpfs case: arm failed"
printf '0001\n' >"$FAKE_STATE/current"
out=$(ret 2>&1); rc=$?
expect_rc "tmpfs case: unit exits 0" "$rc" 0
expect_eq "tmpfs case: order restored byte for byte from the table" "$(order_now)" "0000,0001"
expect_has "tmpfs case: unit says it rebuilt from the table" "$out" "rebuilding from the table"
[ "$(next_now)" = "" ] && ok "tmpfs case: no BootNext needed" || bad "tmpfs case: BootNext was written"

# --- 41: AetherOS first with a third OS, no record ------------------------
setup
add_entry 0000 on ubuntu "$UBUNTU_PATH"
add_entry 0001 on AetherOS "$AETHER_PATH"
add_entry 0002 on Fedora 'HD(1,GPT,99999999-0000-0000-0000-000000000000)/File(\EFI\fedora\shimx64.efi)'
printf '0001,0000,0002\n' >"$FAKE_STATE/order"; printf '0001\n' >"$FAKE_STATE/current"
out=$(ret 2>&1); rc=$?
expect_rc "third-OS case: unit exits 0" "$rc" 0
expect_eq "third-OS case: internal first, AetherOS last" "$(order_now)" "0000,0002,0001"
[ "$(next_now)" = "" ] && ok "third-OS case: no BootNext needed" || bad "third-OS case: BootNext was written"

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" = 0 ]
