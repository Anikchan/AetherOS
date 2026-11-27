#!/usr/bin/env bash
# ci-local.sh — run the same gates as .github/workflows/ci.yml locally.
#
# Parity matters: four bugs passed on macOS and failed on the Ubuntu runner
# (2026-11-27, docs/2026/11/27/results/linux-ci-parity-fixes.md). Running this
# before a push catches them on the developer's machine instead of two hours
# later on CI, and every gate that cannot run on this host says so out loud
# instead of passing silently.
#
# Usage:
#   os-build/scripts/ci-local.sh              # every gate
#   os-build/scripts/ci-local.sh --fast       # skip the slow/auxiliary gates
#   os-build/scripts/ci-local.sh --test-only  # just build + test + summary
#
# Exit codes: 0 all gates passed (or skipped with a printed reason),
#             1 a gate failed. Skips never count as passes.
#
# Environment:
#   LIBRARY_PATH        passed through (macOS Homebrew libs for Slint/freetype)
#   CI_LOCAL_LOG_DIR    where the cargo log and summary are written
#                       (default: target/ci-local)

set -uo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="${CI_LOCAL_LOG_DIR:-$ROOT_DIR/target/ci-local}"
mkdir -p "$LOG_DIR"
CARGO_LOG="$LOG_DIR/cargo-test.log"
SUMMARY="$LOG_DIR/test-summary.md"

FAST=0
TEST_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --fast) FAST=1 ;;
        --test-only) TEST_ONLY=1; FAST=1 ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "ci-local: unknown option: $arg" >&2; exit 2 ;;
    esac
done

FAILED_GATES=""
SKIPPED_GATES=""

gate() {  # gate <name> <command...>
    local name="$1"; shift
    echo "=== $name ==="
    if "$@"; then
        return 0
    fi
    FAILED_GATES="$FAILED_GATES
  - $name"
    echo "!!! GATE FAILED: $name" >&2
    return 1
}

skip() {  # skip <name> <reason>
    echo "=== $1: SKIPPED ==="
    echo "    reason: $2"
    SKIPPED_GATES="$SKIPPED_GATES
  - $1 ($2)"
}

# ---------------------------------------------------------------------------
# 1. Build
# ---------------------------------------------------------------------------
gate "cargo build --workspace" cargo build --workspace || true

# ---------------------------------------------------------------------------
# 2. Test every target (Linux parity) + a summary that names failures
# ---------------------------------------------------------------------------
echo "=== cargo test --workspace --all-targets ==="
# --all-targets skips doctests, which plain `cargo test` ran: keep both.
test_rc=0
cargo test --workspace --all-targets 2>&1 | tee "$CARGO_LOG" || test_rc=$?
echo "=== cargo test --workspace --doc ==="
cargo test --workspace --doc 2>&1 | tee -a "$CARGO_LOG" || test_rc=$?
bash os-build/scripts/check-test-summary.sh "$CARGO_LOG" --annotations > "$SUMMARY"
summary_rc=$?
echo
cat "$SUMMARY"
echo
echo "test log:    $CARGO_LOG"
echo "test summary: $SUMMARY"
if [ $test_rc -ne 0 ]; then
    FAILED_GATES="$FAILED_GATES
  - cargo test --workspace --all-targets (exit $test_rc; see $SUMMARY)"
fi
if [ $summary_rc -ne 0 ] && [ $test_rc -eq 0 ]; then
    FAILED_GATES="$FAILED_GATES
  - test summary (exit $summary_rc: the log has no verdict)"
fi

if [ "$TEST_ONLY" = "1" ]; then
    :
else

# ---------------------------------------------------------------------------
# 3. Budgets
# ---------------------------------------------------------------------------
gate "check-budgets (sample)" \
    python3 os-build/check-budgets.py os-build/metrics/sample.txt || true

# ---------------------------------------------------------------------------
# 4. Guard: silent defconfig drops (WirePlumber)
# ---------------------------------------------------------------------------
gate "defconfig drop guard (self-test)" \
    bash os-build/scripts/check-defconfig-drop.sh --self-test || true

# A real generated .config is only present after a Buildroot run on Linux.
REAL_CONFIG="$ROOT_DIR/os-build/buildroot/buildroot/output/.config"
if [ -r "$REAL_CONFIG" ]; then
    gate "defconfig drop guard (real .config)" \
        bash os-build/scripts/check-defconfig-drop.sh "$REAL_CONFIG" \
            os-build/buildroot/aetheros_defconfig || true
else
    skip "defconfig drop guard (real .config)" \
        "no Buildroot output/.config on this host; the self-test above covers the logic, buildroot-build.yml runs the real check"
fi

# ---------------------------------------------------------------------------
# 5. Guard: image capability contract (pgrep / script)
# ---------------------------------------------------------------------------
gate "capabilities --self-test" \
    bash os-build/test/capabilities.sh --self-test || true
gate "image capability static scan (--strict)" \
    bash os-build/scripts/check-image-capabilities.sh --strict || true

# Host-gated suites: make the skip sentence visible (cargo hides it unless the
# test fails). Same command as the ci.yml step of that name.
host_gated() {
    cargo test -p aether-sandbox --test landlock_enforce --test seccomp_enforce -- --nocapture 2>&1 | tee "$LOG_DIR/host-gated.log"
    grep -E "skipped" "$LOG_DIR/host-gated.log" || echo "no skips: every assertion in these suites ran on this kernel"
}
gate "host-gated suites announce their skips" host_gated || true

ROOTFS="$ROOT_DIR/os-build/buildroot/buildroot/output/images/rootfs.squashfs"
if [ -r "$ROOTFS" ] && command -v unsquashfs >/dev/null 2>&1; then
    UNPACK="$LOG_DIR/rootfs"
    rm -rf "$UNPACK"
    if unsquashfs -no-progress -d "$UNPACK" "$ROOTFS" > /dev/null 2>&1; then
        gate "capabilities contract (unpacked image)" \
            bash os-build/test/capabilities.sh --root "$UNPACK" --check os-build/test/capabilities.expected || true
    else
        skip "capabilities contract (unpacked image)" "unsquashfs failed on $ROOTFS"
    fi
else
    skip "capabilities contract (unpacked image)" \
        "no rootfs.squashfs (or no unsquashfs) on this host; buildroot-build.yml runs it against the built image"
fi

if [ "$FAST" = "1" ]; then
    skip "S4 harness and contract checks" "--fast"
    skip "compositor smoke test" "--fast"
else

# ---------------------------------------------------------------------------
# 6. S4 harness and contract checks (same commands as ci.yml)
# ---------------------------------------------------------------------------
s4_harness() {
    python3 lifecycled/bench/gate-report.py --self-test
    cargo run -q -p aether-taskmanager -- --energy-contract > /dev/null
    cargo run -q -p aether-taskmanager -- --shortcut | grep -q open-activity-manager
    cargo run -q -p aether-taskmanager -- --tab performance | grep -q "Performance (60 s window)"
    cargo run -q -p aether-taskmanager -- --service-actions | grep -q admin.authorize
    cargo run -q -p aether-taskmanager -- --service-action aether-powerd.service restart | grep -q SERVICE_ACTION_PLAN_OK
    cargo run -q -p aether-taskmanager -- --service-action aether-powerd.service restart | grep -q "busctl call org.freedesktop.systemd1"
    ! cargo run -q -p aether-taskmanager -- --service-action aether-lifecycled.service stop > /dev/null 2>&1
    cargo run -q -p aether-taskmanager -- --polkit-policy \
      | diff -u apps/taskmanager/polkit/org.aether.taskmanager.service-action.policy -
    cargo run -q -p aether-taskmanager -- --pack-d-contract | grep -q "aether-net-accounting v1"
    cargo run -q -p aether-taskmanager -- --pack-d-contract | grep -q "aether-lifecycled samples DRM fdinfo"
    cargo run -q -p aether-sandbox --bin aether-sandbox -- doctor | grep -q '"seccomp"'
    cargo run -q -p aether-sandbox --bin aether-sandbox -- seccomp | grep -q SECCOMP_PLAN_OK
    cargo run -q -p aether-sandbox --bin aether-sandbox -- doctor | grep -q landlock_status
    (cargo run -q -p aether-sandbox --bin aether-landlock -- --probe || true) | grep -qE "LANDLOCK_(ABI|UNAVAILABLE)"
    (cargo run -q -p aether-sandbox --bin aether-landlock -- --probe --json || true) | grep -q '"seccomp"'
    cargo run -q -p aether-update -- rauc-config | grep -q RAUC_CONFIG_OK
    bash -n os-build/scripts/make-live-usb.sh
    bash -n os-build/scripts/rauc-bundle.sh
    echo "S4 harness checks: PASS"
}
gate "S4 harness and contract checks" s4_harness || true

# ---------------------------------------------------------------------------
# 7. Compositor smoke test — needs a Linux Wayland/EGL stack
# ---------------------------------------------------------------------------
compositor_smoke() {
    cargo build -p aether-compositor
    local runtime
    runtime="$(mktemp -d)"
    XDG_RUNTIME_DIR="$runtime" ./target/debug/aether-compositor > "$LOG_DIR/comp.log" 2>&1 &
    local comp_pid=$!
    local i
    for i in $(seq 1 100); do grep -q AETHER_COMPOSITOR_SOCKET_READY "$LOG_DIR/comp.log" && break; sleep 0.2; done
    grep -q AETHER_COMPOSITOR_SOCKET_READY "$LOG_DIR/comp.log" || { echo "FAIL: compositor did not start"; cat "$LOG_DIR/comp.log"; return 1; }
    ( cd spikes && cargo build --release -p spike-client )
    WAYLAND_DISPLAY=wayland-0 timeout 30 ./spikes/target/release/spike-client 2>&1 | tee "$LOG_DIR/client.log"
    grep -q CLIENT_WINDOW_MAPPED "$LOG_DIR/client.log" || { echo "FAIL: client did not map"; return 1; }
    grep -q AETHER_COMPOSITOR_CLIENT_MAPPED "$LOG_DIR/comp.log" || { echo "FAIL: compositor did not detect mapped surface"; return 1; }
    wait "$comp_pid" 2>/dev/null || true
    echo "Compositor smoke test: PASS"
}

if [ "$(uname -s)" = "Linux" ]; then
    gate "compositor smoke test" compositor_smoke || true
else
    skip "compositor smoke test" \
        "needs Linux (XWayland, EGL, XDG_RUNTIME_DIR); this host is $(uname -s). The cargo test suite above still covers the compositor's protocol logic"
fi

fi  # --fast
fi  # --test-only

# ---------------------------------------------------------------------------
echo
if [ -n "$SKIPPED_GATES" ]; then
    echo "Skipped on this host:$SKIPPED_GATES"
fi
if [ -n "$FAILED_GATES" ]; then
    echo "FAILED gates:$FAILED_GATES"
    echo "=== ci-local: FAILED ==="
    exit 1
fi
echo "=== ci-local: all gates passed ==="
