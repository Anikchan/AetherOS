# Session S4 (v2 plan) — Progress & Working Plan

- Date: 2026-11-23
- Session: S4 running the v2 dependency-sliced plan
  (`docs/2026/11/22/parallel-plan-v2.md`), owner-approved autonomous run.
- Owner instruction: "consider yourself as the session 1 and continue working
  on S4 v2 plan autonomously" — unlimited retries approved for this session.
- Tracker (§4 of the roadmap) is updated by S1 only; this session records
  progress here and appends to `docs/2026/11/17/os-implementation/plan.md`.

## Territory (exclusive)

`lifecycled/`, `powerd/`, `installer/`, `apps/pkg`, `apps/store`,
`apps/sandbox`, `apps/runner`, `apps/taskmanager`, `docs/.../design/`,
`docs/.../README.md`, other workflows (not buildroot-build / kernel-config),
`os-build/metrics/`, `os-build/budgets.toml`, `os-build/Makefile`,
`os-build/scripts/`.

Must NOT touch: `os-build/kernel/`, `os-build/buildroot/`, `compositor/`,
`shell/`, tracker §4.

## To-do list (execution order)

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | P5-T01 App identity + one cgroup scope per app | Done (2026-11-23) | `lifecycled/src/identity.rs`; tests |
| 2 | P5-T02 Lifecycle state machine + unit tests | Done (2026-11-23) | all 17 allowed rows + forbidden rows tested |
| 3 | P5-T03 Lifecycle protocol (will/did suspend) | Done (2026-11-23) | real-socket integration test with test app |
| 4 | P5-T04 Freeze/hibernate (cgroup.freeze, memory.reclaim, zram) | Done (logic+tests); hardware run pending | G-PM3 needs reference machine |
| 5 | P5-T05 Pressure-driven killer + priority bands | Done (policy+tests); hardware run pending | G-PM4 policy-level pass |
| 6 | P5-T06 Exceptions + background task budget | Done (logic+tests) | 10-min audio run needs hardware |
| 7 | P5-T07 Activity Manager + spike S7 | Not started | apps/taskmanager |
| 8 | P5-T08 50-app benchmark + 72 h soak harness | Harness Done; soak run pending (2026-11-23) | `lifecycled/bench/`, `aether-bench` |
| 9 | P5-T09 powerd policy engine | Done (2026-11-23) | G-BAT3 policy-verified; hardware timed run pending |
| 10 | P5-T10 Runtime PM tuner + revert | Done (2026-11-23) | revert/exclusion tested; kernel-log watcher follow-up |
| 11 | P5-T11 Battery estimator | Done (2026-11-23) | within 10% after 10 min tested |
| 12 | P5-T12 Charge limit control | Done (2026-11-23) | clamped 40-100, unsupported hidden |
| 13 | P5-T13 Low-battery actions + sleep policy | Done (2026-11-23) | thresholds once/cycle, swap rule tested |
| 14 | P5-T14 Battery benchmark + control-center view | Benchmark Done; view = S3 (2026-11-23) | `bench/`, PowerStatus JSON contract |
| 15 | P5-T15 Energy-impact column | API Done (2026-11-23); column with P5-T07 | `energy_by_app` |
| 16 | P5-T16 Battery analytics (spike S8) | Done (2026-11-23) | cycle count fallback tested; hardware check pending |
| 17 | P5-T17 Screen time (basic) | Done (2026-11-23) | screen-on/per-app/pickups/notifications/gaps |
| 18 | P6-T01 Package format + manifest inspector | Not started | |
| 19 | P6-T02 Native bundle install & run | Not started | |
| 20 | P6-T03 AppImage runner | Not started | |
| 21 | P6-T04 Sandbox + permission broker | Not started | |
| 22 | P6-T05/T06 Windows engine packaging + installer flow | Not started | |
| 23 | P6-T07/T08 Cross-platform bundles, macOS detector | Not started | |
| 24 | P6-T09 Software app (basic) | Not started | |
| 25 | P6-T10 Compatibility database | Not started | |
| 26 | P7/P8/P9/P10 startable items | Not started | after P5/P6 |
| 27 | Docs & design pass (D3/D5/D8/D10, feature docs, README) | Continuous | |

## Progress log

- 2026-11-23: Session start. Read v2 plan, D3, D5, D8, D10, gates, 04 §4/§11.2.
  Environment: Rust 1.100.0 (macOS host), crates cache has serde/serde_json/
  toml/thiserror. Confirmed `git status`: only S2's uncommitted `Cargo.lock`
  (smithay) and an untracked S1 doc — both untouched.
- 2026-11-23: P5-T01..T06 + P5-T08 harness Done. 62 unit + 2 integration tests
  pass in the repository workspace; `--simulate` verified the full
  hide→suspend→hibernate→kill flow with audio protection and foreground safety.
  Benchmark harness passes B1/B3/B4/B5/G-PM2/G-PM4 on the model; B2/B6/G-PM1/
  G-PM3 pending reference hardware. Evidence:
  `docs/2026/11/23/results/p5-t01-t06-lifecycle-core.md`. Note: S3's parallel
  `ui/` + `apps/*` work briefly broke `cargo metadata`; crate verified in an
  isolated copy meanwhile and again in the workspace once ui/src landed.
  S2/S3 files left untouched; `Cargo.lock` not committed because it carries
  their in-flight slint packages.
- 2026-11-23: P5-T09..T17 Done (powerd). 59 unit tests pass. Policy
  walk-through verified profile switching, thermal cap, low-battery thresholds
  and lid sleep. Benchmark sampler verified with dry-run rows. Battery view is
  an S3 handoff (PowerStatus JSON), energy-impact API ready for P5-T07.
  Evidence: `docs/2026/11/23/results/p5-t09-t17-powerd.md`.
