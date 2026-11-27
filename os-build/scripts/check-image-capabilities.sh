#!/usr/bin/env bash
# check-image-capabilities.sh — fail when an in-image script calls a tool the
# image does not have.
#
# Why this exists (2026-11-27): the acceptance harness ran
#   pgrep pipewire || say "pipewire: not running"
# The image's busybox has no pgrep, so the check "passed" while pipewire was
# running the whole time (pid 271).  A missing tool must never be able to stand
# in for a negative result.  The tool list lives in
# os-build/test/capabilities.expected; this script enforces it statically, so
# the mistake is caught before an image is ever booted.
#
# A call is acceptable when it is guarded on the same line, e.g.
#   command -v pgrep >/dev/null 2>&1 && pgrep X
# or waived explicitly on the line (or the line above):
#   pgrep X || true   # capabilities-ok: pgrep: optional cleanup
#
# Usage:
#   check-image-capabilities.sh                  # scan the default paths
#   check-image-capabilities.sh --strict         # exit 1 on findings
#   check-image-capabilities.sh --paths DIR ...  # scan specific directories
#   check-image-capabilities.sh --also pkill     # audit one extra tool
#   check-image-capabilities.sh --self-test      # fixture test of this script
#
# Options: --manifest FILE, --strict, --paths DIR..., --also TOOL..., --quiet
#
# Default paths are the things that actually run on an AetherOS box: the rootfs
# overlay.  Host-side harness scripts (os-build/test/boot-test.sh and friends)
# legitimately use host tools, so they are opt-in via --paths.
#
# Exit codes: 0 clean (or findings in non-strict mode), 1 findings in strict
# mode, 2 usage, 3 unreadable input.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MANIFEST="$ROOT_DIR/os-build/test/capabilities.expected"
STRICT=0
QUIET=0
SELF_TEST=0
PATHS=""
ALSO=""

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --manifest) MANIFEST="${2:-}"; shift 2 || { usage >&2; exit 2; } ;;
        --manifest=*) MANIFEST="${1#*=}"; shift ;;
        --strict) STRICT=1; shift ;;
        --quiet|-q) QUIET=1; shift ;;
        --paths) shift; \
            while [ $# -gt 0 ] && [ "${1#-}" = "$1" ]; do PATHS="$PATHS $1"; shift; done ;;
        --also) shift; \
            while [ $# -gt 0 ] && [ "${1#-}" = "$1" ]; do ALSO="$ALSO $1"; shift; done ;;
        --self-test) SELF_TEST=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "check-image-capabilities: unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

# Read the 'absent' tools out of the manifest, plus anything --also names.
absent_tools() {
    local manifest="$1" extra="$2" t
    if [ -r "$manifest" ]; then
        awk '$1 == "absent" { print $2 }' "$manifest"
    fi
    for t in $extra; do echo "$t"; done
}

# scan_file <file> <tools...>  -> prints "<line>: <tool>: <text>" findings
scan_file() {
    local file="$1"; shift
    local tools="$*"
    awk -v tools="$tools" '
        BEGIN {
            n = split(tools, T, " ")
            for (i = 1; i <= n; i++) {
                # command position: start of line, after a shell separator, or
                # inside a command substitution / backtick.
                pat[i] = "(^|[;&|(){}]|\"\$\\(|`)[ \t]*" T[i] "([ \t]|$)"
            }
        }
        {
            raw = $0
            # Full-line comments and documentation never execute, but they can
            # carry a waiver for the next line, so record them as context.
            if (raw ~ /^[ \t]*#/) { prev = raw; next }

            prevline = prev
            prev = raw
            line = raw
            # A bare word on its own line is data (a probe list), not a call.
            trimmed = line
            gsub(/^[ \t]+|[ \t]+$/, "", trimmed)
            skip_bare = 0
            for (k = 1; k <= n; k++) if (trimmed == T[k]) skip_bare = 1
            # Drop trailing comments (good enough: no heredoc handling needed
            # for the overlay scripts, which are small and comment-light).
            sub(/[ \t]+#.*$/, "", line)

            waived = (prevline ~ /capabilities-ok:/) || (line ~ /capabilities-ok:/)
            if (waived || skip_bare) next

            for (i = 1; i <= n; i++) {
                if (line ~ pat[i]) {
                    # A guard on this line, or on the line above (the usual
                    # `if command -v X; then ... X ...` shape), makes it safe.
                    guard = "command -v[ \t]+" T[i] "|which[ \t]+" T[i]
                    if (line ~ guard || prevline ~ guard) next
                    printf "%d: %s: %s\n", FNR, T[i], raw
                }
            }
        }
    ' "$file"
}

run_scan() {
    local paths="$1" tools="$2" findings=0 files=0 file out list p
    for p in $paths; do
        if [ ! -e "$p" ]; then
            echo "check-image-capabilities: path not found: $p" >&2
            return 3
        fi
    done
    for p in $paths; do
        if [ -f "$p" ]; then
            list="$p"
        else
            # *.sh plus anything whose first line is a /bin/sh shebang.
            list=""
            for file in $(find "$p" -type f 2>/dev/null); do
                case "$file" in
                    *.sh) list="$list $file" ;;
                    *) head -1 "$file" 2>/dev/null | grep -q '^#!.*/sh' && list="$list $file" ;;
                esac
            done
        fi
        for file in $list; do
            [ -f "$file" ] || continue
            files=$((files + 1))
            out="$(scan_file "$file" $tools)"
            if [ -n "$out" ]; then
                findings=$((findings + 1))
                while IFS= read -r l; do printf '%s:%s\n' "$file" "$l"; done <<EOF
$out
EOF
            fi
        done
    done

    if [ "$findings" = "0" ]; then
        [ "$QUIET" = "1" ] || echo "check-image-capabilities: OK — no unguarded use of image-absent tools in $files file(s) [$tools]"
        return 0
    fi
    echo "check-image-capabilities: $findings file(s) use tools the image does not provide."
    echo "  Fix: use a tool the image has (pidof instead of pgrep, nothing instead of"
    echo "  script), or guard the call with 'command -v <tool>' and handle absence"
    echo "  explicitly, or waive it with '# capabilities-ok: <tool>: <reason>'."
    if [ "$STRICT" = "1" ]; then
        echo "check-image-capabilities: FAIL (--strict)"
        return 1
    fi
    echo "check-image-capabilities: WARN (not --strict)"
    return 0
}

# ---------------------------------------------------------------------------
run_self_test() {
    local tmp failures=0 rc out
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN
    mkdir -p "$tmp/overlay"

    cat > "$tmp/overlay/unguarded.sh" <<'EOF'
#!/bin/sh
pgrep pipewire >/dev/null || echo "pipewire: not running"
EOF

    cat > "$tmp/overlay/guarded.sh" <<'EOF'
#!/bin/sh
if command -v pgrep >/dev/null 2>&1; then
    pgrep pipewire && echo running
fi
EOF

    cat > "$tmp/overlay/waived.sh" <<'EOF'
#!/bin/sh
# capabilities-ok: script: this one talks to a real terminal only
script -q -c "true" /dev/null
EOF

    cat > "$tmp/overlay/comments.sh" <<'EOF'
#!/bin/sh
# the image has no pgrep, so do not use it here
echo "pgrep is not used below"
EOF

    cat > "$tmp/overlay/documented-only.sh" <<'EOF'
#!/bin/sh
say "pgrep is unavailable"
pidof pipewire >/dev/null
EOF

    cat > "$tmp/manifest" <<'EOF'
present pidof
absent pgrep
absent script
EOF

    # 1. unguarded call is found
    out="$(scan_file "$tmp/overlay/unguarded.sh" pgrep script)"
    printf '%s' "$out" | grep -q '^2: pgrep:' || { echo "self-test FAIL 1: unguarded pgrep missed"; failures=$((failures + 1)); }

    # 2. a same-line 'command -v' guard silences it
    out="$(scan_file "$tmp/overlay/guarded.sh" pgrep)"
    [ -z "$out" ] || { echo "self-test FAIL 2: guarded call still reported:"; echo "$out"; failures=$((failures + 1)); }

    # 3. an explicit waiver silences it
    out="$(scan_file "$tmp/overlay/waived.sh" script)"
    [ -z "$out" ] || { echo "self-test FAIL 3: waived call still reported:"; echo "$out"; failures=$((failures + 1)); }

    # 4. comments and printed prose are not commands
    out="$(scan_file "$tmp/overlay/comments.sh" pgrep)"
    [ -z "$out" ] || { echo "self-test FAIL 4: comment flagged:"; echo "$out"; failures=$((failures + 1)); }

    # 5. mentioning the name inside a quoted message is not a call either
    out="$(scan_file "$tmp/overlay/documented-only.sh" pgrep)"
    [ -z "$out" ] || { echo "self-test FAIL 5: quoted mention flagged:"; echo "$out"; failures=$((failures + 1)); }

    # 6. strict mode exits 1 when the fixture overlay is scanned
    ( STRICT=1; QUIET=1; run_scan "$tmp/overlay" "pgrep script" ) >/dev/null; rc=$?
    [ "$rc" = "1" ] || { echo "self-test FAIL 6: strict scan did not exit 1 (rc=$rc)"; failures=$((failures + 1)); }

    # 7. non-strict exits 0 but still reports
    ( STRICT=0; QUIET=1; run_scan "$tmp/overlay" "pgrep script" ) >/dev/null; rc=$?
    [ "$rc" = "0" ] || { echo "self-test FAIL 7: non-strict scan exited $rc"; failures=$((failures + 1)); }

    # 8. a clean tree stays clean
    mkdir -p "$tmp/clean"
    printf '#!/bin/sh\npidof pipewire >/dev/null || echo "pipewire: not running"\n' > "$tmp/clean/ok.sh"
    ( STRICT=1; QUIET=1; run_scan "$tmp/clean" "pgrep script" ) >/dev/null; rc=$?
    [ "$rc" = "0" ] || { echo "self-test FAIL 8: clean tree reported findings (rc=$rc)"; failures=$((failures + 1)); }

    # 9. missing path is an input error, never a pass
    ( QUIET=1; run_scan "$tmp/nope" "pgrep" ) >/dev/null 2>&1; rc=$?
    [ "$rc" = "3" ] || { echo "self-test FAIL 9: missing path did not exit 3 (rc=$rc)"; failures=$((failures + 1)); }

    # 10. the '--also' escape hatch surfaces a tool the manifest does not list
    cat > "$tmp/overlay/pkill.sh" <<'EOF'
#!/bin/sh
pkill -f wpa_supplicant
EOF
    out="$(scan_file "$tmp/overlay/pkill.sh" pkill)"
    printf '%s' "$out" | grep -q 'pkill' || { echo "self-test FAIL 10: --also tool not detected"; failures=$((failures + 1)); }

    if [ "$failures" = "0" ]; then
        echo "check-image-capabilities --self-test: OK (10 cases)"
        return 0
    fi
    echo "check-image-capabilities --self-test: FAIL ($failures case(s))"
    return 1
}

# ---------------------------------------------------------------------------
if [ "$SELF_TEST" = "1" ]; then
    run_self_test
    exit $?
fi

TOOLS="$(absent_tools "$MANIFEST" "$ALSO" | sort -u | tr '\n' ' ' | sed 's/[[:space:]]*$//')"
if [ -z "$TOOLS" ]; then
    echo "check-image-capabilities: no 'absent' tools in $MANIFEST and no --also given" >&2
    exit 3
fi

[ -n "$PATHS" ] || PATHS="$ROOT_DIR/os-build/board/aetheros/overlay"

run_scan "$PATHS" "$TOOLS"
exit $?
