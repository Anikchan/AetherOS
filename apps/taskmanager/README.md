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
| TM-12 App history | Core + window: per-app energy for the last 24 h and 7 days from the F14 store, with the P5-T15 impact level and share, plus the Pack D CPU-time and network columns (`AppCpuTime`, `AppNetworkHistory`); without Pack D those two columns read "Not reported" |
| TM-13 Startup apps | Core + window: lists user + system autostart entries with an Enable/Disable row action that persists `Hidden=` in the `.desktop` file; startup impact reads "Not measured" instead of a guess |
| TM-14 Users | Core + window: `loginctl list-users` with seat and a "This session" marker |
| TM-16 Details | Core + window: PID/PPID/user/threads/open files/command plus the sandbox level derived from the cgroup (app scope vs system) |
| TM-17 Services | Core + window: `systemctl list-units --type=service` with load/active/sub state; every row offers the prompted Start/Stop/Restart actions (`ServiceActionPrompt`) that build the `systemctl` plan and require confirmation before an executor is called. Executing the plan is hardware-gated |
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

## Service actions behind the permission prompt (TM-17)

`aether-taskmanager --service-actions` prints the `ServiceActionContract`: the
`admin.authorize` permission, the Cancel label, the protected units and the
Start/Stop/Restart specs (label, confirm label, tone, message). The Services tab
never builds a `systemctl` call on its own. A row action id
(`service:<action>:<unit>`) goes to `ServiceActionPrompt::request`, which:

1. validates the unit (`.service` only, systemd characters, protected units
   refused with the reason),
2. builds the `ServiceActionPlan` with the exact command
   (`systemctl <verb> <unit>`) and the prompt text (title, message, command +
   permission, confirm/cancel labels),
3. keeps the plan as `Pending` until `confirm(&executor)` or `cancel()`.

`confirm` hands the pending plan to a `ServiceExecutor` exactly once; `cancel`,
a second `confirm`, or a stray button press returns `NothingPending` and runs
nothing.

The executor is `SystemdDbusExecutor`: it builds the systemd manager call from
the plan

```
busctl call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
    org.freedesktop.systemd1.Manager StartUnit|StopUnit|RestartUnit \
    ss <unit> replace
```

and has two forms — `dry_run()` records the vector, `system()` starts the
process. Only an explicit confirm reaches `system()`:
`aether-taskmanager --service-action UNIT start|stop|restart --yes`. Without
`--yes` the CLI prints the plan plus the bus call and stops
(`SERVICE_ACTION_PLAN_OK`); a refusal prints `SERVICE_ACTION_REFUSED`; a failed
call prints `SERVICE_ACTION_FAILED` with the reason.

The polkit policy lives in `polkit/org.aether.taskmanager.service-action.policy`
(action `org.aether.taskmanager.service-action`, `auth_admin` on
any/inactive/active, no retained answers). It is generated by
`polkit_policy_xml(&contract)` (`aether-taskmanager --polkit-policy` prints it)
and is compiled into the binary with `include_str!`, so a test fails if the
shipped file and the contract drift. Installing it to
`/usr/share/polkit-1/actions/` and the real systemd call are hardware-gated.

## Pack D data contracts (TM-09/TM-12/TM-15)

`aether-taskmanager --pack-d-contract` publishes the advanced-monitoring
contract: 2 s refresh while visible, 30 s otherwise, ≤ 0.5% of one core
(D10 §3), 8-day history retention, the network snapshot path and header, and
the GPU fdinfo source.

- **Per-app network** (`AppNetworkHistory`): the Pack D loader dumps its
  cgroup/socket accounting map to `/run/aether/pack-d/net-accounting`
  (`aether-net-accounting v1`, one `<app> <rx_bytes> <tx_bytes>` line each).
  `parse_network_snapshot` is strict (header, numbers, no duplicate apps);
  `NetworkAccounting::snapshot` reports `NotInstalled` when the path is absent.
  Rates come from counter deltas between samples, hourly buckets feed the 7-day
  totals, and a counter that goes backwards (app restart) reports no delta.
- **Per-app CPU time** (`AppCpuTime`): integrates `cpu_percent / 100 × elapsed`
  from the 1 s process samples for rows inside an app scope, in hourly buckets.
- **Per-process GPU detail** (`GpuDetailSampler`): reads `drm-engine-*`,
  `drm-memory-*` and `drm-pdev` from `/proc/<pid>/fdinfo/<fd>` for DRM fds and
  turns counter deltas into engine busy shares; `apply_gpu_detail` fills
  `ProcessRow::gpu_percent` with the highest engine share.
- **The recorders run in the daemons.** `aether-powerd` owns
  `powerd::pack_d::NetworkRecorder` (samples on every tick, persists
  `pack-d-network.json`) and `aether-lifecycled` owns
  `lifecycled::pack_d::GpuRecorder` (samples every 30 s, persists
  `pack-d-gpu.json`), so the hourly buckets fill whether or not this window is
  open. `src/pack_d.rs` re-exports those contracts, keeps the CPU-time
  integrator and the row mapping, and publishes the wiring in the contract's
  `recorders` field. `--pack-d-history <file>` loads the daemon's network
  buckets into the App history tab.
- The eBPF program attach, the map dump and DRM validation on the reference
  Intel/AMD GPUs are hardware-gated; the contract lists them in
  `hardware_gated`.

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
aether-taskmanager --service-actions      # TM-17 prompt contract
aether-taskmanager --service-action aether-powerd.service restart  # plan only
aether-taskmanager --service-action aether-powerd.service restart --yes  # confirmed (needs systemd + busctl)
aether-taskmanager --polkit-policy        # polkit action file generated from the contract
aether-taskmanager --pack-d-contract      # Pack D monitoring contract
aether-taskmanager --pack-d-history ~/.local/state/aether/powerd/pack-d-network.json --tab app-history
aether-taskmanager --shortcut [--windows-profile]  # shortcut binding for the shell
aether-taskmanager --tab performance      # text view of one tab
                                          # (processes|performance|battery|sensors|app-history|startup-apps|users|details|services)
aether-taskmanager --render DIR           # draw every tab to DIR/activity-manager-<tab>.png
```

Integration flags: `--lifecycle-status` (lifecycled `--once` JSON) adds app
state and priority bands; `--power-history` (powerd `battery-history.json`)
adds the energy-impact column and the App history energy columns;
`--battery-status` (powerd `status.json`) adds the battery tab values;
`--pack-d-history` (powerd `pack-d-network.json`) adds the App history network
column from the daemon's hourly buckets.

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
- Services actions plan and prompt only: the systemd D-Bus/polkit executor and
  the real start/stop/restart runs need AetherOS hardware. The note in the tab
  says so, and `--service-action` prints the plan without running anything.
- Pack D needs the eBPF accounting loader, the map dump to
  `/run/aether/pack-d/net-accounting`, and GPU fdinfo validation on the
  reference machines before its numbers can be claimed; the aggregation and
  parsers are tested against fixture trees and macOS has no `/proc`.
- The window accepts the shortcut only while focused. Registering it globally
  is the compositor/shell's job; `--shortcut` prints the exact binding, and the
  hung flag for the Force Quit wording must come from the compositor's no-hang
  watchdog (P3-T08), not guessed.
- `tests/ui.rs` pins the contract binding of the Processes tab; `tests/tabs.rs`
  pins the tab models, the shortcut hint and the dialog-to-signal mapping;
  `tests/tables.rs` pins the five table tabs and the startup row action. The
  screenshot path is `--render` (one PNG per tab).
