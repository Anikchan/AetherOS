# aether-taskmanager — Activity Manager core (P5-T07, F13)

The Slint window is a thin binding over this crate: every metric, action and
export used by the UI lives here and is tested headlessly. The window itself
uses the S3 design system (`ui/`) and is not wired yet — see Status.

## Status

| Feature | State |
|---|---|
| TM-01 Processes tab (columns, per-column sort) | Core done: name, PID, PPID, state, CPU %, PSS/RSS, disk, GPU (when reported), energy, wakeups, threads, open files, command, lifecycle state/band |
| TM-02 End task / end process tree | Core done: SIGTERM, `--force` SIGKILL, refuses PID 1 and itself; unsaved-work warning is delivered by the lifecycle SDK's `WillSuspend` before a suspend, and the CLI prints the warning |
| TM-03 Efficiency mode | Core done: lowers nice and `cpu.weight` of the app scope, restores on disable |
| TM-04/05/06/07/08 Performance tab | Core done: CPU total/per-core, meminfo, zram, PSI level, disk and network rates, 60 s graphs |
| TM-09 GPU detail | Busy % and VRAM where the driver reports them; per-process GPU is Pack D |
| TM-10 Sensors | hwmon + thermal + fans, with explicit "Not reported" fallbacks |
| TM-11 Battery tab | Reads `powerd` `PowerStatus` JSON |
| TM-12 App history | Battery analytics energy by app (last 24 h / 7 days) |
| TM-13 Startup apps | Lists user + system autostart entries; enable/disable persists in the `.desktop` file |
| TM-14 Users | `loginctl list-users` |
| TM-16 Details tab | Same process table with PPID, user, threads, open files, command |
| TM-17 Services | `systemctl list-units --type=service` |
| TM-19 Export | CSV, JSON and text |
| TM-20 Alerts / TM-21 menu-bar indicator | v1, not started |

## Energy column contract (P5-T15)

`aether-taskmanager --energy-contract` prints the JSON binding the Slint window
consumes, so labels, thresholds, tones, sort order and refresh cadence live in
one place instead of the UI:

```json
{"key":"energy","header":"Energy","unit":"impact","default_sort":"desc",
 "null_label":"Not reported","refresh_seconds":60,
 "levels":[{"level":"high","label":"High","min_share":0.66,"tone":"danger"}, ...],
 "sources":[{"id":"battery-history","priority":1,...},
            {"id":"computed","priority":2,...}]}
```

Source priority: measured F14 `energy_by_app` first; when no history exists the
row falls back to `EnergyImpact::classify(cpu, wakeups, gpu)` (F7 PWR-12).
`apply_energy_impact` and `EnergyImpact::from_share` use the same thresholds.


## Commands

```bash
aether-taskmanager --once                 # process + system snapshot
aether-taskmanager --once --json --limit 20
aether-taskmanager --tui                  # 1 s refresh, ANSI table
aether-taskmanager --spike-s7             # sensor/GPU availability report
aether-taskmanager --export out.csv --format csv
aether-taskmanager --end-task 1234        # dry run; add --yes to signal
aether-taskmanager --efficiency 1234 on --yes
aether-taskmanager --startup-list
aether-taskmanager --startup-disable ~/.config/autostart/mail.desktop
aether-taskmanager --services
aether-taskmanager --energy-contract      # JSON binding for the Energy column
```

Integration flags: `--lifecycle-status` (lifecycled `--once` JSON) adds app
state and priority bands; `--power-history` (powerd `battery-history.json`)
adds the energy-impact column; `--battery-status` (powerd `status.json`) adds
the battery tab values.

## Metric sources (D10)

- `/proc/<pid>/stat`, `status`, `cmdline`, `io`, `fd/`, `smaps_rollup`, `cgroup`
- `/proc/stat`, `/proc/meminfo`, `/proc/uptime`, `/proc/net/dev`
- `/sys/block/*/stat`, `/sys/class/hwmon`, `/sys/class/thermal`,
  `/sys/class/drm`, `/sys/block/zram0`, cpufreq, battery sysfs
- `/proc/pressure/memory` (via `aether-lifecycled`)

CPU % is the delta between samples; expensive per-process memory detail is
opt-in (`ProcessSampler::detailed`), matching the F13 §6 overhead rules.

## Honest limits

- The collector is Linux-only; on macOS it needs a fixture tree (tests do
  this). `--spike-s7` on macOS correctly reports Fail rather than inventing
  sensors.
- G-TM1 (overhead) and G-TM2 (accuracy vs reference tools) need the reference
  machines; TM-AC-04/05/06 need real hardware.
- The Slint window, Ctrl+Shift+Esc shortcut, unsaved-work confirmation dialog
  and the "Not responding" force-quit flow still need the UI binding
  (S3 design system). The core exposes every command they call.
