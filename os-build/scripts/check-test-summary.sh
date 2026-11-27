#!/usr/bin/env bash
# check-test-summary.sh — turn a cargo test log into a CI summary that names
# what failed, so a Linux-only failure cannot hide in 2000 lines of output.
#
# Why this exists (2026-11-27): `cargo test --workspace` was green on the macOS
# dev host and red on Ubuntu, and the only visible artifact was a red job with a
# 3000-line log.  Four defects took a full evening to attribute.  This script
# makes the log self-explanatory: totals, a per-target table, every failing test
# name, and the captured output of each failure, written as GitHub-flavoured
# markdown for $GITHUB_STEP_SUMMARY.
#
# Usage:
#   check-test-summary.sh <cargo-test.log> [--annotations] [--max-output N]
#
#   --annotations   also emit ::error:: workflow commands (stderr) so the
#                   failure is annotated on the run, not only in the summary
#   --max-output N  captured lines per failing test (default 15)
#
# Exit: 0 verdict PASS, 1 failures or build errors, 2 usage, 3 unreadable log,
#       4 no verdict in the log (truncated/crashed run — fail closed).
#
# Works on the raw `cargo test` stdout+stderr stream; no JUnit machinery.

set -uo pipefail

LOG=""
ANNOTATE=0
MAX_OUT=15

usage() {
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --annotations) ANNOTATE=1; shift ;;
        --max-output) MAX_OUT="${2:-15}"; shift 2 || { usage >&2; exit 2; } ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "check-test-summary: unknown option: $1" >&2; usage >&2; exit 2 ;;
        *) LOG="$1"; shift ;;
    esac
done

if [ -z "$LOG" ]; then
    usage >&2
    exit 2
fi
if [ ! -r "$LOG" ]; then
    echo "check-test-summary: cannot read log: $LOG" >&2
    exit 3
fi

# ---------------------------------------------------------------------------
# Parse with awk: one pass, POSIX-compatible.
# ---------------------------------------------------------------------------
PARSED="$(awk -v maxout="$MAX_OUT" '
    function flush_fail() {
        if (failname != "" && !(failname in seen)) {
            seen[failname] = 1
            nf++
            failures[nf] = failname
            capture[nf] = capout
        }
        failname = ""; capout = ""; incap = 0
    }

    /^     Running / {
        # target label: the path shown in parentheses
        label = $0
        sub(/^[ ]*Running /, "", label)
        if (match(label, /\(.*\)/))
            label = substr(label, RSTART + 1, RLENGTH - 2)
        cur = label
        next
    }
    /^   Doc-tests / {
        label = $0
        sub(/^[ ]*Doc-tests /, "", label)
        cur = "doc: " label
        next
    }
    /^test result: / {
        # test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
        passed = failed = ignored = 0
        for (i = 1; i <= NF; i++) {
            if ($i == "passed;")  passed  = $(i - 1) + 0
            if ($i == "failed;")  failed  = $(i - 1) + 0
            if ($i == "ignored;") ignored = $(i - 1) + 0
        }
        nt++
        tlabel[nt] = cur
        tpass[nt] = passed; tfail[nt] = failed; tign[nt] = ignored
        tot_pass += passed; tot_fail += failed; tot_ign += ignored
        flush_fail()
        next
    }
    /^---- .* stdout ----$/ {
        flush_fail()
        failname = $0
        sub(/^---- /, "", failname)
        sub(/ stdout ----$/, "", failname)
        capout = ""; incap = 1; caplines = 0
        next
    }
    /^failures:$/ {
        flush_fail()
        inlist = 1
        next
    }
    {
        if (inlist && $0 ~ /^    [A-Za-z0-9_:<>-]+$/) {
            name = $0
            gsub(/^    /, "", name)
            if (!(name in seen)) { seen[name] = 1; nf++; failures[nf] = name; capture[nf] = "" }
            next
        }
        if (inlist && $0 !~ /^[ \t]*$/) inlist = 0
        if (incap) {
            if ($0 ~ /^---- / || $0 ~ /^failures:$/) { incap = 0 }
            else if (caplines < maxout) { capout = capout $0 "\n"; caplines++ }
        }
        if ($0 ~ /^error(\[[A-Z0-9]+\])?: / && $0 !~ /^error: test failed/) {
            nerr++
            errs[nerr] = $0
        }
    }
    END {
        printf "targets=%d\n", nt
        printf "passed=%d\n", tot_pass
        printf "failed=%d\n", tot_fail
        printf "ignored=%d\n", tot_ign
        printf "build_errors=%d\n", nerr
        for (i = 1; i <= nt; i++)
            printf "T\t%s\t%d\t%d\t%d\n", tlabel[i], tpass[i], tfail[i], tign[i]
        for (i = 1; i <= nerr; i++)
            printf "E\t%s\n", errs[i]
        for (i = 1; i <= nf; i++) {
            printf "F\t%s\n", failures[i]
            if (capture[i] != "") {
                gsub(/\n/, "\\n", capture[i])
                printf "O\t%s\n", capture[i]
            }
        }
    }
' "$LOG")"

verdict_field() { printf '%s\n' "$PARSED" | awk -F= -v k="$1" '$1 == k { print $2; exit }'; }

TARGETS="$(verdict_field targets)"
PASSED="$(verdict_field passed)"
FAILED="$(verdict_field failed)"
IGNORED="$(verdict_field ignored)"
BUILD_ERRORS="$(verdict_field build_errors)"
TARGETS="${TARGETS:-0}"

if [ "$TARGETS" = "0" ] && [ "${BUILD_ERRORS:-0}" != "0" ]; then
    VERDICT="BUILD FAILED"
elif [ "$TARGETS" = "0" ] && [ "${BUILD_ERRORS:-0}" = "0" ]; then
    echo "check-test-summary: no test results found in $LOG (run truncated or killed)" >&2
    echo "check-test-summary: FAIL (no verdict)"
    exit 4
elif [ "${FAILED:-0}" != "0" ] || [ "${BUILD_ERRORS:-0}" != "0" ]; then
    VERDICT="FAIL"
else
    VERDICT="PASS"
fi

# ---------------------------------------------------------------------------
# Markdown for $GITHUB_STEP_SUMMARY
# ---------------------------------------------------------------------------
echo "## Workspace test suite (\`cargo test --workspace --all-targets\`)"
echo
if [ "$VERDICT" = "PASS" ]; then
    echo "**Verdict: PASS** — ${PASSED} passed, ${IGNORED} ignored across ${TARGETS} targets."
else
    echo "**Verdict: ${VERDICT}** — ${PASSED} passed, **${FAILED} failed**, ${IGNORED} ignored across ${TARGETS} targets (${BUILD_ERRORS} build error lines)."
fi
echo

if [ "${BUILD_ERRORS:-0}" != "0" ]; then
    echo "### Build errors"
    echo '```'
    printf '%s\n' "$PARSED" | awk -F'\t' '$1 == "E" { print $2 }' | head -20
    echo '```'
    echo
fi

if [ "${FAILED:-0}" != "0" ]; then
    echo "### Failing tests (Linux parity)"
    printf '%s\n' "$PARSED" | awk -F'\t' '$1 == "F" { print "- `" $2 "`" }'
    echo
    echo "### Captured output"
    printf '%s\n' "$PARSED" | awk -F'\t' '
        $1 == "F" { name = $2; next }
        $1 == "O" {
            text = $2
            gsub(/\\n/, "\n", text)
            printf "**%s**\n```\n%s\n```\n", name, text
        }
    '
    echo
    echo "Reproduce on any host: \`cargo test --workspace --all-targets <name>\`."
    echo
fi

echo "<details><summary>Per-target results (${TARGETS})</summary>"
echo
echo "| target | passed | failed | ignored |"
echo "| --- | --- | --- | --- |"
printf '%s\n' "$PARSED" | awk -F'\t' '$1 == "T" {
    name = $2
    sub(/^.*\/deps\//, "", name)
    printf "| %s | %s | %s | %s |\n", name, $3, $4, $5
}'
echo
echo "</details>"

if [ "$ANNOTATE" = "1" ] && [ "${FAILED:-0}" != "0" ]; then
    # Workflow commands on stderr; GitHub reads annotations from both streams.
    printf '%s\n' "$PARSED" | awk -F'\t' '$1 == "F" { printf "::error title=Test failed::%s\n", $2 }' >&2
fi
if [ "$ANNOTATE" = "1" ] && [ "${BUILD_ERRORS:-0}" != "0" ]; then
    printf '%s\n' "$PARSED" | awk -F'\t' '$1 == "E" { printf "::error title=Build error::%s\n", $2 }' >&2
fi

case "$VERDICT" in
    PASS) exit 0 ;;
    *) exit 1 ;;
esac
