# P5/P6-T05/T06/P8-T01/P9 — S4 finish pass (2026-11-26)

Session S4, final hardware-independent pass over the already landed P5/P6/P8/P9
work: soak reporting, the Activity Manager window binding, integration tests and
docs. No hardware gate is claimed here.

## What changed

| Area | Change | Evidence |
|---|---|---|
| P5-T08 soak reporting | Per-cycle `aether-bench --json` kept in `p5-t08-soak-cycles/cycle-NNN.json`, merged into `p5-t08-soak.json`; the samples TSV gained the cycle exit code; `gate-report.py` fails G-PM5 on any failed cycle | `gate-report.py --self-test` (8 assertions), script run below |
| P5-T15 window binding | `apps/taskmanager/ui/activity-manager.slint` + `src/window.rs`: header, null label, tooltip, level labels, tones and sort order come from `EnergyColumnContract`; `apply_energy_impact_with` accepts an explicit contract | `tests/ui.rs` (1 test), headless render |
| P6-T05/T06 integration | `apps/runner/tests/windows_flow.rs`: packaged engine round trip + tamper detection, install → plan → uninstall with the engine and other prefix preserved | 4 integration tests |
| P8-T01 polish | `--check` reports tools before artifacts, `require_tools` helper, `sgdisk --verify` after assembly, help text fixed | `bash -n`, dry-run and check runs below |
| P9 RAUC flow | `update/tests/rauc_flow.rs`: install → pending boot → mark-good / mark-bad, state persistence, config + manifest template | 3 integration tests |

## Commands and results

```
$ python3 lifecycled/bench/gate-report.py --self-test
gate-report self-test: PASS (8 assertions)

$ grep "No failed soak cycles" <gate-report>       # 4-column samples, exit=1 in cycle 2
| No failed soak cycles | FAIL | 1 of 2 cycles exited non-zero |
# exit code 1 (FAIL) with a 4-column samples file; 0 with all-zero exits

$ cargo test -p aether-lifecycled -p aether-taskmanager -p aether-pkg \
    -p aether-sandbox -p aether-runner -p aether-store -p aether-installer -p aether-update
installer 23 + 3, lifecycled 63 + 2, pkg 15 + 3, runner 21 + 4,
sandbox 15, store 5, taskmanager 28 + 1, update 18 + 3  → 204 tests, 0 failed

$ cargo run -q -p aether-taskmanager -- --render docs/2026/11/26/results/screenshots
AETHER_ACTIVITY_MANAGER_RENDERED .../screenshots/activity-manager.png (1080x640)

$ os-build/scripts/make-live-usb.sh --binaries <stub> --dry-run
AetherOS live USB plan … image: 1094 MiB ; LIVE_USB_DRY_RUN ; exit 0

$ os-build/scripts/make-live-usb.sh --binaries <stub> --check
Missing tools: sgdisk … (distro hints) ; exit 1    # macOS host, no gdisk

$ cargo run -q -p aether-update -- rauc-config | tail -1
RAUC_CONFIG_OK
```

## Screenshot

`docs/2026/11/26/results/screenshots/activity-manager.png` — Processes tab with
the contract-driven Energy column (`Energy ↓`, `High`/`Medium`/`Low`/
`Not reported`) and the contract tooltip in the status bar. The rows are the
`demo_rows` sample set because the dev host has no `/proc`; the labels, tones,
sort order and header are the real contract values.

## Commit attribution

This round's files were staged for an S4 commit when a parallel session (S3)
committed its own P4-T14 work in the same worktree; the staged S4 files were
swept into that commit (`fcd0021`). The content is intact and reviewed — the
file list in that commit contains this round's `apps/taskmanager`,
`apps/runner/tests`, `lifecycled/bench`, `os-build/scripts`, `update/tests`
and docs changes alongside S3's player changes. No history was rewritten; this
note records the attribution so the tracker and review can find the work.

## Still pending (hardware / owner)

- 72-hour soak and G-PM1 on the reference machines (harness now reports per
  cycle and fails on a failed cycle).
- Live USB stick boot on the Dell; image assembly itself needs a Linux host with
  `sgdisk`/`mtools` (verified here only up to the plan and `--check`).
- RAUC `rauc install` / `mark-good` / `mark-bad` against real partitions.
