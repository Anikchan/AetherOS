# 4-Session Development Kickoff — 2026-11-26

- Hardware testing on the Dell is **paused** (network/subnet mismatch, and we want to resume development).
- Source of truth: `docs/2026/11/22/parallel-plan-v2.md` (dependency-sliced, 4 sessions).
- Tracker (`03-roadmap-plan.md` §4) is updated by **S1 only**.
- Each session commits only its own territory; do not touch another session's files.

## Session names (identifiable)
| Session | Name |
|---|---|
| S1 | `session - 1 (Nov26)` |
| S2 | `session - 2 (Nov26)` |
| S3 | `session - 3 (Nov26)` |
| S4 | `session - 4 (Nov26)` |

## Current baseline (verified)
- P0 ✅, P1 ✅ (M1 achieved).
- P2 in progress: T01/T02/T07/T08 done; bare-metal boot validated (graphics/backlight/battery/temp/input/BT adapter). Wi-Fi/webcam/touchpad/audio fixes are in the built image (`d0e57c8`) but hardware re-validation is paused.
- P3-T01 done (compositor skeleton). P4 code+tests done (S3). P5/P6/P7/P8 core code+tests done (S4).

## Session territories (unchanged)
| Session | Owns | Must NOT touch |
|---|---|---|
| S1 | `os-build/kernel/`, `os-build/buildroot/`, `os-build/test/ci-boot*.exp`, `os-build/board/`, `.github/workflows/buildroot-build.yml`, `.github/workflows/kernel-config.yml`, tracker §4, plan.md | `compositor/`, `shell/`, `apps/`, `lifecycled/`, `powerd/`, `installer/` |
| S2 | `compositor/`, `spikes/`, `os-build/test/spike-*`, spike/coverage results | `os-build/kernel/`, `os-build/buildroot/`, `shell/`, `apps/`, `lifecycled/` |
| S3 | `shell/`, `ui/`, `apps/` (except pkg/store/sandbox/runner/taskmanager), `docs/.../features/` | `os-build/`, `compositor/`, `lifecycled/`, `powerd/`, tracker §4 |
| S4 | `lifecycled/`, `powerd/`, `installer/`, `apps/pkg`, `apps/store`, `apps/sandbox`, `apps/runner`, `apps/taskmanager`, `docs/.../design/`, `docs/.../README.md`, other workflows, metrics, budgets, Makefile, scripts | `os-build/kernel/`, `os-build/buildroot/`, `compositor/`, `shell/`, tracker §4 |

## Startable tasks now (hardware-independent)

### S1 — Boot/base hardening + M2 prep (coordinator)
1. **i915 DMC firmware timing**: make i915 a module (or `CONFIG_EXTRA_FIRMWARE`) so DMC loads after rootfs (fixes runtime-PM-disabled caveat; P5 battery depends on it).
2. **P2-T03 code**: confirm ath10k/btusb/r8152 module emissaries build; add `CONFIG_BT_HCIBTUSB=m` if needed.
3. **P2-T06 prep**: add an RTC-wakealarm sleep-test script (`os-build/test/sleep-test.sh`) for automated 10-cycle validation.
4. **P2 image build**: trigger a build; verify image size + all boot chains.
5. Coordinate/merge other sessions' commits; update tracker.

### S2 — Compositor (P3)
1. **P3-T02** XDG shell, layer shell, multi-monitor, HiDPI (two monitors at different scales).
2. **P3-T03** XWayland.
3. Continue T04–T08 as time allows.
Acceptance: layer shell works (unblocks S3), xdg-toplevel, two-monitor test.

### S3 — Shell & apps (P4) + P7 UI
1. **P4-T06** lock screen/login (greetd + custom greeter) — implement the greeter/PAM pieces.
2. **P4-T13** PDF viewer integration (poppler).
3. **P4-T14** Player (GStreamer).
4. **P7-T07** Settings → Features UI (feature manager).
5. On-device acceptance harness for P3-T02 when available.
Acceptance: `cargo test` green; new modules/tests; no changes outside territory.

### S4 — Process/power, platform, installer, docs (P5–P9)
1. **P5 hardware-independent**: 72-hour soak harness, gate report generator; wire Activity Manager UI.
2. **P6-T05/T06** Windows engine packaging + installer flow completion.
3. **P8-T01** live USB image build path (script + docs).
4. **P9** RAUC A/B update wiring (state machine → real config).
5. Docs/design pass (D3/D5/D8/D10, feature docs, README) continuous.
Acceptance: `cargo test` green; docs updated; no changes outside territory.

## Rules
1. Read `docs/2026/11/17/os-implementation/plan.md` before/after work.
2. Do not start a task another session marked In progress.
3. Commit only your territory with a task ID in the message.
4. Run `cargo test` (or your crate's tests) before claiming Done.
5. Record D6 results under `docs/2026/11/26/results/` where measurable.
6. S1 has final merge authority; conflicts favor S1.

## Out of scope until hardware resumes
- Wi-Fi/BT/audio/touchpad/sleep bare-metal validation (needs the Dell network aligned).
- M2 closure.
