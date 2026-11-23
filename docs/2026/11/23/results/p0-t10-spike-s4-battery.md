# Results — P0-T10: Battery spike S4 (UPower + sysfs)

- Date: 2026-11-23 (real-machine run on the Dell)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Tool: `os-build/test/spike-s4.sh`
- Host: Dell Inspiron 5402, Intel i7-1165G7, Ubuntu (kernel 7.0.0-44), battery BAT0
- Status: **Spike passes.** All required markers emitted.

## Markers
- `S4_BATTERY_PRESENT` ✅
- `S4_SYSFS_OK` ✅
- `S4_UPOWER_OK` ✅
- `S4_CHARGE_LIMIT_SUPPORTED` ✅ (attributes present, not writable by non-root)
- `S4_TIME_REMAINING_OK` ✅
- `S4_BATTERY_SPIKE_DONE` ✅

## sysfs readings (BAT0)
| Attribute | Value |
| --- | --- |
| type | Battery |
| status | Full |
| capacity | 100 |
| capacity_level | Full |
| voltage_now | 15.112 V |
| charge_now / charge_full | 1653000 / 1653000 |
| charge_full_design | 3534000 |
| current_now | 1000 |
| cycle_count | 0 |
| health | Good |

## UPower readings
- state: fully-charged
- energy: 24.795 Wh (energy-full-design: 53.01 Wh)
- voltage: 15.112 V
- time to empty: 68.9 days (on AC, idle)
- percentage: 100%
- capacity: 46.77% of design (battery degraded)

## Charge-limit control
- `charge_control_end_threshold` = 90 (not writable by non-root user)
- `charge_control_start_threshold` = 50 (not writable by non-root user)
- Attributes exist → `S4_CHARGE_LIMIT_SUPPORTED`. On AetherOS, `aether-powerd`
  (root) will write these values. Writability by the service user must be
  verified in P5-T12.

## Findings for P5 (power management)
- UPower and sysfs both work on this hardware (Intel Tiger Lake laptop).
- Battery capacity at 46.77% of design — good test case for P5-T16 analytics
  (health trend, estimated cycles).
- cycle_count reports 0 — may be "not reported" on this hardware; P5 must
  display "not reported" rather than 0 (R28).
- Charge-limit thresholds present but read-only for non-root; root service
  can write them.
- Time-to-empty reported by UPower (68.9 days on AC/idle) — usable for
  P5-T11 smoothing baseline.

## Acceptance (P0-T10)
- ✅ Readings match system tools (UPower + sysfs agree).
- ✅ Charge-limit controls detected.
- Done on Linux (not AetherOS), but the sysfs/UPower interface is identical;
  no AetherOS-specific re-test required.