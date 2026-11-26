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
| 9 | P6-T10: compatibility database (schema, 40 slots, scoring engine) + hardware harness | **Done (2026-11-26)** | `apps/runner/src/compat.rs`, `compat_harness.rs`; DOS: real Windows apps pending hardware |
| 10 | P9-T02: rollback/health-check service logic | **Done (2026-11-26)** | `update/src/health.rs`; probes, policy, persisted retries, deadline; slot write pending hardware |
| 11 | P6-T09: Software app JSON UI contract for S3 | **Done (2026-11-26)** | `apps/store/src/contract.rs` + `aether-store --json`; `needs-permission` exit 3 |
| 12 | Docs/design pass for the above (D8 §9/§10, D4 §6.2, README) | **Done (2026-11-26)** | D10 reviewed, unchanged (runtime monitoring, no boot-health contract) |
| 13 | P9-T03: optional LUKS2 disk encryption in the installer (planning + tests) | **Done (planning 2026-11-26)**; encrypted boot pending hardware | `LuksEncryption`/`PlanOptions`/`plan_with_options`, cryptsetup + crypttab + mapper operations, `set_luks_uuid`; 5 integration tests |
| 14 | P6-T10: real 40-app compatibility spec template (installers + probes) | **Done (2026-11-26)**; real runs pending hardware | `apps/runner/src/compat_reference.rs`, `apps/runner/compat/40-app-spec-template.json`, `compat reference-template|reference-slots`; 6 tests |
| 15 | P8-T05: Secure Boot path decision + prerequisites | **Deferred (documented 2026-11-26)** | `docs/2026/11/26/results/p8-t05-secure-boot-deferred.md`, D4 §5.1, F5 INS-AC-08 note; no code |

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
  Commit note: a parallel S3 commit (`fcd0021`, P4-T14) swept the staged S4
  files in the shared worktree; the S4 changes are in that commit and history
  was not rewritten.

- 2026-11-26 (P6-T09/T10, P9-T02 pass): compatibility database completed —
  40 slots with archetypes, per-check evidence, a deterministic tier/score
  engine, and a harness that runs in dry-run or import mode off hardware
  (`apps/runner/src/compat.rs`, `compat_harness.rs`, 16 tests); the Software
  app JSON contract (`apps/store/src/contract.rs`, 7 tests) with
  `aether-store --json` and dual-boot guidance for Tier D; the update
  health-check service (`update/src/health.rs`, 8 tests) with probes, policy,
  persisted retry counters and a confirmation deadline, plus an
  `update_in_progress` guard on staging. Design docs D8 §9/§10 and D4 §6.2
  added. Evidence: `docs/2026/11/26/results/p6-t09-t10-p9-t02.md`.

- 2026-11-26 (P9-T03, P6-T10 reference spec, P8-T05 pass): optional LUKS2 is
  planned by the installer — `--encrypt` orders luksFormat → luksOpen →
  crypttab → mkfs on `/dev/mapper/aether-root`, reserves the 16 MiB header,
  refuses LUKS1/bad mappers and rewrites the boot cmdline once the UUID is
  known (`set_luks_uuid`); 5 integration tests (`installer/tests/luks_plan.rs`).
  The 40 compatibility slots now have a real reference spec (vendor installer,
  entry point, window probe, tweaks) generated from
  `apps/runner/src/compat_reference.rs` into
  `apps/runner/compat/40-app-spec-template.json`, with
  `compat reference-template|reference-slots` and 6 tests. P8-T05 (Secure
  Boot) is recorded as deferred with prerequisites, risks and a test matrix;
  no code. Docs: D4 §2.2/§5.1, F5 §5.1 + INS-AC-10, project README.

- 2026-11-27 (P5-T07 tabs + P6-T04 Landlock): the Activity Manager window now
  renders the Performance (five 60 s graphs + CPU/memory/disk/network/GPU
  details), Battery (powerd status + F14 24 h energy-by-app with P5-T15 levels,
  health trend) and Sensors (temperatures with 85/95 °C tones, fans, GPU,
  alerts, explicit fallbacks) tabs from `src/tabs.rs`; `src/shortcut.rs`
  publishes the `Ctrl+Shift+Esc` (Windows profile) / `Super+Escape` binding and
  matches toolkit key events; `request_end_task` builds the unsaved-work and
  Force Quit dialogs and the window maps Cancel/End task/Force Quit to
  none/SIGTERM/SIGKILL. CLI: `--shortcut`, `--tab`, `--render` draws all four
  tabs. Landlock: `apps/sandbox/src/landlock.rs` (ABI probe, profile → rules,
  ABI masking, raw 444/445/446 syscalls, `PR_SET_NO_NEW_PRIVS`) and the
  `aether-landlock` shim, which exits 126 without running the app when
  enforcement is impossible; `Enforcer::command_with_landlock` and
  `Runner::require_landlock`/`--strict-landlock` use it. Verified: taskmanager
  44 tests, sandbox 30 (incl. 3 shim integration tests), runner 45.
  Evidence: `docs/2026/11/27/results/p5-t07-tabs-p6-t04-landlock-s4.md`.

## Blockers (unchanged, owner/hardware)

- 72-hour soak, real G-PM1/B2/B6 values, G-BAT*, G-TM*, AP-AC-03/04 and the live
  USB stick boot need AetherOS on the Dell (hardware paused).
- Activity Manager window: Processes/Performance/Battery/Sensors bound; the
  remaining tabs (App history, Startup apps, Users, Details, Services,
  Android) are placeholders. Global Ctrl+Shift+Esc registration is the
  compositor/shell's, and the hung flag comes from the P3-T08 watchdog.
- Landlock kernel allow/deny runs and the `/usr/libexec/aether-landlock`
  install need the Linux image (S1 packaging).
- Live persistence service and the BIOS live path are image-overlay/S1 work.
- The 40 compatibility slots are defined, the reference spec is ready and the
  harness runs, but the 40 real Windows apps (APP-AC-06), the encrypted boot
  (P9-T03 hardware half) and the real inactive-slot write/rollback cycle
  (P9-T02 hardware half) still need the reference machine.
