# Battery benchmark (P5-T14, 04 section 11.2)

Gates: **G-BAT1** (within 15% of Windows 11 for W2/W3), **G-BAT2** (idle drain
≤ 6%/h limit, ≤ 4%/h target), **PWR-AC-06**, **PWR-AC-07**.

## Setup

- Owner's laptop, full charge, charge limit at 100%, brightness 50%, Wi-Fi on.
- Same browser profile and same 20 test pages as the Windows 11 run.
- Balanced profile. Lid open for W1/W2/W3.

## Runs

| Workload | Length | Command | Measure |
|---|---|---|---|
| W1 idle | 1 h | `./battery-sampler.sh W1` | drop every 5 min |
| W2 browsing | 1 h | `./battery-sampler.sh W2` | drop; time to 10% |
| W3 video | 1 h | `./battery-sampler.sh W3` | drop |

Repeat each workload 3 times on AetherOS, take the median, then repeat on
Windows 11 on the same laptop with the same settings. Each CSV row is
`timestamp,label,percent,source,charging,rate_w,energy_wh,health_percent,cycle_count`.

## Pass criteria

- W1 drain ≤ 6%/h (limit) and ≤ 4%/h (target).
- W2 and W3 battery life within 15% of Windows 11.
- No failed device tuning during the runs (check `powerd.log`/status
  `tuning_disabled`).

## Evidence

Write the medians and the Windows comparison into
`docs/<year>/<month>/<day>/results/p5-t14-battery-benchmark.md`. A run without
a battery, or in a VM, is recorded as "not measured" and never counted
(D6 / 04 section 10).
