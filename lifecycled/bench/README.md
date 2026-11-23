# lifecycled benchmark harness (P5-T08)

Gates covered: **G-PM1** (50-app benchmark), **G-PM2** (inactive memory),
**G-PM4** (foreground safety), **G-PM5** (72-hour soak), and the B1–B6 checks
from `04-test-and-acceptance.md` section 4.

## Scripts

| Script | Purpose | Typical use |
|---|---|---|
| `bench-50.sh` | 50-app benchmark, one or more cycles | Before M4, after every process-manager change |
| `soak-72h.sh` | 72-hour soak with random switching | Before M4 and before M5 |

Environment: `APPS` (default 50), `CYCLES`, `SOAK_HOURS`, `OUT_DIR`.

Both scripts call `aether-bench`, which drives the real daemon/policy against
real client processes:

```bash
aether-bench --apps 50 --cycles 1 --report docs/2026/11/23/results/p5-t08-bench-50.md
```

## Honesty rules (04 section 10, D6)

- On the macOS dev host the cgroup memory accounting is **modelled** at 20 MB
  per app (04 §4.1) plus a 1.5 GB baseline; B1/B3/B4/B5/G-PM2/G-PM4 are
  policy-level results and are labelled as such in the report.
- **B2** (restore ≤ 500 ms), **B6** (full-pressure share < 5%), the real G-PM1
  memory numbers and the G-PM3 cgroup numbers are only valid on the 8 GB and
  4 GB reference machines. The report prints "not measured" for them on other
  hosts and never claims a gate from a model.
- Every run writes a D6-format result file; keep it with the release evidence.
