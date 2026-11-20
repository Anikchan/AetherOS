# Results — Spike S4: Battery state (P0-T10)

- Date: 2026-11-20 (S2 autonomous prep; run pending on spare Dell)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Tool: `os-build/test/spike-s4.sh`
- Status: **Prep complete; hardware run pending** (spare Dell with battery required)

## What is proven so far (prep)

- Script `os-build/test/spike-s4.sh` created and `bash -n` syntax-checked.
- Markers defined and documented:
  `S4_BATTERY_PRESENT` / `S4_BATTERY_ABSENT`,
  `S4_SYSFS_OK` / `S4_SYSFS_FAIL`,
  `S4_UPOWER_OK` / `S4_UPOWER_FAIL` / `S4_UPOWER_UNAVAILABLE`,
  `S4_CHARGE_LIMIT_SUPPORTED` / `S4_CHARGE_LIMIT_UNSUPPORTED`,
  `S4_TIME_REMAINING_OK` / `S4_TIME_REMAINING_NA`,
  `S4_BATTERY_SPIKE_DONE`.
- Reads sysfs (`/sys/class/power_supply/BAT*`) for status, capacity, energy,
  voltage, power, cycle count, health; UPower (`upower --dump`,
  `--show-info`) for percentage, state, time-to-empty/full; probes
  `charge_control_end_threshold` / `charge_control_start_threshold` /
  `charge_behaviour` / `charge_control_limit` for charge-limit support.
- Read-only: no sysfs writes; charge-limit attributes are only reported
  (writable flag), not changed.

## Pending (needs spare Dell with battery)

- Run `os-build/test/spike-s4.sh` on the spare Dell booted into AetherOS.
- Record readings in the table below and compare sysfs vs UPower (must
  match system tools within tolerance).
- Probe charge-limit on the Dell's firmware (likely unsupported on
  consumer Dell; record either way).

## Run table (fill at first Dell boot)

| Field | sysfs | UPower | Match? |
| --- | --- | --- | --- |
| status | _pending_ | _pending_ | _pending_ |
| capacity (%) | _pending_ | _pending_ | _pending_ |
| energy_now / energy_full | _pending_ | _pending_ | _pending_ |
| power_now (W) | _pending_ | _pending_ | _pending_ |
| time to empty/full | n/a | _pending_ | _pending_ |
| charge limit supported | _pending_ | n/a | n/a |
| cycle count | _pending_ | n/a | n/a |

## Acceptance (P0-T10)

- Readings match system tools (UPower/sysfs cross-check).
- Charge-limit capability recorded (supported/unsupported + firmware).
- Gate: results recorded here; task marked Done only after the run.

## Notes

- Blocked on spare Dell availability (owner action). S1 (critical path) and
  S2 prep are unblocked; this file will be updated with real numbers at the
  first Dell boot, then P0-T10 marked Done by S1 (tracker owner).
