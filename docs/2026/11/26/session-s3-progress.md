# Session S3 (2026-11-26) — Progress & Working Plan

- Session: S3 running the v2 dependency-sliced plan
  (`docs/2026/11/22/parallel-plan-v2.md`), kickoff
  (`docs/2026/11/26/session-kickoff.md`).
- Hardware testing on the Dell is paused; this run is hardware-independent work.
- Tracker (§4 of the roadmap) is updated by S1 only. This file is the S3 working
  plan so parallel sessions can resume without duplication.

## Territory (exclusive)

`shell/`, `ui/`, `apps/` (except `apps/pkg`, `apps/store`, `apps/sandbox`,
`apps/runner`, `apps/taskmanager`), `docs/2026/11/16/os-project/features/`,
`docs/2026/11/26/results/` (S3 results).

Must NOT touch: `os-build/`, `compositor/`, `lifecycled/`, `powerd/`,
`installer/`, `update/`, roadmap tracker §4, `docs/.../design/`.

## To-do list (execution order)

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | P3-T02 → P4: wire the shell's layer-shell surfaces to the compositor host | **Done (2026-11-26)** | `shell/src/host.rs` plan/negotiate/report, `aether-shell --host-check`/`--host-plan`, 5 integration tests |
| 2 | P4-T14: GStreamer play/pause/seek edge cases + tests | **Done (2026-11-26)** | `Engine::set_position`/`set_duration`, spawn-order guarantees, 5 new tests |
| 3 | P7-T07: verify Settings → Features against `aether-config`'s `FeatureManager` | **Done (2026-11-26)** | 4 new page tests (catalog mirror, reload, errors, removal) |
| 4 | On-device acceptance harness kept current | **Done (2026-11-26)** | §1 `--host-check`, §4 features commands, §5 evidence, §6 blocker |

## Progress log

- 2026-11-26: Session start. Read the kickoff, v2 plan, roadmap tracker and the
  existing D6 results for S3. Baseline `cargo test -p aether-shell -p
  aether-player -p aether-config -p aether-ui` green (shell 81 lib + 3
  integration, player 17 lib + 2, config 34, ui 5).
- 2026-11-26: `shell/src/host.rs` — layer-shell host contract. The shell's five
  chrome surfaces are turned into the exact `zwlr_layer_surface_v1` requests
  (namespace, layer enum, anchor bitfield, size, exclusive zone, keyboard
  interactivity), size negotiation mirrors the compositor, and
  `BindingReport::scan` verifies the compositor's own markers. New binary
  commands `--host-plan` and `--host-check <log|->`. `control-center` added to
  `layers::required_namespaces`. Evidence:
  `docs/2026/11/26/results/p3-t02-layer-host-wiring.md`.
- 2026-11-26: Player transport edge cases. `Engine::set_position` records the
  resume position without starting a pipeline; `Engine::seek` only relaunches
  while playing; positions clamp to `set_duration`; empty URIs are rejected.
  The controller pauses before recording (no stray `gst-play-1.0`) and records
  instead of seeking before `play` (next-track/repeat no longer restart the old
  URI). Evidence: `docs/2026/11/26/results/p4-t14-player.md`.
- 2026-11-26: Features page verified against `aether_config::FeatureManager`:
  rows mirror the catalog and manager rows, a fresh load sees the persisted
  state, repeated actions report manager errors and write nothing, removal
  empties the state file. Evidence:
  `docs/2026/11/26/results/p7-t07-features-ui.md`.
- 2026-11-26: Harness updated (`docs/2026/11/26/results/on-device-acceptance-harness.md`):
  §1 now runs `aether-shell --host-check`, §4 lists the automated features
  tests, §5 records the evidence paths, §6 notes the `libwayland-client`
  blocker. F1 doc: SHL-AC-10 partially verified + new §9 describing the host
  contract.

## Final test status (2026-11-26)

`cargo test -p aether-shell -p aether-player -p aether-config -p aether-ui`
→ shell 91 lib + 8 integration (features 1, greeter 1, interaction 1,
layer_host 5), player 19 lib + 3 integration (codec 1, transport_edges 1,
ui 1), config 34, ui 5 — all pass, 0 failed.

## Blockers (unchanged, owner/hardware)

- Socket-level layer-shell bind: the development host has neither
  `libwayland-client` nor a linkable compositor (`xkbcommon` missing), so the
  Wayland client host runs on the image only. The compositor's
  `p3_t02_headless` test covers the server half.
- DA-AC-06 real audio, SHL-AC-* on-device rows, cold-launch/idle timings need
  AetherOS on the Dell (hardware paused).
- P3-T03 XWayland validation, P3-T04 window management are S2 work.

## Coordination note (2026-11-26, shared working tree)

Commit `fcd0021` ("P4-T14: honest play/pause/seek edge cases") also contains
S4's in-flight files (`apps/taskmanager/**`, `apps/runner/tests/**`,
`update/tests/**`, `lifecycled/bench/**`, `installer/LIVE-USB.md`,
`os-build/scripts/make-live-usb.sh`, design docs, `Cargo.lock`). Cause: the four
sessions share one working tree and one Git index; S4 staged everything with
`git add -A` between S3's staging and S3's `git commit`, so the commit swept the
whole index. No work was lost and the commit is already pushed, so history was
not rewritten; S3's own files are exactly the ones listed in the other two
commits. **Mitigation for all sessions:** commit with
`git commit --only -- <paths>` (or check `git diff --cached` immediately before
committing) so another session's staged files cannot ride along.
