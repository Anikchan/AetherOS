# D10 — Monitoring Design (Activity Manager, Battery History, Screen Time)

Contracts first (section 5), then sources, sampling, storage, and overhead rules. Feature requirements: F13, F14. Battery policy: D5. Gates: section 8 of this document and the addendum 2 README.

## 1. Principles

1. **Read, do not poll more than needed.** Sampling rate depends on whether Activity Manager is open.
2. **Read cheap sources first.** Expensive reads (per-process memory maps) only for visible rows.
3. **Keep history small.** Roll up old data. Store only what is shown.
4. **Never show zero for missing data.** Missing sources are shown as "not reported".
5. **Local only.** No data leaves the machine.

## 2. Sources

| Metric | Source | Cost | Notes |
| --- | --- | --- | --- |
| Overall CPU | `/proc/stat` | Very low | Delta between samples |
| Per-core CPU and clock | `/proc/stat`; `/sys/devices/system/cpu/cpu*/cpufreq/` | Very low | Clock from cpufreq |
| CPU energy preference | `/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference` | Very low | Shown in CPU detail |
| Process CPU | `/proc/<pid>/stat` | Low (per process) | Delta between samples |
| Process memory (cheap) | cgroup `memory.current` for each app scope | Very low | Used for background apps |
| Process memory (private, detail) | `/proc/<pid>/smaps_rollup` | Medium | Only for visible rows |
| System memory | `/proc/meminfo` | Very low | Used, available, cached |
| zram and compressed memory | `/sys/block/zram*/mm_stat` | Very low | Compressed and original sizes |
| Memory pressure | `/proc/pressure/memory` (PSI) | Very low | Also used for event waits |
| Disk (system) | `/proc/diskstats` | Very low | Read and write rates; active time |
| Disk (process) | `/proc/<pid>/io` | Low | Bytes read and written |
| Disk health and SMART temperature | Kernel NVMe and SATA interfaces (hwmon) | Low | Read every 60 s |
| Network (interfaces) | `/proc/net/dev` | Very low | Core |
| Network (per app) | Kernel socket accounting through an eBPF program attached to app cgroups | Low to medium | Pack D only; the loader dumps its map to `/run/aether/pack-d/net-accounting` (see 5.2) |
| GPU engine use (system) | Driver statistics: `gpu_busy_percent` (AMD); performance counters (Intel) | Low | Where available |
| GPU per process | DRM usage fields in `/proc/<pid>/fdinfo/<fd>` (engine time and memory keys) | Low to medium | Pack D for detail; basic per-app value in core when cheap |
| Temperatures | hwmon `temp*_input`; thermal zones in `/sys/class/thermal/` | Very low | Labels from hwmon `temp*_label` |
| Fan speeds | hwmon `fan*_input` | Very low | Where present |
| Battery | `/sys/class/power_supply/BAT*/` (energy, power, capacity, status, cycle_count, energy_full, energy_full_design, temp); UPower | Very low | Through D5 telemetry |
| Screen state | Compositor output power state; backlight in sysfs | Very low | Event-driven |
| Focus | Compositor focus events (D3) | Event-driven | Screen Time and App history |
| Services and sessions | systemd D-Bus (units); logind (sessions) | Low | On tab open only |
| Startup items | Desktop autostart folders and user service list | Very low | On tab open only |

Notes:
- Battery cycle count is read from `cycle_count` when present. Many laptops do not report it; the battery view then says so.
- Some GPU and sensor values depend on the driver. Missing values are shown as "not reported".

## 3. Sampling Rates

| Mode | Metrics | Rate | Target overhead |
| --- | --- | --- | --- |
| Activity Manager visible | CPU, memory, disk, network, GPU (basic), sensors, process list | 1 s | ≤ 1% of one core; ≤ 40 MB memory |
| Activity Manager visible, Details tab | Adds per-process memory detail for visible rows | 2 s | ≤ 1% of one core |
| Activity Manager closed | Battery, sensors, app energy and focus (for history) | 30 s | ≤ 0.2% of one core average; ≤ 10 MB |
| Battery health | Full capacity and cycles | Once per day, plus at each session end | Negligible |
| Screen state and focus | Event-driven | On event | Negligible |
| Pack D (advanced) | Per-app network, per-process GPU detail | 2 s when visible; 30 s otherwise | ≤ 0.5% of one core |

Sampling jitter: each sampler waits its interval plus a random offset (within 10%) so that all samplers do not wake the CPU at the same moment. This reduces wakeups (battery).

## 4. Process Data Path

1. The sampler reads the process list once per interval (cheap: names and IDs, cached).
2. CPU and disk values are deltas from the previous sample.
3. Memory for each row comes from the app's cgroup when the row is an app (cheap). For other rows, detail is read only when the row is visible.
4. GPU per process comes from `fdinfo` for open DRM file descriptors of that process. Reads are limited to processes that hold a DRM file descriptor (the list is cached per interval).
5. Results are sent to the Activity Manager view and to the history store (only the values in section 5 are stored).

## 5. Contracts (Rust)

```rust
pub enum MetricId {
    CpuTotal, CpuCore(u16), CpuClockMhz(u16), MemUsed, MemAvailable, MemCompressed,
    MemPressure, DiskRead, DiskWrite, NetRx(String), NetTx(String),
    GpuEngine(String), GpuMemory, TempSensor(String), FanRpm(String),
    BatteryPercent, BatteryPowerW, BatteryTempC, BatteryHealth, BatteryCycles,
    AppCpu(String), AppMemory(String), AppEnergy(String), AppGpu(String),
    AppNetRx(String), AppNetTx(String), ScreenOn, FocusedApp,
}

pub struct MetricPoint { pub id: MetricId, pub at_unix_s: u64, pub value: f64 }

pub enum Availability { Reported(f64), NotReported, Unsupported }

pub trait Sampler {
    fn id(&self) -> &'static str;
    fn sample(&mut self) -> Vec<(MetricId, Availability)>;
    fn interval(&self, visible: bool) -> std::time::Duration;
}

pub trait MetricStore {
    fn append(&mut self, points: &[MetricPoint]) -> std::io::Result<()>;
    fn query(&self, id: &MetricId, from_unix_s: u64, to_unix_s: u64) -> std::io::Result<Vec<MetricPoint>>;
    fn rollup(&mut self, older_than_unix_s: u64, resolution: std::time::Duration) -> std::io::Result<()>;
}

pub trait ProcessSource {
    fn list(&self) -> std::io::Result<Vec<ProcessRow>>;
    fn detail(&self, pid: u32) -> std::io::Result<ProcessDetail>;
}
```

Notes:
- `Availability::NotReported` is the value shown when a sensor is missing. It is never stored as zero.
- `ProcessRow` and `ProcessDetail` hold the Details tab fields (F13 TM-16).

### 5.1 Energy-impact column binding (P5-T15, 2026-11-26)

`apps/taskmanager` is the single owner of the column contract; the Slint window
reads it through `aether-taskmanager --energy-contract` instead of hard-coding
labels or thresholds:

| Field | Value |
| --- | --- |
| key / header | `energy` / `Energy` |
| default sort | descending |
| null label | `Not reported` |
| refresh | 60 s (section 3 roll-up) |
| levels | High ≥ 66 % of the top consumer, Medium ≥ 33 %, Low > 0 Wh, None |
| tones | `danger`, `accent`, `success`, `text_muted` (D1 tokens) |
| source 1 | `powerd battery-history.json → energy_by_app` (measured) |
| source 2 | `EnergyImpact::classify(cpu, wakeups, gpu)` (relative score) |

The same thresholds are used by `apply_energy_impact` and
`EnergyImpact::from_share`, so the CLI, export and window cannot disagree.

The window binding lives in `apps/taskmanager/src/window.rs`
(`ActivityManagerWindow`, `ui/activity-manager.slint`, 2026-11-26):

- The Slint component takes the header, null label, tooltip and sort direction
  as properties set from the contract; level labels and tones come from
  `level_spec`/`label_for`/`tone_for`. Nothing about the column is hard-coded in
  Slint.
- `apply_energy_impact_with` takes an explicit contract, so raising a threshold
  in the contract reclassifies rows in the daemon, the CLI, the export and the
  window together.
- `aether-taskmanager --render DIR` draws the window headlessly
  (`activity-manager.png`) through the shared `aether-ui` renderer. The binding
  is covered by `apps/taskmanager/tests/ui.rs`, which asserts that a changed
  contract changes the rendered header, labels and order.

### 5.2 Pack D column contracts (P7-PK-T01, TM-09/TM-12/TM-15; 2026-11-27)

`apps/taskmanager/src/pack_d.rs` owns the Pack D data model and
`aether-taskmanager --pack-d-contract` publishes it:

| Field | Value |
| --- | --- |
| key / pack | `pack-d` / `D. Advanced monitoring` |
| refresh | 2 s while the Activity Manager is visible, 30 s otherwise |
| overhead target | ≤ 0.5% of one core (section 3) |
| history | 8 days of hourly buckets (7-day App history window plus one hour) |
| network source | kernel socket accounting attached to app cgroups |
| network snapshot | `/run/aether/pack-d/net-accounting`, header `aether-net-accounting v1`, lines `<app> <rx_bytes> <tx_bytes>` |
| network null label | `Not reported` |
| GPU source | `drm-engine-*`, `drm-memory-*`, `drm-pdev` in `/proc/<pid>/fdinfo/<fd>` |
| GPU engines | `render`, `compute`, `video`, `video-enhance`, `copy`, `gfx` |
| GPU memory regions | `vram`, `gtt`, `system`, `stolen` |

Rules the implementation enforces:

- **The recorders live in the daemons (2026-11-27).** `aether-powerd` samples
  the accounting snapshot on every tick (`NetworkRecorder`, 15 s) and
  `aether-lifecycled` samples DRM fdinfo every 30 s (`GpuRecorder`), so the
  hourly buckets fill whether or not the Activity Manager is open. Each recorder
  writes its buckets next to the daemon state (`pack-d-network.json`,
  `pack-d-gpu.json`, 8-day retention) and reloads them on start;
  `aether-taskmanager --pack-d-history <file>` reads the network file into the
  App history window. `apps/taskmanager/src/pack_d.rs` now re-exports the
  daemon contracts, owns only the CPU-time integrator and the row mapping, and
  publishes the `recorders` wiring in the Pack D contract.
- **Snapshot parsing is strict.** A missing header, a non-numeric counter, a
  short line or a duplicate app is an error, not a zero. An absent snapshot
  means `NotInstalled`; an unreadable or malformed one means `NotLoaded`, and
  the tab prints the reason.
- **Rates are deltas.** The first sample only primes the counters, because no
  interval has been measured. A counter that goes backwards (app restarted)
  reports no delta for that interval instead of a negative or a spike.
- **Windowed totals are hourly buckets.** `AppNetworkHistory::totals_for` and
  `AppCpuTime::total_seconds_for` sum buckets since a timestamp, pruned after
  eight days. CPU seconds are `cpu_percent / 100 × elapsed` for rows that carry
  an `aether-app-*` scope; system processes are not Pack D data.
- **GPU values are deltas too.** Engine busy share is
  `Δns / Δwall × 100` clamped to 0..100, and a process's row value is its
  highest engine share. Memory regions are merged across a process's DRM fds by
  maximum so the same client is not counted twice. Only processes whose
  `/proc/<pid>/cgroup` names an `aether-app-*` scope are attributed; everything
  else is system work and is skipped, not counted as zero.
- **D10 section 7 holds.** No Pack D sampler runs when the pack is not
  installed; the window shows `Not reported` and the contract's
  `hardware_gated` list names what still needs hardware.

App history (TM-12) merges three sources per app: measured energy from F14,
`AppCpuTime` (CPU time column) and `AppNetworkHistory` (network column). Apps
that only Pack D saw are still listed, with the energy columns showing the
contract's null label. The column order is pinned by
`apps/taskmanager/tests/tables.rs`: App, 24 hours, 7 days, CPU time, Network,
Impact, Share.

### 5.3 Service-action permission contract (TM-17; 2026-11-27)

The Services tab never runs `systemctl` directly. `ServiceActionContract`
(`apps/taskmanager/src/service_actions.rs`, printed by
`aether-taskmanager --service-actions`) defines:

| Field | Value |
| --- | --- |
| key / header | `service-action` / `Services` |
| permission | `admin.authorize` |
| polkit action | `org.aether.taskmanager.service-action` |
| polkit policy | `/usr/share/polkit-1/actions/org.aether.taskmanager.service-action.policy` (source: `apps/taskmanager/polkit/`) |
| actions | `start`, `stop`, `restart` with label, confirm label, tone and message |
| cancel label | `Cancel` |
| protected units | `aether-lifecycled.service` (stopping it suspends or kills every managed app) |
| command | `systemctl <verb> <unit>` (what the prompt shows) |
| D-Bus call | `busctl call org.freedesktop.systemd1 /org/freedesktop/systemd1 org.freedesktop.systemd1.Manager StartUnit|StopUnit|RestartUnit ss <unit> replace` |
| remembered answers | none — every action prompts again |

Flow: a row action id `service:<action>:<unit>` →
`ServiceActionPrompt::request` validates the unit (`.service` only, systemd
characters, protected units refused with the reason), builds a
`ServiceActionPlan` (command + prompt title/message/detail) and stores it as
`Pending`. `confirm(executor)` moves the plan to a `ServiceExecutor` exactly
once; `cancel()`, a second confirm or a stray button press returns
`NothingPending` and runs nothing.

Executor: `SystemdDbusExecutor` builds the `busctl` argument vector from the
plan and has two forms — `dry_run()` records the vector, `system()` starts the
process. Only `aether-taskmanager --service-action UNIT start|stop|restart
--yes` uses `system()`; without `--yes` the CLI prints the plan and
`SERVICE_ACTION_PLAN_OK`. The policy file is generated by
`polkit_policy_xml(&contract)` and is compiled into the binary with
`include_str!`, so a test fails if the shipped file and the contract drift
apart (`aether-taskmanager --polkit-policy` prints it). Every default is
`auth_admin` with no retention.

Hardware-gated: the real invocation needs systemd, polkit and `busctl` on
AetherOS; on this host the confirmed path reports
`cannot run busctl: No such file or directory` and `SERVICE_ACTION_FAILED`. The
app-level action is what the prompt names; systemd authorizes the D-Bus call
itself with its `org.freedesktop.systemd1.manage-units` right, which the OS
ships.


## 6. Storage

| Store | Format | Resolution and retention | Size estimate |
| --- | --- | --- | --- |
| Live buffer | Memory ring | 60 seconds at 1 s | 1 MB |
| Minute history | Binary append-only file | 1 minute for 30 days | About 40 MB per year before rollup |
| Rollup 15 minutes | Binary file | 2 years | About 5 MB per year |
| Battery daily | Small table | 5 years | Under 100 KB per year |
| Sessions | Table | 2 years | Under 1 MB per year |
| Screen time and focus | Per app per day table | 2 years | Under 5 MB per year |

Location: `~/.local/state/nexa/metrics/` (user) and `/var/log/nexa/metrics/` (system values only). Files are written in batches (every 60 s) to reduce disk writes. Writes are asynchronous; a crash loses at most one minute of data.

Rollup: a background job runs once per hour, at low priority, and converts old minute data to 15-minute averages (and maxima and minima where useful).

## 7. Overhead Rules

- No sampler runs more often than its interval, even when the view requests it.
- A sampler that takes longer than its interval is lengthened, and the view shows "slow sampling" in the status line.
- Disk writes are batched. No write for a metric that did not change.
- Pack D samplers are not installed, not loaded, and not running when the pack is not installed.

## 8. Gates

| Gate | Pass value | Method |
| --- | --- | --- |
| G-TM1 Activity Manager overhead | Open and visible: ≤ 1% of one core and ≤ 40 MB; closed: ≤ 0.2% of one core average and ≤ 10 MB | Measure with `top` or `pidstat` for 10 minutes each |
| G-TM2 Accuracy | Each value within 5% of the reference tool (`top`, `sensors`, `iostat`, `ip`) | Side-by-side sampling for 5 minutes |
| G-BAT4 Cycle count | Matches the system battery report when reported | Compare with the reported value |
| G-BAT5 History continuity | At least 99% of minutes recorded over 7 days while the machine is on | Count of stored minutes against uptime |
| G-ST1 Screen time | Matches the compositor focus log within 1 minute per day | Compare totals for 3 test days |
| G-OPT1 Core idle | Covered by F6 RES-03 and RES-04 with all core samplers running | 5-minute idle measurement |

## 9. Tests

| Test | Type | Pass criterion |
| --- | --- | --- |
| Sampler intervals | Unit | Each sampler runs at its stated rate, with jitter within 10% |
| Missing sensor | Unit | Shows "not reported"; never stores zero |
| Rollup | Unit | Totals match before and after rollup |
| Store crash | Integration | At most one minute lost after a forced stop |
| Overhead | System | G-TM1 passes |
| Export | Integration | CSV and text reports match the store |
| Pack D snapshot parser | Unit | Strict header/counter/duplicate checks; missing snapshot is `NotInstalled`, never zero |
| Pack D aggregation | Unit | First sample primes, deltas become rates, counter resets report nothing, hourly totals expire after 8 days |
| GPU fdinfo | Unit | Intel (`render`/`video`/`copy`) and AMD (`gfx`/`compute`) keys parse; busy share is `Δns / Δwall`; rows without DRM fds stay unreported |
| Pack D recorder loops | Unit | `aether-powerd` fills the network buckets on `tick` and persists them; `aether-lifecycled` fills the GPU buckets on `sample_pack_d` and persists them; a restarted recorder keeps its buckets and primes again |
| Pack D GPU attribution | Unit | Processes with an `aether-app-*` cgroup are bucketed per app; a system process with a DRM fd is skipped, not counted |
| Pack D history file | Unit | `--pack-d-history` bucket file renders the App history network cell; an unknown version is refused |
| Service plan and prompt | Unit | No plan runs without a confirm; cancel drops it; a confirmed plan reaches the executor once; protected units and non-`.service` names are refused |
| Service executor | Unit | The confirmed plan becomes `busctl call … StartUnit/StopUnit/RestartUnit ss <unit> replace`; nothing is spawned before the confirm |
| Polkit policy | Unit | The shipped policy file equals `polkit_policy_xml(&contract)`; one action id, `auth_admin`, no retained answers |
| Contract pinning | Unit | Energy, service-action and Pack D contracts keep their published keys, permissions, paths, refresh rates and null labels |
