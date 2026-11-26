# aether-taskmanager — Activity Manager core (P5-T07, F13)

The Slint window is a thin binding over this crate: every metric, action and
export used by the UI lives here and is tested headlessly. The window
(`ui/activity-manager.slint`, `src/window.rs`) uses the S3 design system (`ui/`)
and reads the Energy column from the contract — see Status.

## Status

| Feature | State |
|---|---|
| TM-01 Processes tab (columns, per-column sort) | Core + window done: name, PID, PPID, state, CPU %, PSS/RSS, disk, GPU (when reported), energy, wakeups, threads, open files, command, lifecycle state/band |
| TM-02 End task / end process tree | Core + dialog: SIGTERM, `--force` SIGKILL, refuses PID 1 and itself; `request_end_task` builds the confirmation dialog, and a hung app (flagged by the compositor's no-hang watchdog) gets the Force Quit wording; the window maps Cancel / End task / Force Quit to no signal / SIGTERM / SIGKILL |
| TM-03 Efficiency mode | Core done: lowers nice and `cpu.weight` of the app scope, restores on disable |
| TM-04/05/06/07/08 Performance tab | Core + window: five 60-second graphs (CPU, memory, disk, network, GPU) normalised per section, per-core bars, and CPU/memory/disk/network/GPU detail columns |
| TM-09 GPU detail | Busy % and VRAM where the driver reports them; per-process GPU is Pack D |
| TM-10 Sensors | Core + window: hwmon + thermal + fans sorted by name, 85 °C warning / 95 °C danger tones and alert lines, GPU engine/VRAM; explicit "Not reported" fallbacks instead of fake zeros |
| TM-11 Battery tab | Core + window: live `powerd` `PowerStatus`/sysfs values plus the F14 history (health trend sparkline, per-app energy for 24 h using the P5-T15 energy levels) |
| TM-12 App history | Core + window: per-app energy for the last 24 h and 7 days from the F14 store, with the P5-T15 impact level and share; CPU time and per-app network are not collected yet and the table says so |
| TM-13 Startup apps | Core + window: lists user + system autostart entries with an Enable/Disable row action that persists `Hidden=` in the `.desktop` file; startup impact reads "Not measured" instead of a guess |
| TM-14 Users | Core + window: `loginctl list-users` with seat and a "This session" marker |
| TM-16 Details | Core + window: PID/PPID/user/threads/open files/command plus the sandbox level derived from the cgroup (app scope vs system) |
| TM-17 Services | Core + window: `systemctl list-units --type=service` with load/active/sub state and a read-only note (start/stop/restart need the permission prompt) |
| TM-19 Export | CSV, JSON and text |
| TM-AC-01 shortcut | Contract + matcher: `Ctrl+Shift+Esc` (Windows profile) and `Super+Escape` (default); the window accepts both while focused and `--shortcut` publishes the binding for the shell to register globally |
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
`apply_energy_impact`, `EnergyImpact::from_share` and the window all use the
same thresholds through `EnergyColumnContract::level_for`; labels, tones, sort
direction and the null label come from `label_for`, `tone_for` and
`default_sort`.

### Window binding

`ActivityManagerWindow` (`src/window.rs`) owns the Slint component. It sets the
contract properties, sorts rows by energy (then CPU), applies the search filter
and maps each row to `ProcessRowView`. A different contract can be supplied with
`with_contract`/`set_contract`, which is how the binding test proves the UI has
no hard-coded labels.


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
aether-taskmanager --shortcut [--windows-profile]  # shortcut binding for the shell
aether-taskmanager --tab performance      # text view of one tab
                                          # (processes|performance|battery|sensors|app-history|startup-apps|users|details|services)
aether-taskmanager --render DIR           # draw every tab to DIR/activity-manager-<tab>.png
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
- The window covers Processes, Performance, Battery, Sensors and the five table
  tabs (App history, Startup apps, Users, Details, Services). The Android tab is
  hidden unless Pack A is installed and has no view yet.
- Services is read-only in the window: start/stop/restart need the permission
  prompt (TM-17) and are not wired.
- The window accepts the shortcut only while focused. Registering it globally
  is the compositor/shell's job; `--shortcut` prints the exact binding, and the
  hung flag for the Force Quit wording must come from the compositor's no-hang
  watchdog (P3-T08), not guessed.
- `tests/ui.rs` pins the contract binding of the Processes tab; `tests/tabs.rs`
  pins the tab models, the shortcut hint and the dialog-to-signal mapping;
  `tests/tables.rs` pins the five table tabs and the startup row action. The
  screenshot path is `--render` (one PNG per tab).
