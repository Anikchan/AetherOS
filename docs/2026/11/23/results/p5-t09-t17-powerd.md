# P5-T09..T17 — power policy, tuner, estimator, analytics, screen time

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run
- Scope: `powerd/`
- Design: D5 `power-management-design.md`, F14, gates in D6/04 §11.2

## Verified

### Tests

```bash
cargo test -p aether-powerd
```

Result: **59 unit tests pass, 0 failed** (2026-11-23).

| Task | Tests |
|---|---|
| P5-T09 policy | battery → Saver + battery background policy; AC → Balanced + AC policy; user choice survives source changes; thermal Hot caps Performance at Balanced; unknown source keeps profile; battery event with new source switches profile |
| P5-T13 low battery/sleep | 20/10/5/3 % each fire once per discharge cycle; charging resets; 3 % hibernates when swap exists; configured 5 % action used; lid close suspends only when not charging; charge-limit clamp |
| P5-T10 tuner | enable writes + records old values; disable restores; device error within 10 min reverts and excludes; error outside window ignored; 11th exclusion disables tuning and reverts; write failures reported; exclusions TOML round trip |
| P5-T11 estimator | within 10 % after 10 minutes; "Estimating" during 2-minute warmup; single spike rejected; three spikes accepted; source change restarts warmup |
| P5-T11 telemetry | sysfs energy battery (percentage, watts, health, cycles, temperature, time to empty); charge-based battery uses voltage; UPower output parse; missing battery → Unsupported; missing percentage → error |
| P5-T12 charge | unsupported hardware reports no limit and hides control; limit applied, clamped to 40–100, read back |
| P5-T13 sleep | `mem_sleep` parse; deep chosen only when preferred + offered; unavailable mode rejected; hibernation needs swap ≥ RAM; kernel `disk` flag detected |
| P5-T16 analytics | cycle count; one health point per day; discharge session percentages within 1 %; standby drain over 8 h screen-off; energy by app ranking; estimated cycles; daily temperature min/max/avg; report labels missing values and estimates; JSON round trip |
| P5-T17 screen time | screen-on accumulation; per-app focus time adds up; pickups + notifications; midnight split; gaps are `None` not zero; matches a compositor focus log within 1 minute; JSON round trip |
| daemon wiring | battery source → profile + background policy; 4 % → suspend; charge limit applied; status reports estimating/percent/source/cycles; lid close; analytics and screen-time recording |

### Rule walk-through (no hardware)

```bash
cargo run -p aether-powerd --bin aether-powerd -- --simulate
```

Output (2026-11-23):

```text
source=battery: profile=Saver ... SetBackgroundPolicy(grace 10s, hibernate 300s, cpu 50)
battery=19: Notify("Battery at 19%. Consider plugging in.")
battery=9:  Notify("... Saver profile enabled.")
battery=4:  SetLowBatteryAction(Suspend); Suspend
battery=2:  Notify("Battery critical"); Suspend
source=ac:   profile=Balanced ... grace 30s, hibernate 600s, cpu 100
user=performance: SetProfile(Performance)
thermal=hot: SetProfile(Balanced) + notify
```

### Benchmark sampler

```bash
cargo run -p aether-powerd --bin aether-battery-sampler -- \
  --dry-run-samples 3 --interval 300 --out /tmp/p5t14.csv
```

Result: header + 3 labelled `dry-run` rows, `P5T14_SAMPLER_DRYRUN rows=3`.

## Gate status

| Gate | Status | Reason |
|---|---|---|
| G-BAT3 profile switching within 5 s | **policy-verified** | profile action emitted immediately on source change; timed hardware run pending |
| G-BAT1 battery life vs Windows | **not measured** | needs owner's laptop + dual boot |
| G-BAT2 idle drain ≤ 6 %/h | **not measured** | needs owner's laptop |
| PWR-AC-06/07 | policy implemented; hardware values pending | 04 §11.2 procedure scripted in `bench/` |
| P5-T14 control-center battery view | **handoff to S3** | `PowerStatus` JSON is the data contract; the view belongs to `shell/controlcenter` |
| P5-T15 energy-impact column | **API ready** | `BatteryAnalytics::energy_by_app`; column lands with Activity Manager (P5-T07) |
| Spike S8 cycle count | implemented with fallback | "Not reported by this battery" path tested; real check on hardware |

## Honest limits

- All hardware values in this log come from fake sysfs trees and scripted
  telemetry. No battery claim is made from a host without a battery.
- UPower is read through its public CLI (`upower -i`) so the daemon has no
  D-Bus dependency yet; the D-Bus event subscription is a follow-up when
  logind/UPower events land.
- The tuner's real device-error detection needs the kernel log watcher; the
  revert/exclusion state machine itself is fully tested.
- lifecycled consumes the background policy file on its next poll; a direct
  socket/D-Bus call is a follow-up integration task.

## Reproduce

```bash
cargo test -p aether-powerd
cargo run -p aether-powerd --bin aether-powerd -- --simulate
cargo run -p aether-powerd --bin aether-powerd -- --dry-run --once
```
