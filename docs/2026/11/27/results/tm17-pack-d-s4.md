# TM-17 service actions + Pack D data contracts — S4 (2026-11-27, round 3)

Session S4 (`session - 4 (Nov27)`), third hardware-independent round.
Follows `p5-t07-tables-p6-t04-seccomp-s4.md` (commit `aac4b5c`).

1. **TM-17**: Start/Stop/Restart for systemd units behind the permission-prompt
   contract — plan, prompt state machine, executor seam, window buttons, tests.
2. **Pack D**: per-app network (kernel socket accounting contract) and
   per-process GPU detail (DRM fdinfo) sampling/aggregation model, plus the App
   history CPU-time and network columns wired to that contract.
3. Docs: F13, F15, D10 (§5.2, §5.3, §9), taskmanager README.

No hardware gate is claimed. The Dell hardware validation is still paused.

## 1. TM-17 — service actions and the permission prompt

### What changed

| File | Change |
|---|---|
| `apps/taskmanager/src/service_actions.rs` (new) | `ServiceAction` (`start`/`stop`/`restart`), `ServiceActionContract` (permission `admin.authorize`, Cancel label, protected units, three action specs with message/confirm label/tone), `ServiceActionPlanner` (unit validation: `.service` only, systemd characters, protected units refused with the reason), `ServiceActionPlan` (`systemctl <verb> <unit>` + `ServicePrompt` title/message/detail), `ServiceActionPrompt` state machine (`Idle`/`Pending`; confirm runs the plan once, cancel and stray confirmations return `NothingPending`), `ServiceExecutor` trait + `PlanningExecutor`, `action_id`/`parse_action_id` |
| `apps/taskmanager/src/tabs.rs` | `TableRow` now carries `actions: Vec<RowAction>` (multiple buttons per row, used by Services). `services_view`/`services_view_with` add Stop+Restart to running units and Start+Restart to the rest, ids `service:<action>:<unit>`; the note names the permission prompt |
| `apps/taskmanager/ui/activity-manager.slint` | `RowActionView { id, label }`; the shared `DataTable` renders one button per row action instead of a single hard-coded button |
| `apps/taskmanager/src/window.rs` | `bind_table` maps every row action into the Slint model |
| `apps/taskmanager/src/main.rs` | `--service-actions` (contract JSON) and `--service-action UNIT start|stop|restart` (plan + prompt, `SERVICE_ACTION_PLAN_OK` / `SERVICE_ACTION_REFUSED`, never spawns a process) |
| `apps/taskmanager/tests/tables.rs` | Services row actions bound to the row and the action id maps back through `on_row_action` |
| `os-build/Makefile` | `make service-actions` |

### Behaviour pinned by tests

- A valid request produces `systemctl restart aether-powerd.service`, the
  title `Restart “aether-powerd.service”?`, the confirm label `Restart service`
  and the prompt detail with the permission name.
- Empty names, non-`.service` names, names with shell/path characters and
  protected units are refused **before** any prompt exists.
- Confirming with no prompt, cancelling, and confirming twice never call the
  executor; a confirmed plan calls it exactly once with that plan; executor
  errors become `Failed { unit, action, reason }`.
- `service:restart:<unit>` round-trips for instanced units
  (`getty@tty1.service`).

### Commands and results

```
$ cargo run -q -p aether-taskmanager -- --service-action aether-powerd.service restart
Restart “aether-powerd.service”?
Restart “aether-powerd.service”? It stops and starts again, so anything using it
reconnects. Administrator permission is required.
systemctl restart aether-powerd.service — An administrator action: the prompt shows
the exact command and the answer is never remembered. (admin.authorize)
command: systemctl restart aether-powerd.service
No command is run: the plan waits for the permission prompt.
SERVICE_ACTION_PLAN_OK
exit=0

$ cargo run -q -p aether-taskmanager -- --service-action aether-lifecycled.service stop
refused: “aether-lifecycled.service” is protected: the lifecycle daemon suspends and
kills every managed app
SERVICE_ACTION_REFUSED
exit=1

$ cargo run -q -p aether-taskmanager -- --service-action aether-powerd.socket stop
refused: only .service units can be started, stopped or restarted
SERVICE_ACTION_REFUSED
exit=1
```

## 2. Pack D — data contracts

### What changed

| File | Change |
|---|---|
| `apps/taskmanager/src/pack_d.rs` (new) | `NetworkAccounting` + strict `parse_network_snapshot` (`aether-net-accounting v1`, `<app> <rx> <tx>`; missing header/number/duplicate is an error, absent path = `NotInstalled`); `AppNetworkHistory` (delta rates, hourly buckets, 8-day retention, counter-reset handling, session totals); `AppCpuTime` (integrates `cpu_percent/100 × elapsed` per app scope into the same buckets); `parse_fdinfo` (Intel `render`/`video`/`copy` and AMD `gfx`/`compute`, `drm-memory-*` units, `drm-pdev`, `us`/`ms` fallbacks); `GpuDetailSampler` (DRM fds only, `Δns/Δwall` busy share clamped 0..100, region max merge across fds); `apply_gpu_detail`/`apply_network_totals`; `PackDContract` published by `--pack-d-contract` |
| `apps/taskmanager/src/tabs.rs` | `app_history_view(analytics, cpu, network, now)` merges energy + Pack D into one table with `CPU time` and `Network` columns; apps seen only by Pack D are listed; absent sources keep the contract null label; summary marks "CPU/network from Pack D" |
| `apps/taskmanager/src/window.rs` | `demo_pack_d()` builds the render history by running the real integrators hourly for seven days |
| `apps/taskmanager/src/main.rs` | `--pack-d-contract`; App history render binds the Pack D columns |
| `os-build/Makefile`, `.github/workflows/ci.yml` | `make pack-d-contract`; CI guards for both new contracts and the refused service action |

### Numbers produced by the demo render (real integrators, demo inputs)

`--render` on the macOS dev host (demo data, documented as such):

| App | CPU time (7 d) | Network (7 d) |
|---|---|---|
| org.aether.Browser | 103 h 32 m | 1.5 GB in · 374.3 MB out |
| org.aether.Player | 30 h 03 m | 191.1 MB in · 47.8 MB out |
| org.aether.Files | 5 h 00 m | 19.1 MB in · 4.8 MB out |

The CPU column equals `Σ cpu_percent/100 × 1 h` for 167 hours at 62% (103.2 h)
and the network column equals 167 × the hourly counter delta, which is what the
tests assert on small fixtures.

### Acceptance-relevant honesty rules

- Missing Pack D → `Not reported` in both columns and a note naming the pack;
  no zeros.
- First sample primes counters (0 B/s, because no interval was measured).
- A counter that goes backwards reports no delta for that interval.
- A process without DRM fdinfo keeps `gpu_percent = None`.
- `hardware_gated` in the contract lists: eBPF attach + map dump, DRM fdinfo
  validation on the reference Intel/AMD GPUs, G-TM1 overhead with Pack D.

## 3. Tests and verification

```
$ cargo test -p aether-taskmanager -p aether-lifecycled
lifecycled: 63 lib + 2 integration  = 65 passed
taskmanager: 67 lib + 3 integration = 70 passed
total 135 passed, 0 failed
  (new: service_actions 7, pack_d 10; updated: tabs App history/Services,
   tables.rs row actions)

$ cargo clippy -p aether-taskmanager --all-targets
1 new lint fixed (large_enum_variant annotated with rationale;
cloned_ref_to_slice_refs fixed). No new warnings in the new modules.

$ cargo run -q -p aether-taskmanager -- --render docs/2026/11/27/results/screenshots
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-app-history.png (1080x640)
AETHER_ACTIVITY_MANAGER_RENDERED .../activity-manager-services.png (1080x640)
  (+ the other seven tabs still render)

$ cargo run -q -p aether-taskmanager -- --tab app-history
APP HISTORY
  App  24 hours  7 days  CPU time  Network  Impact  Share
  No battery history: pass --power-history (powerd battery-history.json).
  Energy is measured by powerd (F14). Per-app CPU time and network need Pack D
  (advanced monitoring) and read "Not reported" until it is installed.
```

### Screenshots (demo data; documented as such)

- `screenshots/activity-manager-app-history.png` — three apps with 24 h / 7-day
  energy, CPU time, network in/out, impact level and share, plus the Pack D note.
- `screenshots/activity-manager-services.png` — five units; running units show
  Stop + Restart, inactive/failed units show Start + Restart, and the note names
  the administrator permission prompt.

## 4. Hardware-gated and still pending

- The systemd D-Bus/polkit `ServiceExecutor` and real start/stop/restart runs
  (plus the polkit policy file) on AetherOS.
- Attaching the Pack D eBPF cgroup/socket program and dumping its map to
  `/run/aether/pack-d/net-accounting`.
- DRM fdinfo validation and G-OPT2/G-TM1/G-TM2 measurements on the reference
  Intel/AMD machines.
- TM-AC-01 (500 ms open) remains a shell/compositor task.
