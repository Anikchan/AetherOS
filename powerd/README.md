# aether-powerd — power and battery (P5-T09..T17)

Implements design D5 and F14 (battery analytics, screen time). Policy decisions
are pure and unit-tested; mechanisms are trait objects with fakes for tests.

## Status

| Task | State | Evidence |
|---|---|---|
| P5-T09 Profile policy engine | Done (logic + tests) | `src/policy.rs`: AC/Battery profile switching, user choice retained per source, thermal cap, background policy → lifecycled |
| P5-T10 Runtime PM tuner | Done (logic + tests) | `src/tuner.rs`: safe list, old-value recording, 10-min watch window, revert + exclusion, >10 exclusions disables tuning |
| P5-T11 Battery telemetry + estimator | Done (logic + tests) | `src/telemetry.rs` (UPower + sysfs), `src/estimator.rs` (2–5 min average, 2-min warmup, 3× spike rejection) |
| P5-T12 Charge limit | Done (logic + tests) | `src/charge.rs`, clamped 40–100%, hidden when unsupported |
| P5-T13 Low battery + sleep policy | Done (logic + tests) | `src/policy.rs` §7 machine, `src/sleep.rs` (s2idle/deep, suspend-then-hibernate needs swap ≥ RAM) |
| P5-T14 Battery benchmark + control-center view | Benchmark done; view is S3 | `bench/battery-sampler.sh`, `aether-battery-sampler`; `PowerStatus` JSON is the control-center contract |
| P5-T15 Energy-impact column | API done (Activity Manager column is P5-T07) | `BatteryAnalytics::energy_by_app(now, days)` |
| P5-T16 Battery analytics | Done (logic + tests) | `src/analytics.rs`: cycle count, daily health, estimated cycles, sessions, temp history, standby drain, energy by app, report export |
| P5-T17 Screen time | Done (logic + tests) | `src/screentime.rs`: screen-on, per-app focus, pickups, notifications, gaps, focus-log comparison |

## Commands

```bash
aether-powerd --once          # one battery sample + policy evaluation, status JSON
aether-powerd --simulate      # rule walk-through, no hardware
aether-powerd                 # loop (15 s samples by default)

aether-power-report           # text battery report from the history store
aether-battery-sampler --label W1 --hours 1 --interval 300 --out w1.csv
powerd/bench/battery-sampler.sh W1
```

## Configuration

`/etc/aether/power/powerd.toml` or `$XDG_CONFIG_HOME/aether/power/powerd.toml`:

```toml
state_dir = "/var/lib/aether/powerd"
dry_run = false
charge_limit = 80
low_battery_action = "suspend"   # or "hibernate"
swap_gib = 8
ram_gib = 16
suspend_then_hibernate = false
```

Exclusions for the tuner are stored in
`$XDG_CONFIG_HOME/aether/power/exclusions.toml` as `exclude = ["pcie-aspm"]`.

## Data

- `battery-history.json` — samples, daily health/temperature, sessions,
  per-app energy (retention is enforced by the battery-analytics pack).
- `screen-time.json` — per-day screen-on time, per-app time, pickups,
  notification counts; missing days are gaps.
- `status.json` — `PowerStatus` for the control center: percentage, source,
  profile, time-remaining estimate (or "estimating"), health, cycles,
  temperature, charge limit, tuned devices, exclusions.

## Integration points

- **lifecycled**: the effective background policy is written to
  `$XDG_RUNTIME_DIR/aether/background-policy.json` (`FileBackgroundSink`).
  lifecycled applies it on the next poll; a direct D-Bus/socket call is a
  follow-up.
- **Shell control center (S3)**: read `status.json`; the view itself is S3's
  P4-T04/P5-T14 split. This crate owns the data and actions, not the UI.
- **Activity Manager (P5-T07)**: use `BatteryAnalytics::energy_by_app` for the
  energy-impact column, and `PowerStatus` for the battery tab.

## Honest limits

- Hardware behaviour (G-BAT1/G-BAT2, real charge limit, tuner revert on real
  device errors, UPower values) needs the owner's laptop. Tests here use
  sysfs fakes and scripted telemetry.
- Spike S8 (cycle count availability) is implemented as a read with an
  explicit "Not reported by this battery" fallback; the real value is recorded
  on the Dell/hardware run.
- Battery benchmark numbers from a machine without a battery or from a VM do
  not count (04 section 10).
