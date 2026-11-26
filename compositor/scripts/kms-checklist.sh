#!/usr/bin/env bash
# P3-T09 / P3-T05 hardware checklist — run the compositor on the real DRM/KMS
# backend and grade the markers it prints.
#
# Nothing here runs on the macOS dev host or in CI: it needs a DRM device, a
# libseat session (seatd or logind) and a connected display, i.e. the Dell.
# `cargo test -p aether-compositor` and `scripts/check-linux-target.sh` stay the
# headless gates; this script is the on-device counterpart that produces the D6
# evidence for:
#
#   1. EGL/GLES context on the GPU driver,
#   2. the first modeset on the primary connector,
#   3. real page flips (VBlanks),
#   4. libinput input over the seat,
#   5. the effect tier (full/reduced/minimal) and its glass program set,
#   6. `failures=0` in the glass counters,
#   7. damage-scoped frost (`frosted_px` far below `surface_px`),
#   8. damage-scoped backdrop capture (`backdrop_px` below `backdrop_union_px`).
#
# Usage:
#   ./compositor/scripts/kms-checklist.sh                    # build + 15 s at Full, 6 s at Minimal
#   ./compositor/scripts/kms-checklist.sh --duration 60
#   ./compositor/scripts/kms-checklist.sh --tier reduced --single-tier
#   ./compositor/scripts/kms-checklist.sh --client 'foot'    # map a window during the run
#   ./compositor/scripts/kms-checklist.sh --binary ./target/release/aether-compositor --no-build
#
# While it runs, move the pointer and press a key: the input checks look for the
# `INPUT_KEY` / `INPUT_BUTTON` markers. Without a keypress they are reported as
# WARN, never as PASS.
#
# Exit code: 0 when every check passed (warnings allowed), 1 when one failed.

set -uo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

pass_count=0
fail_count=0
warn_count=0

say() { printf '%s\n' "$*"; }
pass() { pass_count=$((pass_count + 1)); say "PASS  $*"; }
fail() { fail_count=$((fail_count + 1)); say "FAIL  $*"; }
warn() { warn_count=$((warn_count + 1)); say "WARN  $*"; }

duration=15
short_duration=6
tier="full"
out_dir=""
binary="$repo_root/target/debug/aether-compositor"
client_cmd=""
do_build=1
single_tier=0
grade_log=""
selftest=0

while [ $# -gt 0 ]; do
  case "$1" in
    --duration) duration="${2:?--duration needs seconds}"; shift 2 ;;
    --short-duration) short_duration="${2:?--short-duration needs seconds}"; shift 2 ;;
    --tier) tier="${2:?--tier needs full|reduced|minimal}"; shift 2 ;;
    --out) out_dir="${2:?--out needs a directory}"; shift 2 ;;
    --binary) binary="${2:?--binary needs a path}"; shift 2 ;;
    --client) client_cmd="${2:?--client needs a command}"; shift 2 ;;
    --grade) grade_log="${2:?--grade needs a captured log}"; shift 2 ;;
    --selftest) selftest=1; shift ;;
    --no-build) do_build=0; shift ;;
    --single-tier) single_tier=1; shift ;;
    -h|--help) awk 'NR>1 && /^set -uo pipefail/{exit} NR>1' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

case "$tier" in
  full|reduced|minimal) ;;
  *) echo "--tier must be full, reduced or minimal (got $tier)" >&2; exit 2 ;;
esac

# ---------------------------------------------------------------------------
# --selftest: prove the grader on this host, with no DRM device, by scoring the
# captured Dell-style fixtures. A future session can re-run this to check that a
# change to the markers or to the grader did not silently break the checklist.
# ---------------------------------------------------------------------------
if [ "$selftest" -eq 1 ]; then
  status=0
  "$0" --grade "$script_dir/fixtures/kms-full-tier.log" --tier full >/dev/null 2>&1 \
    && say "selftest OK   : full-tier fixture scored PASS" \
    || { say "selftest FAIL : full-tier fixture should score PASS"; status=1; }
  "$0" --grade "$script_dir/fixtures/kms-minimal-tier.log" --tier minimal >/dev/null 2>&1 \
    && say "selftest OK   : minimal-tier fixture scored PASS" \
    || { say "selftest FAIL : minimal-tier fixture should score PASS"; status=1; }
  broken="$(mktemp)"
  sed -e 's/failures=0/failures=2/' -e 's/AETHER_COMPOSITOR_DRM_VBLANK:/DRM_VBLANK_MISSING:/g' \
    "$script_dir/fixtures/kms-full-tier.log" >"$broken"
  if "$0" --grade "$broken" --tier full >/dev/null 2>&1; then
    say "selftest FAIL : a log with shader failures and no page flip should score FAIL"
    status=1
  else
    say "selftest OK   : broken fixture scored FAIL (failures=2, no VBlank)"
  fi
  rm -f "$broken"
  # A whole-union backdrop capture (the pre-P3-T06 behaviour) must not pass.
  unscoped="$(mktemp)"
  sed -e 's/[ ]backdrop_px=54464 backdrop_union_px=746496/ backdrop_px=746496 backdrop_union_px=746496/' \
    "$script_dir/fixtures/kms-full-tier.log" >"$unscoped"
  if "$0" --grade "$unscoped" --tier full >/dev/null 2>&1; then
    say "selftest FAIL : a whole-union backdrop capture should score FAIL"
    status=1
  else
    say "selftest OK   : whole-union capture fixture scored FAIL (backdrop_px == backdrop_union_px)"
  fi
  rm -f "$unscoped"
  [ "$status" -eq 0 ] && say "SELFTEST: PASSED" || say "SELFTEST: FAILED"
  exit "$status"
fi

stamp="$(date +%Y%m%d-%H%M%S)"
if [ -z "$out_dir" ]; then
  out_dir="$repo_root/kms-checklist-$stamp"
fi
# `--grade` scores a captured log, so it needs no output directory.
if [ -z "$grade_log" ]; then
  mkdir -p "$out_dir"
  out_dir="$(cd "$out_dir" && pwd)"
fi

say "AetherOS compositor — DRM/KMS hardware checklist"
say "  date     : $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say "  host     : $(uname -srm)"
say "  log dir  : $out_dir"
say "  workload : ${duration}s at tier=$tier$([ "$single_tier" -eq 0 ] && echo ", ${short_duration}s at tier=minimal")"
say ""

# ---------------------------------------------------------------------------
# Preconditions: this is a device run, not a CI run. `--grade` only re-scores a
# log captured elsewhere, so it skips every device precondition.
# ---------------------------------------------------------------------------
if [ -z "$grade_log" ]; then
case "$(uname -s)" in
  Linux) ;;
  *) fail "not Linux ($(uname -s)): the DRM backend only runs on the device"; exit 1 ;;
esac

if ls /dev/dri/card* >/dev/null 2>&1; then
  pass "DRM device nodes present ($(ls /dev/dri/card* | tr '\n' ' '))"
else
  fail "no /dev/dri/card* — is the i915 driver loaded (i915 as a module, P2)?"
fi

if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
  fail "XDG_RUNTIME_DIR is not set (log in on the seat, or export /run/user/\$(id -u))"
  exit 1
fi
if [ -S "$XDG_RUNTIME_DIR/wayland-0" ]; then
  warn "$XDG_RUNTIME_DIR/wayland-0 already exists: another compositor may hold the seat"
fi

if [ "$do_build" -eq 1 ]; then
  say "building: cargo build -p aether-compositor --features drm"
  if ! (cd "$repo_root" && cargo build -p aether-compositor --features drm) >"$out_dir/build.log" 2>&1; then
    fail "build failed — see $out_dir/build.log"
    exit 1
  fi
  pass "built the DRM backend (build.log)"
fi

if [ ! -x "$binary" ]; then
  fail "no compositor binary at $binary (build it, or pass --binary)"
  exit 1
fi
say ""
fi

# ---------------------------------------------------------------------------
# One bounded run. `AETHER_COMPOSITOR_RUN_SECS` makes the compositor stop on its
# own so it prints its exit markers instead of being killed.
# ---------------------------------------------------------------------------
run_pass() {
  local pass_tier="$1" seconds="$2" name="$3"
  local log="$out_dir/$name.log"
  local err="$out_dir/$name.err"

  say "--- run '$name': tier=$pass_tier, ${seconds}s -------------------------------"
  AETHER_COMPOSITOR_BACKEND=drm \
  AETHER_COMPOSITOR_RUN_SECS="$seconds" \
  AETHER_COMPOSITOR_EFFECT_TIER="$pass_tier" \
    "$binary" >"$log" 2>"$err" &
  local pid=$!

  if [ -n "$client_cmd" ]; then
    # Give the compositor a moment to bind wayland-0 before the client starts.
    for _ in $(seq 1 100); do
      [ -S "$XDG_RUNTIME_DIR/wayland-0" ] && break
      sleep 0.1
    done
    WAYLAND_DISPLAY=wayland-0 sh -c "$client_cmd" >"$out_dir/$name.client.log" 2>&1 &
    say "client started: $client_cmd (log: $name.client.log)"
  fi

  # The compositor stops itself at the deadline; allow a generous margin.
  local waited=0
  while kill -0 "$pid" 2>/dev/null; do
    sleep 0.5
    waited=$((waited + 1))
    if [ "$waited" -gt $((seconds * 4 + 40)) ]; then
      warn "run '$name' did not exit after the deadline; killing it (stats may be partial)"
      kill "$pid" 2>/dev/null
      break
    fi
  done
  wait "$pid" 2>/dev/null
  local status=$?

  if [ "$status" -ne 0 ]; then
    fail "run '$name' exited with status $status — see $err"
    head -n 20 "$err" | sed 's/^/      /'
  fi
  say "  log: $log"
  say ""
}

# ---------------------------------------------------------------------------
# Marker grading.
# ---------------------------------------------------------------------------
contains() { grep -qF -- "$2" "$1"; }
count_of() { grep -cF -- "$2" "$1" || true; }

grade_common() {
  local log="$1" label="$2"

  contains "$log" "AETHER_COMPOSITOR_BACKEND: drm" \
    && pass "$label: backend selected (AETHER_COMPOSITOR_BACKEND=drm)" \
    || fail "$label: backend marker missing (AETHER_COMPOSITOR_BACKEND: drm)"

  contains "$log" "AETHER_COMPOSITOR_DRM_SESSION: seat=" \
    && pass "$label: libseat session opened ($(grep -m1 'DRM_SESSION:' "$log" | sed 's/.*seat=/seat=/'))" \
    || fail "$label: no libseat session (DRM_SESSION marker)"

  contains "$log" "AETHER_COMPOSITOR_DRM_PRIMARY_GPU: path=" \
    && pass "$label: primary GPU $(grep -m1 'DRM_PRIMARY_GPU:' "$log" | sed 's/.*path=//')" \
    || fail "$label: no primary GPU chosen (DRM_PRIMARY_GPU marker)"

  contains "$log" "AETHER_COMPOSITOR_DRM_DEVICE: path=" \
    && pass "$label: DRM device opened ($(grep -m1 'DRM_DEVICE:' "$log" | sed 's/.*path=//' | cut -d' ' -f1))" \
    || fail "$label: DRM device was not opened (DRM_DEVICE marker)"

  # 1. EGL/GLES context on the real driver.
  if contains "$log" "AETHER_COMPOSITOR_DRM_RENDERER: backend=gles"; then
    pass "$label: EGL/GLES context ($(grep -m1 'DRM_RENDERER:' "$log" | sed 's/.*backend=//'))"
  else
    fail "$label: no EGL/GLES context (DRM_RENDERER: backend=gles missing)"
  fi

  # 2. Modeset on the connected connector.
  if contains "$log" "AETHER_COMPOSITOR_DRM_CONNECTOR: name=" && \
     contains "$log" "AETHER_COMPOSITOR_DRM_OUTPUT: connector="; then
    pass "$label: modeset ($(grep -m1 'DRM_OUTPUT:' "$log" | sed 's/.*connector=//'))"
  else
    fail "$label: no modeset (DRM_CONNECTOR / DRM_OUTPUT marker missing)"
  fi

  # 3. Real page flips.
  local vblanks
  vblanks="$(count_of "$log" "AETHER_COMPOSITOR_DRM_VBLANK:")"
  if [ "$vblanks" -gt 0 ]; then
    pass "$label: page flips confirmed by $vblanks DRM_VBLANK marker(s)"
  else
    fail "$label: no AETHER_COMPOSITOR_DRM_VBLANK — frames were queued but never presented"
  fi
  local queued
  queued="$(grep -m1 'AETHER_COMPOSITOR_DRM_STATS:' "$log" | sed 's/.*frames_queued=\([0-9]*\).*/\1/' || true)"
  if [ -n "${queued:-}" ]; then
    pass "$label: DRM_STATS frames_queued=$queued ($(grep -m1 'AETHER_COMPOSITOR_DRM_STATS:' "$log" | sed 's/^AETHER_COMPOSITOR_DRM_STATS: //'))"
  else
    warn "$label: no DRM_STATS exit marker (run shorter than the deadline?)"
  fi

  # 4. Input.
  contains "$log" "AETHER_COMPOSITOR_INPUT: backend=libinput" \
    && pass "$label: libinput attached to the seat" \
    || fail "$label: no libinput input source (INPUT marker)"
  contains "$log" "AETHER_COMPOSITOR_SEAT: name=" \
    && pass "$label: seat advertised ($(grep -m1 'AETHER_COMPOSITOR_SEAT:' "$log" | sed 's/^AETHER_COMPOSITOR_SEAT: //'))" \
    || fail "$label: no seat advertised (SEAT marker)"
  if contains "$log" "AETHER_COMPOSITOR_INPUT_KEY:" || contains "$log" "AETHER_COMPOSITOR_INPUT_BUTTON:"; then
    pass "$label: input events reached the seat (key/button markers)"
  else
    warn "$label: no key/button seen — move the pointer and press a key during the run to cover this"
  fi

  # 5. Glass programs.
  if contains "$log" "AETHER_COMPOSITOR_GLASS_SHADER: programs=3"; then
    pass "$label: glass programs compiled (3: kawase-down, kawase-up, glass-composite)"
  elif contains "$log" "glass shaders unavailable"; then
    fail "$label: glass shaders failed to compile ($(grep -m1 'glass shaders unavailable' "$log"))"
  else
    fail "$label: no GLASS_SHADER programs marker"
  fi

  local expected_blur
  local expected_tier="${3:-$tier}"
  case "$expected_tier" in
    minimal) expected_blur="blur=none" ;;
    *) expected_blur="blur=dual-kawase" ;;
  esac
  if contains "$log" "AETHER_COMPOSITOR_EFFECT_TIER: $expected_tier"; then
    pass "$label: effect tier $expected_tier selected"
  else
    fail "$label: EFFECT_TIER: $expected_tier missing"
  fi
  if contains "$log" "AETHER_COMPOSITOR_GLASS_SHADER: tier=$expected_tier $expected_blur"; then
    pass "$label: per-frame glass passes use $expected_blur"
  elif contains "$log" "AETHER_COMPOSITOR_GLASS_SHADER: tier="; then
    fail "$label: per-frame glass tier/blur do not match $expected_tier / $expected_blur"
  else
    # No client mapped = no glass surface = no per-frame line: only a warning.
    warn "$label: no per-frame GLASS_SHADER line (no client mapped? pass --client)"
  fi

  # 6. Shader failures must stay at zero.
  local stats
  stats="$(grep -m1 'AETHER_COMPOSITOR_GLASS_STATS:' "$log" || true)"
  if [ -z "$stats" ]; then
    warn "$label: no GLASS_STATS exit marker (run shorter than the deadline?)"
  else
    local failures
    failures="$(printf '%s' "$stats" | sed 's/.*failures=\([0-9]*\).*/\1/')"
    if [ "${failures:-1}" -eq 0 ]; then
      pass "$label: glass failures=0 (${stats#AETHER_COMPOSITOR_GLASS_STATS: })"
    else
      fail "$label: glass failures=$failures — ${stats#AETHER_COMPOSITOR_GLASS_STATS: }"
    fi
    # 7. Damage-scoped frost: the chain rewrote fewer pixels than full surfaces.
    local frosted surface
    frosted="$(printf '%s' "$stats" | sed 's/.*frosted_px=\([0-9]*\).*/\1/')"
    surface="$(printf '%s' "$stats" | sed 's/.*surface_px=\([0-9]*\).*/\1/')"
    if [ -n "${frosted:-}" ] && [ -n "${surface:-}" ] && [ "${surface:-0}" -gt 0 ]; then
      if [ "$frosted" -lt "$surface" ]; then
        pass "$label: damage-scoped frost (frosted_px=$frosted < surface_px=$surface)"
      else
        fail "$label: frost covers whole surfaces (frosted_px=$frosted >= surface_px=$surface)"
      fi
    else
      warn "$label: no frost/surface pixel counters (no client mapped?)"
    fi
  fi

  if contains "$log" "AETHER_COMPOSITOR_FRAME_STATS:"; then
    pass "$label: frame stats ($(grep -m1 'AETHER_COMPOSITOR_FRAME_STATS:' "$log" | sed 's/^AETHER_COMPOSITOR_FRAME_STATS: //'))"
  else
    warn "$label: no FRAME_STATS marker"
  fi

  # 8. Damage-scoped backdrop capture: the wallpaper pass only rasterises the
  #    strips the frost chain samples, not the whole union of glass surfaces.
  local shader_frame bpx bunion
  shader_frame="$(grep -m1 'AETHER_COMPOSITOR_GLASS_SHADER: tier=' "$log" || true)"
  if [ -z "$shader_frame" ]; then
    warn "$label: no per-frame GLASS_SHADER line, so the backdrop capture is unscored"
  else
    bpx="$(printf '%s' "$shader_frame" | sed -n 's/.*[ ]backdrop_px=\([0-9]*\).*/\1/p')"
    bunion="$(printf '%s' "$shader_frame" | sed -n 's/.*[ ]backdrop_union_px=\([0-9]*\).*/\1/p')"
    if [ -z "$bpx" ] || [ -z "$bunion" ]; then
      warn "$label: no backdrop_px/backdrop_union_px counters (compositor built before the damage-scoped capture?)"
    elif [ "$expected_blur" = "blur=none" ]; then
      if [ "${bpx:-1}" -eq 0 ]; then
        pass "$label: no backdrop capture at $expected_tier (fill mode, backdrop_px=0)"
      else
        fail "$label: $expected_tier captured $bpx backdrop px although it samples none"
      fi
    elif [ "${bunion:-0}" -gt 0 ] && [ "$bpx" -lt "$bunion" ]; then
      pass "$label: damage-scoped backdrop capture (backdrop_px=$bpx < backdrop_union_px=$bunion)"
    else
      fail "$label: backdrop capture covers the whole union (backdrop_px=$bpx >= backdrop_union_px=$bunion)"
    fi
  fi

  contains "$log" "AETHER_COMPOSITOR_DONE" \
    && pass "$label: clean shutdown (AETHER_COMPOSITOR_DONE)" \
    || warn "$label: no AETHER_COMPOSITOR_DONE marker (killed before the deadline?)"
}

# ---------------------------------------------------------------------------
# Primary run (and, unless --single-tier, the Minimal tier pass).
# ---------------------------------------------------------------------------
if [ -n "$grade_log" ]; then
  if [ ! -f "$grade_log" ]; then
    fail "no log at $grade_log"
    exit 1
  fi
  say "--- grading a captured log: $grade_log (tier=$tier) ----------------"
  grade_common "$grade_log" "graded"
  say ""
  say "-----------------------------------------------------------------------"
  say "RESULT: $pass_count passed, $fail_count failed, $warn_count warnings"
  [ "$fail_count" -gt 0 ] && { say "CHECKLIST: FAILED"; exit 1; }
  say "CHECKLIST: PASSED"
  exit 0
fi

run_pass "$tier" "$duration" "primary-$tier"
grade_common "$out_dir/primary-$tier.log" "$tier"

if [ "$single_tier" -eq 0 ] && [ "$tier" != "minimal" ]; then
  run_pass "minimal" "$short_duration" "minimal"
  grade_common "$out_dir/minimal.log" "minimal" "minimal"
  # Cross-run tier evidence: both pipelines were really drawn.
  if grep -qF "blur=dual-kawase" "$out_dir/primary-$tier.log" && \
     grep -qF "blur=none" "$out_dir/minimal.log"; then
    pass "tier switch: dual-kawase at $tier and no blur at minimal across runs"
  else
    warn "tier switch: pipeline markers were not both present (no client mapped?)"
  fi
  if grep -qF "AETHER_COMPOSITOR_EFFECT_TIER_SWITCH:" "$out_dir/primary-$tier.log"; then
    say  "      runtime tier switch observed: $(grep -m1 'EFFECT_TIER_SWITCH:' "$out_dir/primary-$tier.log")"
  fi
fi

# ---------------------------------------------------------------------------
# Summary.
# ---------------------------------------------------------------------------
say ""
say "-----------------------------------------------------------------------"
say "RESULT: $pass_count passed, $fail_count failed, $warn_count warnings"
say "logs  : $out_dir"
say "Attach $out_dir/*.log to docs/2026/11/27/results/ (or the day's results dir)"
say "as the D6 evidence for the run."
if [ "$fail_count" -gt 0 ]; then
  say "CHECKLIST: FAILED"
  exit 1
fi
say "CHECKLIST: PASSED"
exit 0
