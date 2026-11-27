#!/usr/bin/env bash
# check-defconfig-drop.sh — fail when a symbol we asked for in a defconfig did
# not survive the kconfig run.
#
# Why this exists (2026-11-27): `BR2_PACKAGE_WIREPLUMBER=y` needs a Lua >= 5.3
# provider.  With no Lua symbol the package's prompt has unmet dependencies, so
# kconfig removes the symbol from `.config` *entirely* — no error, no warning,
# no `# ... is not set` line.  The image built fine and shipped PipeWire with no
# session manager.  Nothing in CI looked at the difference between "we asked for
# it" and "we got it".
#
# kconfig semantics this script encodes:
#
#   defconfig `SYM=y`             -> the intent is ON.  `.config` must contain
#                                    `SYM=y`.  A missing line is a silent drop
#                                    (unmet dependency / unknown symbol).
#   defconfig `# SYM is not set`  -> the intent is OFF.  How the symbol lands in
#                                    `.config` is not our business; never report
#                                    it.  This is why the guard is quiet on a
#                                    defconfig full of deliberate `is not set`
#                                    lines.  `SYM=n` is treated the same way.
#   defconfig `SYM="text"` / `SYM=123`
#                                 -> the intent is "symbol exists with this
#                                    value".  Missing line = DROP; a different
#                                    value = CHANGED (reported, not fatal, so a
#                                    harmless requote does not redden the job).
#   `.config` `# SYM is not set` for a `=y` intent
#                                 -> DISABLED: kconfig saw the symbol but
#                                    something overrode the value (usually a
#                                    dependency that is itself off).  Fatal,
#                                    because the image will not have it.
#
# Usage:
#   check-defconfig-drop.sh <generated/.config> [defconfig]
#   check-defconfig-drop.sh --self-test
#
# Options:
#   --defconfig PATH   defconfig to read (default: os-build/buildroot/aetheros_defconfig)
#   --quiet            only print problems and the one-line verdict
#   --json             machine-readable result on stdout
#   --self-test        prove the guard catches each failure mode (fixtures, no
#                      Buildroot needed — runs on macOS and Linux alike)
#
# Limitation: defconfig entries split across lines with a trailing backslash are
# not reconstructed; the first line is still checked, which is what we care
# about (a dropped symbol is dropped regardless of quoting).
#
# Exit codes: 0 = nothing dropped, 1 = at least one drop/disable, 2 = usage,
#             3 = input missing/unreadable (fail closed: we could not verify).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEFAULT_DEFCONFIG="$ROOT_DIR/os-build/buildroot/aetheros_defconfig"

CONFIG=""
DEFCONFIG=""
QUIET=0
JSON=0
SELF_TEST=0

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --defconfig) DEFCONFIG="${2:-}"; shift 2 || { usage >&2; exit 2; } ;;
        --defconfig=*) DEFCONFIG="${1#*=}"; shift ;;
        --quiet|-q) QUIET=1; shift ;;
        --json) JSON=1; shift ;;
        --self-test) SELF_TEST=1; shift ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "check-defconfig-drop: unknown option: $1" >&2; usage >&2; exit 2 ;;
        *)
            if [ -z "$CONFIG" ]; then CONFIG="$1"; else DEFCONFIG="$1"; fi
            shift ;;
    esac
done

# ---------------------------------------------------------------------------
# The check itself: one awk pass per file (portable awk, no gawk extensions, so
# the same code runs on the macOS host and on the Ubuntu runner).
# ---------------------------------------------------------------------------
run_check() {
    local config="$1" defconfig="$2" quiet="$3" json="$4"
    awk -v quiet="$quiet" -v json="$json" '
        function jesc(s) {
            gsub(/\\/, "\\\\", s)
            gsub(/"/, "\\\"", s)
            return s
        }
        BEGIN { checked = 0; ignored = 0 }

        function finish(line, s) {
            sub(/^[[:space:]]*#[[:space:]]*/, "", line)
            sub(/[[:space:]]+is not set.*$/, "", line)
            return line
        }

        # ---- defconfig (first file) ----
        FNR == NR {
            if ($0 ~ /^[[:space:]]*#/) {
                if ($0 ~ /is not set[[:space:]]*$/) {
                    sym = finish($0)
                    if (sym != "" && !(sym in intent)) {
                        intent[sym] = "off"; ignored++
                        offs[++no] = sym
                    }
                }
                next
            }
            if ($0 ~ /^[A-Za-z0-9_]+=/) {
                eq = index($0, "=")
                sym = substr($0, 1, eq - 1)
                val = substr($0, eq + 1)
                if (val == "n") {
                    if (!(sym in intent)) { ignored++; offs[++no] = sym }
                    intent[sym] = "off"
                } else if (val == "y" || val == "m") {
                    intent[sym] = "bool"; want[sym] = val
                } else {
                    intent[sym] = "value"; want[sym] = val
                }
                # keep the original order, last value wins on duplicates
                if (!(sym in order_index)) {
                    order[++n] = sym
                    order_index[sym] = n
                }
                next
            }
            next
        }

        # ---- generated .config (second file) ----
        {
            if ($0 ~ /^# [A-Za-z0-9_]+ is not set[[:space:]]*$/) {
                sym = $0
                sub(/^# /, "", sym)
                sub(/ is not set.*$/, "", sym)
                kind[sym] = "notset"
                next
            }
            if ($0 ~ /^[A-Za-z0-9_]+=/) {
                eq = index($0, "=")
                sym = substr($0, 1, eq - 1)
                kind[sym] = "value"
                have[sym] = substr($0, eq + 1)
            }
        }

        END {
            for (i = 1; i <= n; i++) {
                sym = order[i]
                if (intent[sym] == "off") continue
                checked++
                if (!(sym in kind)) {
                    drops[++nd] = sym " (asked for \"" want[sym] "\", symbol absent from .config)"
                } else if (kind[sym] == "notset") {
                    disabled[++ndi] = sym " (asked for \"" want[sym] "\", .config has \"# " sym " is not set\")"
                } else if (want[sym] != have[sym]) {
                    changed[++nc] = sym " (asked for " want[sym] ", .config has " have[sym] ")"
                }
            }

            # A deliberate OFF that something else turned ON is not a failure,
            # but it changes the image, so say it.
            for (i = 1; i <= no; i++) {
                sym = offs[i]
                if (kind[sym] == "value" && have[sym] == "y")
                    notes[++nn] = sym " (deliberate \"is not set\", but .config has \"y\")"
            }

            failed = (nd > 0 || ndi > 0) ? 1 : 0

            if (json == "1") {
                printf "{\"checked\":%d,\"ignored_off\":%d,\"dropped\":[", checked, ignored
                for (i = 1; i <= nd; i++) printf "%s\"%s\"", (i > 1 ? "," : ""), jesc(drops[i])
                printf "],\"disabled\":["
                for (i = 1; i <= ndi; i++) printf "%s\"%s\"", (i > 1 ? "," : ""), jesc(disabled[i])
                printf "],\"changed\":["
                for (i = 1; i <= nc; i++) printf "%s\"%s\"", (i > 1 ? "," : ""), jesc(changed[i])
                printf "],\"noted_off\":["
                for (i = 1; i <= nn; i++) printf "%s\"%s\"", (i > 1 ? "," : ""), jesc(notes[i])
                printf "]}\n"
                exit failed
            }

            if (nd > 0) {
                print "DROPPED (asked for in the defconfig, gone from .config — unmet dependency or unknown symbol):"
                for (i = 1; i <= nd; i++) print "  - " drops[i]
            }
            if (ndi > 0) {
                print "DISABLED (asked for in the defconfig, .config turned it off):"
                for (i = 1; i <= ndi; i++) print "  - " disabled[i]
            }
            if (nc > 0) {
                print "CHANGED (value differs; warning only — check it is intentional):"
                for (i = 1; i <= nc; i++) print "  - " changed[i]
            }
            if (nn > 0) {
                print "NOTE (deliberate \"is not set\" in the defconfig, but ON in .config — something selected it):"
                for (i = 1; i <= nn; i++) print "  - " notes[i]
            }

            if (failed) {
                print "check-defconfig-drop: FAIL — " (nd + ndi) " requested symbol(s) did not survive; the image would build without them."
                print "  Fix the dependency in the defconfig (os-build/buildroot/aetheros_defconfig) or drop the line."
                exit 1
            }
            if (quiet != "1")
                print "check-defconfig-drop: OK — " checked " requested symbol(s) survived, " ignored " deliberate \"is not set\" entr" (ignored == 1 ? "y" : "ies") " ignored."
            exit 0
        }
    ' "$defconfig" "$config"
}

missing_input() {
    local path="$1" what="$2"
    [ -r "$path" ] || { echo "check-defconfig-drop: cannot read $what: $path" >&2; return 1; }
    return 0
}

# ---------------------------------------------------------------------------
# Self-test: fixtures that reproduce every failure mode, including the exact
# WirePlumber case.  No Buildroot, no Linux: runs wherever bash and awk do.
# ---------------------------------------------------------------------------
run_self_test() {
    local tmp failures=0 out rc
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN

    fixture() {  # fixture <name> <content-heredoc-already-written>
        printf '%s' "$2" > "$tmp/$1"
    }

    # 1. WirePlumber: the symbol is *absent* from .config (unmet dependency).
    cat > "$tmp/d1" <<'EOF'
BR2_PACKAGE_PIPEWIRE=y
BR2_PACKAGE_WIREPLUMBER=y
BR2_PACKAGE_ALSA_LIB=y
EOF
    cat > "$tmp/c1" <<'EOF'
BR2_PACKAGE_PIPEWIRE=y
BR2_PACKAGE_ALSA_LIB=y
EOF
    out="$(run_check "$tmp/c1" "$tmp/d1" 1 0)"; rc=$?
    if [ "$rc" != "1" ] || ! printf '%s' "$out" | grep -q -- '- BR2_PACKAGE_WIREPLUMBER'; then
        echo "self-test FAIL 1: absent symbol not reported as a drop (rc=$rc)"; echo "$out"; failures=$((failures + 1))
    fi

    # 2. Deliberate "# ... is not set" in the defconfig stays quiet.
    cat > "$tmp/d2" <<'EOF'
BR2_PACKAGE_PIPEWIRE=y
# BR2_PACKAGE_SYSTEMD_HWDB is not set
EOF
    cat > "$tmp/c2" <<'EOF'
BR2_PACKAGE_PIPEWIRE=y
EOF
    out="$(run_check "$tmp/c2" "$tmp/d2" 1 0)"; rc=$?
    if [ "$rc" != "0" ]; then echo "self-test FAIL 2: deliberate off reported (rc=$rc)"; echo "$out"; failures=$((failures + 1)); fi
    out="$(run_check "$tmp/c2" "$tmp/d2" 0 1)"
    printf '%s' "$out" | grep -q '"ignored_off":1' || { echo "self-test FAIL 2b: 'is not set' not counted as intent-off"; echo "$out"; failures=$((failures + 1)); }

    # 3. Symbol present in .config but turned off: fatal, classified DISABLED.
    cat > "$tmp/d3" <<'EOF'
BR2_INIT_SYSTEMD=y
EOF
    cat > "$tmp/c3" <<'EOF'
# BR2_INIT_SYSTEMD is not set
EOF
    out="$(run_check "$tmp/c3" "$tmp/d3" 1 0)"; rc=$?
    if [ "$rc" != "1" ] || ! printf '%s' "$out" | grep -q '^DISABLED'; then
        echo "self-test FAIL 3: '# SYM is not set' in .config not treated as a disable"; echo "$out"; failures=$((failures + 1))
    fi

    # 4. Clean case: booleans and string values all survive.
    cat > "$tmp/d4" <<'EOF'
BR2_x86_64=y
BR2_ROOTFS_OVERLAY="../../board/aetheros/overlay"
EOF
    cat > "$tmp/c4" <<'EOF'
BR2_x86_64=y
BR2_ROOTFS_OVERLAY="../../board/aetheros/overlay"
EOF
    out="$(run_check "$tmp/c4" "$tmp/d4" 1 0)"; rc=$?
    if [ "$rc" != "0" ]; then echo "self-test FAIL 4: clean config reported as failure"; echo "$out"; failures=$((failures + 1)); fi
    out="$(run_check "$tmp/c4" "$tmp/d4" 0 1)"
    printf '%s' "$out" | grep -q '"checked":2' || { echo "self-test FAIL 4b: expected 2 checked"; echo "$out"; failures=$((failures + 1)); }

    # 5. Unknown/typo symbol (a "y" with no matching kconfig symbol at all).
    cat > "$tmp/d5" <<'EOF'
BR2_PACKAGE_LUA_5_4=y
BR2_PACKAGE_LUAA=y
EOF
    cat > "$tmp/c5" <<'EOF'
BR2_PACKAGE_LUA_5_4=y
EOF
    out="$(run_check "$tmp/c5" "$tmp/d5" 1 0)"; rc=$?
    if [ "$rc" != "1" ] || ! printf '%s' "$out" | grep -q -- '- BR2_PACKAGE_LUAA'; then
        echo "self-test FAIL 5: unknown/typo symbol not reported"; echo "$out"; failures=$((failures + 1))
    fi

    # 6. A changed non-bool value is a warning, not a failure.
    cat > "$tmp/d6" <<'EOF'
BR2_ROOTFS_OVERLAY="../../board/aetheros/overlay"
EOF
    cat > "$tmp/c6" <<'EOF'
BR2_ROOTFS_OVERLAY="../../board/aetheros/overlay/"
EOF
    out="$(run_check "$tmp/c6" "$tmp/d6" 1 0)"; rc=$?
    if [ "$rc" != "0" ] || ! printf '%s' "$out" | grep -q '^CHANGED'; then
        echo "self-test FAIL 6: value change should warn, not fail (rc=$rc)"; echo "$out"; failures=$((failures + 1))
    fi

    # 7. Missing .config fails closed (exit 3 from main), never a pass.
    ( missing_input "$tmp/nope" ".config" ) >/dev/null 2>&1 && { echo "self-test FAIL 7: missing .config treated as readable"; failures=$((failures + 1)); }

    # 8. The shipped defconfig parses and yields a plausible number of intents.
    if [ -r "$DEFAULT_DEFCONFIG" ]; then
        out="$(run_check "$tmp/c4" "$DEFAULT_DEFCONFIG" 0 1)"
        checked="$(printf '%s' "$out" | sed 's/.*"checked":\([0-9]*\).*/\1/')"
        [ "${checked:-0}" -gt 40 ] || { echo "self-test FAIL 8: shipped defconfig yielded only '$checked' intents"; failures=$((failures + 1)); }
    else
        echo "self-test SKIP 8: $DEFAULT_DEFCONFIG not found"
    fi

    # 9. No false positive on the whole shipped defconfig when it survives
    #    verbatim (a .config equal to the defconfig must pass).
    if [ -r "$DEFAULT_DEFCONFIG" ]; then
        out="$(run_check "$DEFAULT_DEFCONFIG" "$DEFAULT_DEFCONFIG" 1 0)"; rc=$?
        if [ "$rc" != "0" ]; then
            echo "self-test FAIL 9: defconfig against itself failed"; echo "$out"; failures=$((failures + 1))
        fi
    fi

    if [ "$failures" = "0" ]; then
        echo "check-defconfig-drop --self-test: OK (8 cases + shipped-defconfig sanity)"
        return 0
    fi
    echo "check-defconfig-drop --self-test: FAIL ($failures case(s))"
    return 1
}

# ---------------------------------------------------------------------------
if [ "$SELF_TEST" = "1" ]; then
    run_self_test
    exit $?
fi

if [ -z "$CONFIG" ]; then
    echo "check-defconfig-drop: no generated .config given" >&2
    usage >&2
    exit 2
fi
[ -n "$DEFCONFIG" ] || DEFCONFIG="$DEFAULT_DEFCONFIG"

missing_input "$CONFIG" "generated config (did 'make defconfig' run? Buildroot writes output/.config)" || exit 3
missing_input "$DEFCONFIG" "defconfig" || exit 3

run_check "$CONFIG" "$DEFCONFIG" "$QUIET" "$JSON"
exit $?
