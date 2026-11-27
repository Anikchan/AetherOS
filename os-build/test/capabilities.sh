#!/usr/bin/env bash
# capabilities.sh — say exactly which tools an AetherOS image (or any root
# filesystem) actually has, and assert that list.
#
# Why this exists (2026-11-27): two acceptance checks lied.  The image's busybox
# has no `pgrep` and no `script`, so `pgrep X || say "X is not running"` passed
# while proving nothing, and the `script`-based transcript capture silently
# recorded nothing.  A missing tool must never stand in for a negative result.
# The fix is to pin the tool list: it is a checked-in contract, CI diffs the
# image against it, and every script dependency is deliberate.
#
# Usage:
#   capabilities.sh                     # probe this machine, human readable
#   capabilities.sh --json              # probe this machine, JSON
#   capabilities.sh --root DIR          # probe an unpacked image root (no exec)
#   capabilities.sh --applets           # list busybox applets
#   capabilities.sh --write FILE        # write a manifest for the current probe
#   capabilities.sh --check FILE        # assert against a manifest (CI gate)
#   capabilities.sh --self-test         # fixture-based test of this script
#
# Options: --quiet, --json, --root DIR, --applets
#
# Manifest format (one entry per line, `#` comments allowed):
#   present <tool>     the image MUST have it; missing -> exit 1
#   absent  <tool>     scripts must not rely on it; if it shows up -> NOTE only
#
# Host vs image: this script is written to run in the image (in the acceptance
# harness), on the CI runner against an unpacked squashfs (`--root`), and on the
# macOS dev host for its own self-test.  Path probing with `--root` never
# executes anything from the image, so it is safe on any host.
#
# Exit codes: 0 ok, 1 assertion failed, 2 usage, 3 unreadable input.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

ROOT=""
MODE=probe
MANIFEST=""
QUIET=0
JSON=0
APPLETS=0
SELF_TEST=0

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --root) ROOT="${2:-}"; shift 2 || { usage >&2; exit 2; } ;;
        --root=*) ROOT="${1#*=}"; shift ;;
        --write) MODE=write; MANIFEST="${2:-}"; shift 2 || { usage >&2; exit 2; } ;;
        --write=*) MODE=write; MANIFEST="${1#*=}"; shift ;;
        --check) MODE=check; MANIFEST="${2:-}"; shift 2 || { usage >&2; exit 2; } ;;
        --check=*) MODE=check; MANIFEST="${1#*=}"; shift ;;
        --applets) APPLETS=1; shift ;;
        --json) JSON=1; shift ;;
        --quiet|-q) QUIET=1; shift ;;
        --self-test) SELF_TEST=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "capabilities: unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[ -n "$ROOT" ] && ROOT="${ROOT%/}"

# ---------------------------------------------------------------------------
# The probe list. Keep this curated and comment why an entry matters: it is the
# contract the acceptance harness codes against.
#
#   core shell/busybox applets the harness itself is built from
#   process      how "is it running?" is answered (pgrep is absent on purpose)
#   hardware     what the P2/P3 checks read
#   tools        optional validators that must be guarded with `command -v`
# ---------------------------------------------------------------------------
PROBES="
sh
awk
sed
grep
cut
head
tail
tr
sort
uniq
wc
tee
find
mktemp
chmod
stat
cat
cp
mv
rm
mkdir
rmdir
ln
ls
df
du
dd
od
printf
date
seq
sleep
sync
uname
hostname
id
kill
mount
umount
ps
pidof
pgrep
pkill
script
ip
ifconfig
udhcpc
wget
curl
sha256sum
tar
gzip
dmesg
systemctl
journalctl
loginctl
systemd-analyze
upower
powerprofilesctl
pipewire
wireplumber
wpctl
pw-cli
wpa_supplicant
wpa_cli
iw
bluetoothctl
libinput
lspci
lsusb
glxinfo
vulkaninfo
Xwayland
aplay
arecord
speaker-test
amixer
sshd
ssh
ssh-keygen
rauc
bash
python3
sudo
timeout
"

# Candidate directories inside a root filesystem (Buildroot layout).
ROOT_BINDIRS="bin usr/bin sbin usr/sbin usr/local/bin usr/local/sbin"

# ---------------------------------------------------------------------------
# Busybox awareness: the same name means different things depending on whether
# it is a busybox applet or a real binary, so record which one it is.
# ---------------------------------------------------------------------------
link_target() {
    local path="$1" link depth=0
    while [ -L "$path" ] && [ "$depth" -lt 5 ]; do
        link="$(readlink "$path" 2>/dev/null)" || break
        case "$link" in
            /*) path="$link" ;;
            *) path="$(dirname "$path")/$link" ;;
        esac
        depth=$((depth + 1))
    done
    printf '%s' "$path"
}

# resolve_tool <name> -> "path<TAB>via" or "" when not found.
resolve_tool() {
    local name="$1" dir candidate real
    if [ -n "$ROOT" ]; then
        for dir in $ROOT_BINDIRS; do
            candidate="$ROOT/$dir/$name"
            if [ -e "$candidate" ] || [ -L "$candidate" ]; then
                real="$(link_target "$candidate")"
                case "$(basename "$real")" in
                    busybox) printf '%s\tbusybox' "$candidate" ;;
                    *) printf '%s\tbinary' "$candidate" ;;
                esac
                return 0
            fi
        done
        return 0
    fi
    candidate="$(command -v "$name" 2>/dev/null)" || return 0
    real="$(link_target "$candidate")"
    case "$(basename "$real")" in
        busybox*) printf '%s\tbusybox' "$candidate" ;;
        *) printf '%s\tbinary' "$candidate" ;;
    esac
    return 0
}

collect() {
    local name entry path via
    for name in $PROBES; do
        entry="$(resolve_tool "$name")"
        if [ -n "$entry" ]; then
            path="${entry%%	*}"
            via="${entry##*	}"
            printf 'present\t%s\t%s\t%s\n' "$name" "$path" "$via"
        else
            printf 'absent\t%s\t-\t-\n' "$name"
        fi
    done
}

list_applets() {
    local real name
    if [ -n "$ROOT" ]; then
        # No execution: every symlink in the root that points at busybox is an
        # installed applet.  Hardlinked applets cannot be found this way.
        for dir in $ROOT_BINDIRS; do
            [ -d "$ROOT/$dir" ] || continue
            for path in "$ROOT/$dir"/*; do
                [ -L "$path" ] || continue
                real="$(link_target "$path")"
                [ "$(basename "$real")" = "busybox" ] && basename "$path"
            done
        done | sort -u
        return 0
    fi
    if command -v busybox >/dev/null 2>&1; then
        busybox --list 2>/dev/null | sort
        return 0
    fi
    # Not busybox-based: every executable on PATH is the effective applet set.
    for dir in $(printf '%s' "${PATH:-/usr/bin:/bin}" | tr ':' ' '); do
        [ -d "$dir" ] || continue
        ls -1 "$dir" 2>/dev/null
    done | sort -u
}

# ---------------------------------------------------------------------------
emit_probe() {
    local tab rows
    rows="$(collect)"
    if [ "$JSON" = "1" ]; then
        printf '{"root":"%s","tools":[' "${ROOT:-/}"
        printf '%s' "$rows" | awk -F'\t' '
            BEGIN { first = 1 }
            {
                if (!first) printf ","
                first = 0
                printf "{\"name\":\"%s\",\"present\":%s,\"path\":\"%s\",\"via\":\"%s\"}", \
                    $2, ($1 == "present" ? "true" : "false"), $3, $4
            }'
        printf ']}\n'
        return 0
    fi

    [ "$QUIET" = "1" ] || printf 'capabilities: root=%s host=%s\n' "${ROOT:-/}" "$(uname -sr)"
    printf '%s\n' "$rows" | awk -F'\t' '
        $1 == "present" { printf "present %-16s %s (%s)\n", $2, $3, $4; p++ }
        $1 == "absent"  { if (!quiet) printf "absent  %-16s\n", $2; a++ }
        END { if (!quiet) printf "-- %d present, %d absent\n", p+0, a+0 }
    ' quiet="$QUIET"
}

emit_write() {
    local rows out="${1:-$MANIFEST}"
    rows="$(collect)"
    {
        echo "# AetherOS image capabilities — generated by"
        echo "#   os-build/test/capabilities.sh --write <this file>"
        echo "#"
        echo "# present <tool>   the image must have it; a missing one fails CI."
        echo "# absent  <tool>   scripts must not rely on it (guard with 'command -v');"
        echo "#                  if it ever appears, CI notes it, it does not fail."
        echo "#"
        echo "# Regenerate on the image and review the diff: a new 'present' line is a"
        echo "# tool scripts may now assume, a new 'absent' line is a dependency they"
        echo "# must drop."
        printf '%s\n' "$rows" | awk -F'\t' '$1 == "present" { printf "present %s\n", $2 }'
        printf '%s\n' "$rows" | awk -F'\t' '$1 == "absent"  { printf "absent %s\n", $2 }'
    } > "$out"
    if [ "$QUIET" != "1" ]; then
        printf 'capabilities: wrote %s (%s present, %s absent)\n' "$out" \
            "$(printf '%s\n' "$rows" | awk -F'\t' '$1=="present"' | wc -l | tr -d ' ')" \
            "$(printf '%s\n' "$rows" | awk -F'\t' '$1=="absent"' | wc -l | tr -d ' ')"
    fi
}

emit_check() {
    local manifest="$1" rows kind tool live status missing="" unexpected="" extra="" checked=0 failed=0

    if [ ! -r "$manifest" ]; then
        echo "capabilities: cannot read manifest: $manifest" >&2
        return 3
    fi
    rows="$(collect)"

    while read -r kind tool _; do
        case "$kind" in
            ''|\#*) continue ;;
            present|absent) ;;
            *) echo "capabilities: bad manifest line '$kind $tool' in $manifest" >&2; return 2 ;;
        esac
        [ -n "$tool" ] || continue
        checked=$((checked + 1))
        live="$(printf '%s\n' "$rows" | awk -F'\t' -v t="$tool" '$2 == t { print $1; exit }')"
        [ -n "$live" ] || live=unknown
        if [ "$kind" = "present" ]; then
            if [ "$live" != "present" ]; then
                failed=$((failed + 1))
                missing="$missing
  - $tool: required by the manifest, not in the image"
            fi
        else
            if [ "$live" = "present" ]; then
                unexpected="$unexpected
  - $tool: manifest says absent, image has it (note only)"
            fi
        fi
    done < "$manifest"

    # Tools present but unlisted are informational: they tell us the contract
    # is narrower than the image, not that anything is broken.
    extra="$(printf '%s\n' "$rows" | awk -F'\t' '$1 == "present" { print $2 }' | while read -r t; do
        grep -qE "^[[:space:]]*(present|absent)[[:space:]]+$t([[:space:]]|$)" "$manifest" || printf ' %s' "$t"
    done)"

    if [ "$failed" -gt 0 ]; then
        echo "capabilities: FAIL — $failed of $checked manifest entries are not satisfied:"
        printf '%s\n' "${missing#?}"
        [ -n "$extra" ] && echo "  (image also has, not in the manifest:$extra)"
        echo "  Fix the defconfig/package that provides the tool, or remove the"
        echo "  entry from $manifest and stop depending on it."
        return 1
    fi

    if [ "$QUIET" != "1" ]; then
        echo "capabilities: OK — all $checked manifest entries satisfied (root=${ROOT:-/})."
        [ -n "$unexpected" ] && echo "  notes:${unexpected}"
        [ -n "$extra" ] && echo "  unlisted but present:$extra"
    fi
    return 0
}

# ---------------------------------------------------------------------------
# Self-test: a fake root filesystem proves present/absent/root-mode/assertions
# without an image.  Runs on macOS.
# ---------------------------------------------------------------------------
run_self_test() {
    local tmp failures=0 rc out real

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN

    mkdir -p "$tmp/root/bin" "$tmp/root/usr/bin" "$tmp/root/usr/sbin"
    printf '#!/bin/sh\n' > "$tmp/root/bin/busybox"
    chmod +x "$tmp/root/bin/busybox"
    for a in sh ls grep sed cat mount pidof; do
        ln -s busybox "$tmp/root/bin/$a"
    done
    printf '#!/bin/sh\n' > "$tmp/root/usr/bin/upower"
    chmod +x "$tmp/root/usr/bin/upower"
    ln -s ../bin/busybox "$tmp/root/usr/bin/awk"

    real="$ROOT"

    # 1. busybox applet detected as busybox, not as a binary
    ROOT="$tmp/root"
    out="$(collect)"
    printf '%s' "$out" | awk -F'\t' '$2=="grep" { found = ($1=="present" && $4=="busybox") } END { exit !found }' \
        || { echo "self-test FAIL 1: grep not recognised as a busybox applet"; failures=$((failures + 1)); }

    # 2. a real package binary is reported as 'binary' via /usr/bin
    printf '%s' "$out" | awk -F'\t' '$2=="upower" { found = ($1=="present" && $4=="binary" && $3 ~ /usr\/bin\/upower$/) } END { exit !found }' \
        || { echo "self-test FAIL 2: upower not found in /usr/bin"; failures=$((failures + 1)); }

    # 3. the tools that burned us are reported absent, not silently missing
    for tool in pgrep script; do
        printf '%s' "$out" | awk -F'\t' -v t="$tool" '$2==t { found = ($1=="absent") } END { exit !found }' \
            || { echo "self-test FAIL 3: $tool not reported absent"; failures=$((failures + 1)); }
    done

    # 4. --applets lists the busybox symlinks without executing anything
    out="$(list_applets)"
    printf '%s\n' "$out" | grep -qx awk || { echo "self-test FAIL 4: applet scan missed usr/bin/awk"; failures=$((failures + 1)); }

    # 5. a manifest requiring a missing tool fails (exit 1) and names it
    printf 'present sh\npresent pgrep\nabsent script\n' > "$tmp/m-fail"
    out="$(emit_check "$tmp/m-fail" 2>&1)"; rc=$?
    if [ "$rc" != "1" ] || ! printf '%s' "$out" | grep -q 'pgrep: required'; then
        echo "self-test FAIL 5: missing required tool did not fail the check (rc=$rc)"; echo "$out"; failures=$((failures + 1))
    fi

    # 6. a satisfied manifest passes, and an 'absent' tool that IS present only
    #    produces a note (exit 0)
    printf 'present sh\npresent grep\nabsent pidof\n' > "$tmp/m-ok"
    out="$(emit_check "$tmp/m-ok" 2>&1)"; rc=$?
    if [ "$rc" != "0" ]; then echo "self-test FAIL 6: satisfied manifest failed (rc=$rc)"; echo "$out"; failures=$((failures + 1)); fi
    printf '%s' "$out" | grep -q 'manifest says absent' || { echo "self-test FAIL 6b: absent-but-present note missing"; failures=$((failures + 1)); }

    # 7. --write then --check round-trips on the fake root
    emit_write "$tmp/m-written" >/dev/null
    out="$(emit_check "$tmp/m-written" 2>&1)"; rc=$?
    if [ "$rc" != "0" ]; then echo "self-test FAIL 7: written manifest does not check clean (rc=$rc)"; echo "$out"; failures=$((failures + 1)); fi

    # 8. a malformed manifest line is a usage error, not a pass
    printf 'maybe sh\n' > "$tmp/m-bad"
    emit_check "$tmp/m-bad" >/dev/null 2>&1; rc=$?
    [ "$rc" = "2" ] || { echo "self-test FAIL 8: malformed manifest did not exit 2 (rc=$rc)"; failures=$((failures + 1)); }

    # 9. a missing manifest fails closed
    emit_check "$tmp/does-not-exist" >/dev/null 2>&1; rc=$?
    [ "$rc" = "3" ] || { echo "self-test FAIL 9: missing manifest did not exit 3 (rc=$rc)"; failures=$((failures + 1)); }

    # 10. the checked-in contract lists pgrep/script as absent, so no script can
    #     quietly depend on them
    real="$SCRIPT_DIR/capabilities.expected"
    if [ -r "$real" ]; then
        for tool in pgrep script; do
            grep -qE "^absent[[:space:]]+$tool([[:space:]]|$)" "$real" \
                || { echo "self-test FAIL 10: $real does not declare '$tool' absent"; failures=$((failures + 1)); }
        done
    else
        echo "self-test SKIP 10: $real not found"
    fi

    ROOT="$real"
    if [ "$failures" = "0" ]; then
        echo "capabilities --self-test: OK (10 cases)"
        return 0
    fi
    echo "capabilities --self-test: FAIL ($failures case(s))"
    return 1
}

# ---------------------------------------------------------------------------
if [ "$SELF_TEST" = "1" ]; then
    run_self_test
    exit $?
fi

if [ -n "$ROOT" ] && [ ! -d "$ROOT" ]; then
    echo "capabilities: root directory not found: $ROOT" >&2
    exit 3
fi

if [ "$APPLETS" = "1" ]; then
    list_applets
    exit 0
fi

case "$MODE" in
    probe) emit_probe ;;
    write) emit_write ;;
    check) emit_check "$MANIFEST" ;;
    *) echo "capabilities: unknown mode $MODE" >&2; exit 2 ;;
esac
