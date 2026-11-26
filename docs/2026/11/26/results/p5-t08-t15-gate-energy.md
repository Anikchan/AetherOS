# P5-T08 / P5-T15 — Soak gate report and Energy column contract (D6 evidence)

- Date: 2026-11-26
- Session: S4
- Host: macOS arm64 (model host; reference machines not available)
- Tasks: P5-T08 (72-hour soak harness + G-PM1/G-PM5 gate report),
  P5-T15 (Energy-impact column contract)

## P5-T08 — gate report generator

`lifecycled/bench/gate-report.py` reads `aether-bench --json` and emits a D6
report with `PASS` / `FAIL` / `PENDING` / `INCOMPLETE` per gate. A model-host
run can never produce PASS.

Commands and results:

```text
$ python3 lifecycled/bench/gate-report.py --self-test
gate-report self-test: PASS (7 assertions)

$ make -C os-build soak-smoke APPS=50 CYCLES=2 OUT_DIR=/tmp/aether-soak-make
cycle 1: apps=50 inactive=10.8% b1=PASS b3=PASS b4=PASS b5=PASS gpm2=PASS gpm4=PASS kills=6
soak cycle 1/2 complete (MemAvailable 0 kB -> 0 kB)
cycle 2: ... identical
P5T08_SOAK_DONE apps=50 cycles=2
```

Gate report verdict on this host:

| Gate | Verdict |
|---|---|
| G-PM1 — 50-app benchmark (B1–B6, 04 §4.3) | **PENDING** (model host; B2/B6 not measurable) |
| G-PM5 — 72-hour soak | **PENDING** (compressed run validates the harness) |

Reference-hardware path was exercised with synthetic evidence:
`--reference-hardware --b2-restore-ms 380 --b6-share 2.1 --soak-hours 72` plus
5 % baseline samples → G-PM1 **PASS**, G-PM5 **PASS**, exit 0. Missing B2/B6 or
a short soak returns `INCOMPLETE` (exit 2); a failed B1/B3 returns `FAIL`
(exit 1).

`soak-72h.sh` now samples `/proc/meminfo` `MemAvailable` before and after every
cycle (`p5-t08-soak-samples.tsv`) and merges the per-cycle JSON. On macOS the
kernel file is absent, so the samples are 0 and the drift check is PENDING —
correct, not fabricated.

## P5-T15 — Energy column contract

`aether-taskmanager --energy-contract` prints the binding consumed by the Slint
window:

```json
{"key":"energy","header":"Energy","unit":"impact","default_sort":"desc",
 "null_label":"Not reported","refresh_seconds":60,
 "levels":[{"level":"high","label":"High","min_share":0.66,"tone":"danger"},
           {"level":"medium","label":"Medium","min_share":0.33,"tone":"accent"},
           {"level":"low","label":"Low","min_share":0.0,"tone":"success"},
           {"level":"none","label":"None","min_share":0.0,"tone":"text_muted"}],
 "sources":[{"id":"battery-history","priority":1}, {"id":"computed","priority":2}]}
```

`apply_energy_impact` and `EnergyImpact::from_share` share the thresholds, so
the CLI, the export and the window cannot disagree. Tests: `27 passed` in
`aether-taskmanager` (including `energy_column_contract_is_stable`).

## Remaining work

- Full 72-hour soak and real G-PM1 numbers on the 8 GB and 4 GB reference
  machines (hardware paused).
- Activity Manager Slint window binding (S3 UI) and G-TM1/G-TM2 overhead gates.
