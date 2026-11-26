# Session S4 (2026-11-26) — Progress & Working Plan

- Session: S4 running the v2 dependency-sliced plan
  (`docs/2026/11/22/parallel-plan-v2.md`), kickoff
  (`docs/2026/11/26/session-kickoff.md`).
- Hardware testing on the Dell is paused; this run is hardware-independent work.
- Tracker (§4 of the roadmap) is updated by S1 only. This file is the S4 working
  plan so parallel sessions can resume without duplication.

## Territory (exclusive)

`lifecycled/`, `powerd/`, `installer/`, `apps/pkg`, `apps/store`,
`apps/sandbox`, `apps/runner`, `apps/taskmanager`, `docs/2026/11/16/os-project/design/`,
`docs/2026/11/16/os-project/README.md`, non-S1 workflows, `os-build/metrics/`,
`os-build/budgets.toml`, `os-build/Makefile`, `os-build/scripts/`.

Must NOT touch: `os-build/kernel/`, `os-build/buildroot/`, `os-build/board/`,
`compositor/`, `shell/`, roadmap tracker §4.

## To-do list (execution order)

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | P5: 72-hour soak harness + G-PM1/G-PM5 gate report generator | **Done (2026-11-26)** | `gate-report.py`; per-cycle JSON in `p5-t08-soak-cycles/`; TSV has the cycle exit code and G-PM5 fails on a failed cycle; hardware gates PENDING |
| 2 | P5-T15: wire Activity Manager energy-impact column contract | **Done (2026-11-26)** | Slint window `ui/activity-manager.slint` + `src/window.rs` bound to the contract; `--render` evidence; 28 unit + 1 UI test |
| 3 | P6-T05: Windows engine packaging (versioning + verification) | **Done (2026-11-26)** | `engine_package.rs`, `engines package|verify`; `tests/windows_flow.rs` packages + verifies + detects tampering |
| 4 | P6-T06: Windows installer flow + prefix-only uninstall | **Done (2026-11-26)** | `windows.rs`; `tests/windows_flow.rs` runs install → launch plan → uninstall (AP-AC-03/04) |
| 5 | P8-T01: live USB image build path (script + docs) | **Done (build path 2026-11-26)**; boot pending hardware | `make-live-usb.sh` with `--check`/`--dry-run` + `sgdisk --verify`, `installer/LIVE-USB.md`, D4 §2.1 |
| 6 | P9: wire update state machine to a real RAUC A/B config | **Done (config + bridge 2026-11-26)**; slot install pending hardware | `update/config/*`, `update/src/rauc.rs`, `rauc-bundle.sh`, `tests/rauc_flow.rs` |
| 7 | Docs/design pass (D3/D5/D8/D10, F7, README) | **Done (2026-11-26)** | status sections added |
| 8 | CI assist for S2: xwayland + x11-apps for P3-T03 | **Done (2026-11-26)** | commit `af9de2d`; same job runs S4 harness checks |

## Progress log

- 2026-11-26: Session start. Read the kickoff, v2 plan, session plan, D3/D5/D8/D10
  and F13/F14/F7. Baseline `cargo test` for the S4 crates is green (installer 26,
  lifecycled 64, pkg 18, runner 12, sandbox 15, store 5, taskmanager 26).
- 2026-11-26: P3-T03 CI assist committed first (`af9de2d`) at S1/coordinator
  request: `xwayland x11-apps` added to the CI apt step. The same job now runs
  the S4 harness checks (gate report self-test, energy contract, RAUC config,
  script syntax).
- 2026-11-26: P5-T08 gate report + soak baseline sampling; P5-T15 energy column
  contract. `gate-report.py --self-test` PASS (7 assertions); compressed
  50-app soak (2 cycles) completes with G-PM1/G-PM5 PENDING on the model host.
  Evidence: `docs/2026/11/26/results/p5-t08-t15-gate-energy.md`.
- 2026-11-26: P6-T05/T06 engine packaging + Windows installer flow. An engine
  manifest pins every file's SHA-256; the installer flow creates a per-app
  prefix, records the produced program and writes a launcher; uninstall removes
  only the prefix + launcher. 21 runner tests. Evidence:
  `docs/2026/11/26/results/p6-t05-t06-windows-engine.md`.
- 2026-11-26: P8-T01 live USB build path (rootless GPT assembly, dry-run and
  tool check verified on macOS) and P9 RAUC wiring (real `system.conf`, bundle
  manifest, `rauc status` parsing, sync, mark-good/bad, bundle script). 18
  update tests. Evidence: `docs/2026/11/26/results/p8-t01-live-usb.md`,
  `docs/2026/11/26/results/p9-rauc-wiring.md`.
- 2026-11-26: Docs pass: D3 §12 (gate report/soak), D5 §14 (energy contract),
  D8 §5.1 (engine packaging/install flow), D10 §5.1 (column binding), D4 §2.1
  (live USB) + §6.1 (RAUC), F7 PWR-12 + §9, project README status, crate
  READMEs, scripts README.

- 2026-11-26 (finish pass): soak reporting made durable per cycle and
  `gate-report.py` now fails G-PM5 on a failed cycle (self-test 8 assertions);
  Activity Manager window bound to the energy contract with a headless UI test
  and a screenshot (`results/screenshots/activity-manager.png`);
  `apps/runner/tests/windows_flow.rs` (4 tests) and `update/tests/rauc_flow.rs`
  (3 tests) added; live USB script `--check`/`--dry-run`/`sgdisk --verify`
  polish; docs updated (D3 §12, D4 §6/`LIVE-USB.md`, D5, D8 §5.1, D10 §5.1,
  project README, crate/bench/config READMEs). All 204 tests in the eight S4
  crates pass. Evidence: `docs/2026/11/26/results/p5-soak-ui-p6-p8-p9-finish.md`.

## Blockers (unchanged, owner/hardware)

- 72-hour soak, real G-PM1/B2/B6 values, G-BAT*, G-TM*, AP-AC-03/04 and the live
  USB stick boot need AetherOS on the Dell (hardware paused).
- Activity Manager window: Processes tab bound; the remaining tabs and the
  shortcut/confirmation flows are still open.
- Live persistence service and the BIOS live path are image-overlay/S1 work.
