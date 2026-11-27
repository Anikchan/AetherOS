#!/usr/bin/env bash
# support-selftest.sh — prove `aether-support` builds the first-boot diagnostic
# bundle without a booted AetherOS, so it is testable on macOS and in CI.
#
# A fake root (AETHER_SUPPORT_ROOT) supplies /proc, /etc, /dev, /sys and
# /var/log fixtures, and a stub bin dir supplies systemctl/journalctl/dmesg/
# lspci/ip so the collector's own behaviour is what gets tested.
#
# Covered:
#   1  sh -n / bash -n clean, host symlink resolves to the image binary
#   2  the bundle is created and the expected member list is inside it
#   3  a missing journalctl becomes a note, not a failure (exit 0)
#   4  psk / password / passphrase are redacted in captured files and tool output
#   5  /run/aether-wpa.conf is never read; no environment dump is bundled
#   6  absent optional logs (compositor, aether-test) become notes
#   7  /sys/class/drm/card*-*/status and modes are captured per file
#   8  the report prints the path, the size and the scp hint
#   9  --usb is a no-op with a clear message when the ESP is absent
#
# Usage: os-build/test/support-selftest.sh
# Exit: 0 all cases pass, 1 a case failed.

set -uo pipefail

SELF_DIR=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$SELF_DIR/../.." && pwd)
OVERLAY=$REPO/os-build/board/aetheros/overlay
CANON=$OVERLAY/usr/bin/aether-support
HOSTLINK=$REPO/os-build/scripts/aether-support.sh

pass=0
fail=0
ok()  { pass=$((pass + 1)); printf 'ok %2d - %s\n' "$((pass + fail))" "$1"; }
bad() { fail=$((fail + 1)); printf 'not ok %2d - %s\n' "$((pass + fail))" "$1"; [ -n "${2:-}" ] && printf '         %s\n' "$2"; }
expect_eq() { [ "$2" = "$3" ] && ok "$1" || bad "$1" "got [$2], expected [$3]"; }
expect_has() { printf '%s\n' "$2" | grep -Fq -- "$3" && ok "$1" || bad "$1" "missing [$3]"; }
expect_lacks() { printf '%s\n' "$2" | grep -Fq -- "$3" && bad "$1" "unexpected [$3]" || ok "$1"; }

T=""
cleanup() { [ -n "$T" ] && rm -rf "$T"; }
trap cleanup EXIT
T=$(mktemp -d "${TMPDIR:-/tmp}/aether-support.XXXXXX")

# --- fixtures ---------------------------------------------------------------
mkdir -p "$T/root"/{proc,etc/aether,var/log/aether,dev/dri,run,sys/class/drm/card0-HDMI-A-1}
R=$T/root
printf 'root=PARTUUID=deadbeef aether.boot=stay console=ttyS0\n' >"$R/proc/cmdline"
printf 'NAME="AetherOS"\nVERSION_ID="0.1"\n' >"$R/etc/os-release"
printf 'compositor=drm built=2026-11-28\n' >"$R/etc/aether/ui-build.txt"
printf 'AETHER_BOOT_SWITCH: next=0000 result=status\nwifi psk="supersecret" ssid=x\n' >"$R/var/log/aether-boot.log"
printf 'AETHER_COMPOSITOR_BACKEND: headless\nPassword=hunter2\n' >"$R/var/log/aether/compositor.log"
: >"$R/dev/dri/card0"; : >"$R/dev/dri/renderD128"
printf 'connected\n' >"$R/sys/class/drm/card0-HDMI-A-1/status"
printf '1920x1080\n1280x720\n' >"$R/sys/class/drm/card0-HDMI-A-1/modes"
printf 'psk="LEAKME-please"\n' >"$R/run/aether-wpa.conf"

mkbin() { # <dir> [skip...] — stubs for the tools the collector calls
    d=$1; shift; mkdir -p "$d"; skip=" $* "
    printf '#!/bin/sh\nprintf "%%s\\n" "$*" >>"${STUB_LOG:-/dev/null}"\necho "journal line"\necho "wpa psk=\\"topsecret\\""\n' >"$d/journalctl"
    printf '#!/bin/sh\necho "  aether-session.service loaded active running"\n' >"$d/systemctl"
    printf '#!/bin/sh\necho "[    0.1] AetherOS kernel"\n' >"$d/dmesg"
    printf '#!/bin/sh\necho "aether_compositor 12345 0"\n' >"$d/lsmod"
    printf '#!/bin/sh\necho "00:02.0 VGA compatible controller: Intel (rev 02)"\n' >"$d/lspci"
    printf '#!/bin/sh\necho "Bus 001 Device 002: ID 1234:5678 Keyboard"\n' >"$d/lsusb"
    printf '#!/bin/sh\necho "2: enp0s31f6: <BROADCAST,UP> mtu 1500"\necho "    inet 192.168.1.42/24 brd 192.168.1.255 scope global enp0s31f6"\n' >"$d/ip"
    printf '#!/bin/sh\ni=1; while [ $i -le 30 ]; do echo "${i}.2s aether-session$i.service"; i=$((i+1)); done\n' >"$d/systemd-analyze"
    for f in "$d"/*; do
        case "$skip" in *" $(basename "$f") "*) rm -f "$f" ;; esac
    done
    chmod +x "$d"/* 2>/dev/null
}
export STUB_LOG=$T/calls; : >"$STUB_LOG"
mkbin "$T/bin"
mkbin "$T/bin-nojournal" journalctl

bundle() { # latest bundle under the fake root
    ls -1t "$R/var/log"/aether-support-*.tar.gz 2>/dev/null | head -1
}
members() { tar -tzf "$1" 2>/dev/null | sed 's#^[^/]*/##' | grep -v '^$' | sort; }
member() { tar -xzOf "$1" --include="*/$2" 2>/dev/null; }

# --- 1: syntax and the one-implementation symlink ---------------------------
if sh -n "$CANON" 2>/dev/null; then ok "sh -n aether-support"; else bad "sh -n aether-support"; fi
if bash -n "$CANON" 2>/dev/null; then ok "bash -n aether-support"; else bad "bash -n aether-support"; fi
if [ -L "$HOSTLINK" ]; then ok "os-build/scripts/aether-support.sh is a symlink"; else bad "host helper is not a symlink"; fi
if [ "$(cd "$(dirname "$HOSTLINK")" && cd "$(dirname "$(readlink "$HOSTLINK")")" && pwd)/$(basename "$(readlink "$HOSTLINK")")" = "$CANON" ]; then
    ok "host symlink resolves to the image binary"
else
    bad "host symlink does not resolve to $CANON"
fi

# --- 2/4/6/7/8: the full collection run ------------------------------------
out=$(AETHER_SUPPORT_ROOT="$R" PATH="$T/bin:/usr/bin:/bin" sh "$CANON" 2>&1); rc=$?
expect_eq "collection exits 0" "$rc" 0
B=$(bundle)
if [ -f "$B" ]; then ok "bundle created under /var/log"; else bad "bundle missing" "$out"; fi

want='dev-dri.txt
dmesg.txt
etc-aether-ui-build.txt
etc-os-release.txt
ip-4-addr.txt
ip-route.txt
journalctl-boot.txt
lsmod.txt
lspci.txt
lsusb.txt
proc-cmdline.txt
sys-class-drm-card0-HDMI-A-1-modes.txt
sys-class-drm-card0-HDMI-A-1-status.txt
systemctl-failed.txt
systemd-analyze-blame.txt
uname.txt
unit-aether-boot-return.txt
unit-aether-session.txt
unit-aether-shell.txt
unit-getty-tty2.txt
var-log-aether-boot.log
var-log-aether-compositor.log
var-log-aether-test.log'
got=$(members "$B")
missing=$(comm -23 <(printf '%s\n' "$want") <(printf '%s\n' "$got"))
expect_eq "bundle has the expected member list" "${missing:-complete}" complete

expect_eq "proc cmdline captured" "$(member "$B" proc-cmdline.txt)" "$(cat "$R/proc/cmdline")"
expect_eq "os-release captured" "$(member "$B" etc-os-release.txt)" "$(cat "$R/etc/os-release")"
expect_eq "ui-build captured" "$(member "$B" etc-aether-ui-build.txt)" "$(cat "$R/etc/aether/ui-build.txt")"
expect_has "drivers listed" "$(member "$B" lsmod.txt)" "aether_compositor"
expect_has "unit status captured" "$(member "$B" unit-aether-session.txt)" "aether-session.service loaded active running"
expect_has "journalctl boot log captured" "$(member "$B" journalctl-boot.txt)" "journal line"
expect_has "journal is bounded to 5000 lines" "$(cat "$T/calls")" "-n 5000"
expect_has "unit journal is bounded to 200 lines" "$(cat "$T/calls")" "-n 200"
expect_has "getty@tty2 is queried" "$(cat "$T/calls")" "-u getty@tty2"
expect_eq "systemd-analyze blame is head -20" "$(member "$B" systemd-analyze-blame.txt | wc -l | tr -d ' ')" 20
expect_has "drm status captured" "$(member "$B" sys-class-drm-card0-HDMI-A-1-status.txt)" "connected"
expect_has "drm modes captured" "$(member "$B" sys-class-drm-card0-HDMI-A-1-modes.txt)" "1920x1080"
expect_has "/dev/dri listed" "$(member "$B" dev-dri.txt)" "renderD128"
expect_has "compositor log captured" "$(member "$B" var-log-aether-compositor.log)" "AETHER_COMPOSITOR_BACKEND: headless"
expect_has "absent aether-test.log is a note" "$(member "$B" var-log-aether-test.log)" "not present: /var/log/aether-test.log"

bootlog=$(member "$B" var-log-aether-boot.log)
expect_has "psk= is redacted" "$bootlog" 'psk=<redacted>'
expect_lacks "the psk secret is gone" "$bootlog" "supersecret"
expect_lacks "the Wi-Fi password is gone" "$(member "$B" var-log-aether-compositor.log)" "hunter2"
expect_lacks "journal psk is redacted" "$(member "$B" journalctl-boot.txt)" "topsecret"
expect_eq "aether-wpa.conf is never read" "$(tar -tzf "$B" | grep -c 'wpa')" 0
expect_eq "no wpa secret anywhere in the bundle" "$(tar -xzOf "$B" 2>/dev/null | grep -c 'LEAKME')" 0

expect_has "report names the bundle" "$out" "bundle $B ("
expect_has "report gives the size in bytes" "$out" "($(wc -c <"$B" | tr -d ' ') bytes)"
expect_has "report gives the scp hint" "$out" "scp root@192.168.1.42:$B ."

# --- 3: a missing journalctl is a note, not a failure ----------------------
rm -f "$R"/var/log/aether-support-*.tar.gz
out=$(AETHER_SUPPORT_ROOT="$R" PATH="$T/bin-nojournal:/usr/bin:/bin" sh "$CANON" 2>&1); rc=$?
expect_eq "run without journalctl exits 0" "$rc" 0
B=$(bundle)
expect_has "missing journalctl is a note" "$(member "$B" journalctl-boot.txt)" "not available: journalctl"
expect_has "missing journalctl is a note in the unit section" "$(member "$B" unit-aether-session.txt)" "not available: journalctl"
expect_has "systemctl still captured" "$(member "$B" unit-aether-session.txt)" "aether-session.service"

# --- 9: --usb is a no-op when the ESP is absent ----------------------------
rm -f "$R"/var/log/aether-support-*.tar.gz
out=$(AETHER_SUPPORT_ROOT="$R" AETHER_SUPPORT_ESP=/dev/aether-no-such-esp PATH="$T/bin:/usr/bin:/bin" sh "$CANON" --usb 2>&1); rc=$?
expect_eq "--usb without an ESP exits 0" "$rc" 0
expect_has "--usb without an ESP says so" "$out" "no USB copy"
expect_has "--usb without an ESP still lands the bundle" "$out" "bytes)"
[ -f "$(bundle)" ] && ok "--usb without an ESP leaves the bundle in /var/log" || bad "--usb lost the bundle"
expect_has "nothing is left mounted" "$(mount 2>/dev/null | grep -c aether)" 0

# --- the entry point overrides reject a bad argument ----------------------
out=$(sh "$CANON" --nope 2>&1); rc=$?
expect_eq "unknown argument exits 2" "$rc" 2
expect_has "unknown argument explains itself" "$out" "unknown argument"

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" = 0 ]
