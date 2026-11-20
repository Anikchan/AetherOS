# D3 — Process Manager Design (`aether-lifecycled`)

This document defines the contracts first (section 4), then the behavior (sections 3, 6, and 7). Feature requirements are in F2. Benchmarks are in 04 section 4.

## 1. Purpose

Keep many apps open while inactive apps use almost no memory and no CPU. Protect the foreground app. End background apps in a fixed order under memory pressure.

## 2. Mechanisms Used (Linux Kernel)

| Mechanism | Kernel feature | Use |
| --- | --- | --- |
| cgroup v2 scopes | cgroup v2 | One scope per app; limits and accounting |
| `cgroup.freeze` | Kernel 5.2 and newer | Suspend an app (all its processes stop) |
| `memory.reclaim` | Kernel 5.19 and newer | Proactively reclaim memory from a cgroup (hibernation) |
| `memory.high` and `memory.low` | cgroup v2 | Soft ceiling for background apps; protection for the foreground app |
| `cpu.weight` | cgroup v2 | Lower CPU share for background apps |
| zram | Kernel block device | Compressed swap in RAM |
| `vm.swappiness` | Sysctl | Raised for zram so anonymous pages are compressed early (values above 100 require kernel 5.8 and newer) |
| PSI | `/proc/pressure/memory` | Memory pressure signal (`some` and `full` averages) |
| Poll on PSI triggers | `poll()` on pressure files | Event-driven pressure notifications with no polling loop |

Kernel minimum: 5.19 (required for `memory.reclaim`). Target: LTS 6.x.

Implementation note (spike S3, P0-T09, 2026-11-18): `cgroup.freeze` and
`memory.reclaim` were verified Pass on the test kernel (~40 MB reclaimed on
demand). PSI (`/proc/pressure`) was **absent** from the Alpine virt kernel
even with `psi=1` on the command line — the kernel was built without
`CONFIG_PSI`. The P1-T01 kernel fragment therefore sets `CONFIG_PSI=y`, and
the PSI check is re-run there. Until that is confirmed, pressure-driven
features (section 6) must treat a missing `/proc/pressure/memory` as
"pressure unavailable" and fall back to reclaim-only behavior rather than
crashing.

## 3. States and Transition Rules

| From | To | Condition |
| --- | --- | --- |
| Foreground | Visible | Focus moves to another window, but this app's window is still on screen |
| Foreground | Background | Window is hidden or minimized |
| Visible | Foreground | Focus moves to this app |
| Visible | Background | Window is hidden |
| Background | Foreground | Window is focused |
| Background | Suspended | Grace period elapsed (default 30 s on AC, 10 s on battery) and no protection reason |
| Suspended | Foreground or Visible | User focuses or shows the window (thaw is immediate) |
| Suspended | Hibernated | Suspended for hibernation timeout (default 10 min), or memory pressure reaches Elevated (section 6) |
| Hibernated | Foreground or Visible | User focuses or shows the window (thaw, then page-in on demand) |
| Any non-foreground | Terminated | Killer selects it (section 6) |
| Any | Background | Protection reason added while suspended: thaw and return to Background |

Protection reasons (`ProtectReason`) prevent suspension (freezing). A protected app stays in Background: its processes keep running, and its memory can still be reclaimed under pressure. The exception is Audio and Call, where reclaim is also blocked so playback is not interrupted.

| Reason | Source | Blocks freeze | Blocks reclaim |
| --- | --- | --- | --- |
| Audio | PipeWire stream active | Yes | Yes |
| Download | Background portal active transfer | Yes | No |
| Vpn | VPN interface owned by the app | Yes | No |
| Call | Call portal active | Yes | Yes |
| Navigation | Navigation portal active | Yes | No |
| UserPinned | User pinned app in settings | No | No (pins only affect the killer) |

## 4. Interfaces (Contracts)

These are defined first. Implementation follows these signatures. Types are Rust.

```rust
// Identity and state
pub struct AppId(pub String);                    // desktop file ID, e.g. "org.example.App"

pub enum AppState {
    Foreground,
    Visible,
    Background,
    Suspended,
    Hibernated,
    Terminated,
}

pub enum ProtectReason { Audio, Download, Vpn, Call, Navigation, UserPinned }

pub struct AppProcess {
    pub id: AppId,
    pub scope: std::path::PathBuf,               // cgroup scope path
    pub pids: Vec<u32>,
    pub state: AppState,
    pub protect: Vec<ProtectReason>,
    pub memory_current: u64,                     // bytes, from memory.current
    pub last_focused: std::time::Instant,
}

// Events from compositor, portals, PSI, and timers
pub enum Event {
    FocusChanged { to: Option<AppId> },
    WindowShown(AppId),
    WindowHidden(AppId),
    ProtectionChanged { id: AppId, reason: ProtectReason, active: bool },
    PressureChanged(PressureLevel),
    TimerElapsed { id: AppId, timer: TimerKind },
    AppExited(AppId),
}

pub enum TimerKind { Grace, Hibernate }

pub enum PressureLevel { Normal, Elevated, Critical }

// Decision and action layer
pub trait Policy {
    fn on_event(&mut self, event: Event, apps: &[AppProcess]) -> Vec<Action>;
}

pub enum Action {
    SetState { id: AppId, to: AppState },
    Reclaim { id: AppId, bytes: u64 },
    Kill { id: AppId, reason: KillReason },
    Notify { id: AppId, message: LifecycleMessage },
    Log { id: AppId, text: String },
}

pub enum KillReason { MemoryPressure, UserQuit, Crash }

// Mechanisms (kernel-facing)
pub trait Freezer {
    fn freeze(&self, scope: &std::path::Path) -> std::io::Result<()>;   // writes "1" to cgroup.freeze
    fn thaw(&self, scope: &std::path::Path) -> std::io::Result<()>;     // writes "0"
}

pub trait Reclaimer {
    fn reclaim(&self, scope: &std::path::Path, bytes: u64) -> std::io::Result<u64>; // writes to memory.reclaim; returns bytes requested
}

pub trait PressureSource {
    fn subscribe(&self) -> std::io::Result<PressureStream>;
}

pub trait Killer {
    fn kill(&self, scope: &std::path::Path) -> std::io::Result<()>;     // kills all processes in the scope
}

// App-facing notifications (lifecycle protocol)
pub enum LifecycleMessage { WillSuspend, DidResume, MemoryWarning }

pub enum Ack { Accepted, Timeout }

pub trait LifecycleNotifier {
    fn notify(&self, id: &AppId, message: LifecycleMessage) -> Result<Ack, NotifyError>;
}

#[derive(Debug)]
pub enum NotifyError { NoClient, Socket(std::io::Error) }
```

Notes on contracts:
- `Policy` contains no I/O. It takes events and returns actions. This makes it easy to unit test.
- Mechanisms (`Freezer`, `Reclaimer`, `PressureSource`, `Killer`) contain the only kernel I/O.
- Failures in a mechanism return an error. The policy logs it and does not retry more than twice (team retry rule).

## 5. Lifecycle Protocol (App-Facing)

1. Before suspending, the daemon sends `WillSuspend` through the SDK socket. The app has up to 2 s to acknowledge (`Ack::Accepted`).
2. On `Accepted`, the app should close network connections it does not need and save state.
3. On timeout, the daemon suspends the app anyway, unless the app holds an exception (section 3).
4. On resume, the daemon sends `DidResume`. The app should reopen connections it needs.
5. Under memory pressure, the daemon sends `MemoryWarning`. The app should release caches. The daemon waits up to 500 ms before it may kill.

SDK: a small library for GTK, Qt, Electron, and Flutter apps. Apps that do not use the SDK still get suspension, but without the messages.

## 6. Priority Bands and Killer

Bands are ordered from most protected (lowest number) to first to be killed (highest number).

| Band | Value | Members | Killable |
| --- | --- | --- | --- |
| System | 0 | Compositor, shell, lifecycle daemon, audio, network, session | No |
| Foreground | 1 | Focused app | No |
| Visible | 2 | Visible but not focused | Only at Critical level, after all lower bands |
| Protected | 3 | Apps with protection reasons (Audio, Call, Navigation, Vpn, Download, UserPinned) | Only at Critical level, after band 4 is empty |
| Background | 4 | Background, not protected | Yes, at Elevated level |
| Suspended | 5 | Suspended apps | Yes |
| Hibernated | 6 | Hibernated apps | Yes, first |

Within a band, the killer picks the app with the largest `memory_current`. Ties are broken by `last_focused` (oldest first).

Pressure response:

| Level | Trigger (proposed, tune in P5) | Actions |
| --- | --- | --- |
| Normal | PSI `some` avg10 < 10 | None |
| Elevated | PSI `some` avg10 ≥ 10 or PSI `full` avg10 ≥ 1 | Hibernate suspended apps now; reclaim from background apps |
| Critical | PSI `full` avg10 ≥ 5 | Send `MemoryWarning` to all apps; after 500 ms, kill one app from the lowest band that has an eligible app |

The killer kills one app per decision, then waits 2 s before evaluating again. This prevents a kill storm.

## 7. Decision Flow (Pseudocode)

```
on FocusChanged(to):
    if previous focus exists: SetState(previous, Visible or Background)
    SetState(to, Foreground)
    cancel Grace and Hibernate timers for to

on WindowHidden(id):
    SetState(id, Background)
    start Grace(id) with grace_period(power_source)

on TimerElapsed(id, Grace):
    if state(id) == Background and no protection(id):
        notify(id, WillSuspend) and wait up to 2 s
        freeze(id.scope); SetState(id, Suspended)
        start Hibernate(id) with hibernate_timeout

on TimerElapsed(id, Hibernate):
    if state(id) == Suspended and no protection(id) in {Audio, Call}:
        # Suspended apps have no freeze-blocking reason, so only Audio and Call matter here
        reclaim(id.scope, memory_current(id) * reclaim_ratio)
        SetState(id, Hibernated)

on PressureChanged(level):
    Elevated: for each Suspended app: Hibernate now
    Critical: notify(all, MemoryWarning); after 500 ms: Kill(lowest band eligible)
```

Notes:
- `reclaim_ratio` is a starting value of 0.8 for hibernated apps (proposed, tune in P5).
- Thawing is always immediate on focus. The app is not woken by a timer.

## 8. Failure Modes

| Failure | Detection | Response |
| --- | --- | --- |
| `cgroup.freeze` write fails | Error return from `Freezer` | Keep app Background; log; do not retry more than twice |
| Frozen app holds a socket and times out | Peer timeout reported by app SDK | Protection reason added by daemon (for example, Download); app is thawed |
| Killer kills an app with unsaved data | User report | Mitigation: `MemoryWarning` first; hibernated apps first; document in F2 |
| `memory.reclaim` not supported (kernel older than 5.19) | Write returns error | Fall back to `MADV_PAGEOUT`-based reclaim through the SDK, or disable hibernation and use suspend only |
| Daemon crash | Service manager | Restart; rebuild state from cgroup scopes; apps stay in their current frozen or running state |
| Stuck frozen process | Freeze timeout | Thaw; mark app as Background for 5 minutes |

Safety rule: the daemon never freezes or kills a process outside its own app scopes.

## 9. Metrics and Logs

| Metric | Source | Use |
| --- | --- | --- |
| Count per state | Daemon | Activity monitor, benchmark |
| Memory per state | `memory.current` per scope | Benchmark B1 |
| Restore time | Time from `FocusChanged` to frame | PT-09 |
| Kill count and reasons | Daemon log | B3, B5 |
| Decision time | Trace per event | PT-12 |
| PSI averages | `/proc/pressure/memory` | Pressure tuning |

Every action is logged with its reason. Example log line:

```
2026-11-16T10:02:11 lifecycled: app=org.example.Mail state Background->Suspended reason=grace_elapsed(30s)
```

## 10. Tests

| Test | Type | Pass criterion |
| --- | --- | --- |
| State machine transitions (every row in section 3) | Unit | All transitions produce the expected actions |
| Protection blocks suspend | Unit | No Freeze action with Audio protection |
| Killer order | Unit | Lowest eligible band killed first |
| Kill storm guard | Unit | No more than one kill in 2 s |
| Freeze and thaw on a real process | Integration | Process counter stops and resumes |
| 50-app benchmark | System | 04 section 4 passes |
| Crash recovery | Integration | Daemon restart keeps app states |
| Fuzzing of JSON messages | Security | No crashes in 1 hour of fuzzing |

## 11. Open Questions

- Exact PSI thresholds (section 6). Tune in P5-T05 on the 4 GB reference machine.
- Whether to use `memory.reclaim` only, or to also use the SDK's `MADV_PAGEOUT` path for apps that release memory themselves.
