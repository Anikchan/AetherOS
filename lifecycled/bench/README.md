# lifecycled benchmark harness (P5-T08)

Gates covered: **G-PM1** (50-app benchmark), **G-PM2** (inactive memory),
**G-PM4** (foreground safety), **G-PM5** (72-hour soak), and the B1–B6 checks
from `04-test-and-acceptance.md` section 4.

## Scripts

| Script | Purpose | Typical use |
|---|---|---|
| `bench-50.sh` | 50-app benchmark, one or more cycles | Before M4, after every process-manager change |
| `soak-72h.sh` | 72-hour soak with cycle-baseline memory samples | Before M4 and before M5 |
| `gate-report.py` | Turns bench/soak JSON into a D6 G-PM1/G-PM5 verdict | Called by both scripts; runnable on its own |

Environment: `APPS` (default 50), `CYCLES`, `SOAK_HOURS`, `CYCLE_SECONDS`,
`OUT_DIR`, `REFERENCE_HARDWARE=1`, `B2_RESTORE_MS`, `B6_SHARE`.

Both scripts call `aether-bench`, which drives the real daemon/policy against
real client processes:

```bash
aether-bench --apps 50 --cycles 1 --report docs/2026/11/23/results/p5-t08-bench-50.md
```

## Gate report (D6)

`gate-report.py` reads `aether-bench --json` output and emits a Markdown gate
report with an explicit verdict per gate: `PASS`, `FAIL`, `PENDING` or
`INCOMPLETE`. Exit codes: `0` pass/pending, `1` fail, `2` missing
reference-hardware evidence.

```bash
python3 gate-report.py --self-test
python3 gate-report.py --bench-json p5-t08-bench-50.json --gate all \
  --reference-hardware --b2-restore-ms 380 --b6-share 2.1 \
  --out p5-t08-gate-report.md
```

`soak-72h.sh` samples `/proc/meminfo` `MemAvailable` before and after every
cycle (`p5-t08-soak-samples.tsv`) so G-PM5's "memory returns to baseline" rule
is measured, not assumed.

## Soak reporting files

| File | Content |
|---|---|
| `p5-t08-soak-cycles/cycle-NNN.json` | Raw `aether-bench --json` per cycle (kept as evidence) |
| `p5-t08-soak.json` | All cycles merged, read by `gate-report.py` |
| `p5-t08-soak-samples.tsv` | `cycle<TAB>MemAvailable_before_kb<TAB>MemAvailable_after_kb<TAB>exit` |
| `p5-t08-gate-report.md` / `.json` | D6 verdict for G-PM1/G-PM5 |

A non-zero per-cycle exit is recorded in the TSV and fails the G-PM5 check
`No failed soak cycles`, so a soak with a crashed or skipped cycle can never be
reported as a pass. `REFERENCE_HARDWARE=1` adds `--reference-hardware`, which is
the only way to reach a gate `PASS`.

## Honesty rules (04 section 10, D6)

- On the macOS dev host the cgroup memory accounting is **modelled** at 20 MB
  per app (04 §4.1) plus a 1.5 GB baseline; B1/B3/B4/B5/G-PM2/G-PM4 are
  policy-level results and are labelled as such in the report.
- A model-host run can never produce a gate `PASS`. The gate report returns
  `PENDING` until the 8 GB and 4 GB reference machines run it.
- **B2** (restore ≤ 500 ms), **B6** (full-pressure share < 5%), the real G-PM1
  memory numbers and the G-PM3 cgroup numbers are only valid on the reference
  machines. Pass them to `gate-report.py` only when they were actually
  measured; the report prints `PENDING`/`INCOMPLETE` otherwise.
- Every run writes a D6-format result file; keep it with the release evidence.
