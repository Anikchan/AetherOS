# D9 — Android Runtime Design (`nexa-android`)

Contracts first (section 4), then the lifecycle, bridges, and resource policy. Feature requirements: F12. Process rules: D3. Metrics: D10. Packaging and permission rules: D8.

## 1. Architecture

```
+-------------------------------------------------------------+
| Desktop: launcher, dock, Activity Manager, notifications         |
+-------------------------------------------------------------+
| nexa-android (manager, user service)                         |
|   lifecycle state machine, resource policy, install, updates |
+-------------------------------------------------------------+
| Bridges (run in the host session)                            |
|   window bridge  | input bridge | audio (PipeWire socket)    |
|   clipboard      | notifications | file picker (broker)      |
+-------------------------------------------------------------+
| Container (Android userspace; host Linux kernel)             |
|   own mount, PID, IPC, UTS, and network namespaces           |
|   Android system services, ART runtime, apps                 |
|   binder devices from the host kernel                        |
+-------------------------------------------------------------+
| Host kernel (cgroup v2 caps, freezer, PSI, DRM, binder)      |
+-------------------------------------------------------------+
```

Principle: the container runs on the host kernel. It does not emulate hardware. This is why performance can be close to native for x86 apps.

## 2. Components

| Component | Responsibility | Process |
| --- | --- | --- |
| `nexa-android` | Lifecycle, resource caps, install and update orchestration, state reporting | User service |
| Container runtime (internal engine) | Creates namespaces, mounts the Android system image, starts Android services | Started by `nexa-android` |
| Window bridge | Maps each Android app window to a Wayland surface with our decorations; shares buffers through the host GPU path | Helper in the user session |
| Input bridge | Converts pointer, keyboard, and touch events; maps back and long-press actions (F12 AND-17) | Helper |
| Audio | Passes the host PipeWire socket into the container | Configuration only |
| Clipboard bridge | Syncs text clipboard between host and container | Helper |
| Notification bridge | Forwards Android notifications to the host notification center | Helper |
| File picker broker | Gives the container file handles from the host picker only (D8 section 5) | `nexa-permissions` |
| Install and store | Reads the open repository index, verifies package signatures and checksums | `nexa-pkg` (D8) |

## 3. Lifecycle

### 3.1 States

| State | Meaning | Memory in use |
| --- | --- | --- |
| Absent | Pack not installed | 0 |
| Stopped | Installed; container not running | 0 |
| Starting | Container stages running (prepare, binder ready, system services ready, app ready) | Rising to the cap |
| Running (focused) | An Android app is focused | Up to the cap |
| Running (background) | Apps open but none focused; CPU weight low | Up to the cap |
| Frozen | Cgroup frozen; memory compressed | ≤ 150 MB target (F12 AND-T5) |
| Hibernated | Frozen and reclaimed under pressure | Below the frozen value |
| Stopping | Container shutdown in progress | Falling |
| Failed | Start or run failed; reason stored | 0 after cleanup |

### 3.2 Transitions

| From | To | Condition |
| --- | --- | --- |
| Stopped | Starting | User opens an Android app, or keep-ready is on at login |
| Starting | Running (focused) | First app window appears |
| Starting | Failed | A stage times out (30 s) or a required kernel feature is missing |
| Running (focused) | Running (background) | No Android app is focused |
| Running (background) | Frozen | No Android app focused for 60 s (configurable) |
| Frozen | Running (focused) | User focuses an Android app (thaw immediately) |
| Frozen | Hibernated | Memory pressure reaches Elevated (D3 section 6) |
| Any running state | Stopping | Pack removed, or no Android app for 30 minutes and keep-ready is off |
| Stopping | Stopped | Shutdown complete |

### 3.3 Startup Stages (shown to the user)

| Stage | Expected time (estimate) | Message |
| --- | --- | --- |
| Prepare | 0.5 s | "Preparing Android" |
| Kernel interfaces | 0.5 s | "Connecting services" |
| System services | 4 to 8 s | "Starting Android" |
| First app | 1 to 3 s | "Opening app" |

Targets are in F12 section 4 (AND-T1, AND-T2). A stage that takes longer than its limit is shown with the stage name, not as a frozen window.

## 4. Contracts (Rust)

```rust
pub enum AndroidState { Absent, Stopped, Starting, RunningFocused, RunningBackground, Frozen, Hibernated, Stopping, Failed(String) }

pub struct AndroidConfig {
    pub memory_cap_bytes: u64,            // default 1.5 GB
    pub cpu_weight: u32,                  // default 50 (native apps default 100)
    pub io_weight: u32,
    pub keep_ready: bool,
    pub background_grace: std::time::Duration,   // default 60 s
    pub idle_stop_after: std::time::Duration,    // default 30 min
    pub microg_enabled: bool,
}

pub enum StartStage { Prepare, KernelInterfaces, SystemServices, FirstApp }

pub trait AndroidContainer {
    fn start(&mut self, stage_sink: &dyn Fn(StartStage)) -> Result<(), AndroidError>;
    fn freeze(&mut self) -> Result<(), AndroidError>;
    fn thaw(&mut self) -> Result<(), AndroidError>;
    fn stop(&mut self) -> Result<(), AndroidError>;
    fn state(&self) -> AndroidState;
}

pub trait WindowBridge {
    fn on_app_window(&mut self, app: &AndroidAppId, title: &str) -> Result<SurfaceId, AndroidError>;
    fn on_app_window_closed(&mut self, surface: SurfaceId);
}

pub trait InputBridge {
    fn map_key(&self, host: HostKey) -> Option<AndroidKey>;
}

pub trait ClipboardBridge {
    fn push_text(&mut self, text: &str) -> Result<(), AndroidError>;
    fn pull_text(&mut self) -> Result<Option<String>, AndroidError>;
}

pub enum TranslationStatus { NotInstalled, UserSupplied { path: std::path::PathBuf } }

pub trait AppStateReporter {
    fn report(&self, app: &AndroidAppId, state: crate::AppState);   // D3 states
}

#[derive(Debug)]
pub enum AndroidError { KernelFeatureMissing(String), StageTimeout(StartStage), CapExceeded, Io(std::io::Error), Container(String) }
```

Notes:
- `AndroidContainer::start` reports each stage through a sink, so the shell can show progress.
- `TranslationStatus::UserSupplied` is set only when the owner adds a translator on their own machine. The OS never ships or downloads one.

## 5. Resource Policy

| Control | Value (default) | Mechanism |
| --- | --- | --- |
| Memory cap | 1.5 GB | `memory.max` on the container's cgroup scope; `memory.high` at 1.2 GB as a soft limit |
| CPU share | Weight 50 (native apps 100) | `cpu.weight` |
| Disk I/O share | Low weight | `io.weight` |
| Freeze | After 60 s with no Android app focused | `cgroup.freeze` |
| Compression | Frozen container memory reclaimed | `memory.reclaim` plus zram |
| Pressure | Container is the first group to hibernate after background native apps | D3 bands (container placed in the Background band) |
| Killing | Container is killed only at Critical pressure and only after native background apps | D3 section 6 |

Rule: the container must never reach host Critical pressure by itself. When its cap is reached, Android's own low-memory killer ends its background apps first. The host does not wait for that.

Keep-ready mode: the container starts at login with a low priority, runs its services until the first freeze, and then stays frozen. Its frozen memory is the cost (about 150 MB target). This cost is shown in Settings.

## 6. Keep-Ready Memory Cost (Estimate)

| Mode | Idle memory cost | Start time for first app |
| --- | --- | --- |
| Lazy (default) | 0 MB | 8 s target |
| Keep-ready (frozen) | About 150 MB (target) | 1.5 s target (thaw) |
| Running and unfocused (before freeze) | Up to the cap (1.5 GB default) for at most 60 s | Immediate |

Numbers are estimates until measured by spike S6 and gates G-AND1 to G-AND3.

## 7. Failure Modes

| Failure | Response |
| --- | --- |
| Binder support missing in the kernel | Pack refuses to start, explains the missing feature, does not change the host |
| A start stage times out | Shows the stage name, offers Retry (at most 2 automatic retries), then Failed with the log |
| Container crashes | Apps close, container stops, state becomes Failed; host unaffected; logs stored |
| ARM app without translator | Store and launch show "needs translation"; no install or start attempt |
| Window bridge cannot map an app window | Falls back to the full Android window (documented major limit) |
| Pack removed while running | Stopping, then removal (F15 section 4) |
| Host pressure rises | Freeze, then hibernate; never block native apps |

## 8. Security Notes

- The container shares the host kernel. A bug in the kernel or the container can affect the host. Mitigation: apply the same seccomp profile and namespaces used for apps (D8 section 5), keep the container image updated, and offer a stronger virtual-machine mode later.
- No container access to host files except the picker and the shared folder (if enabled).
- Network inside the container is limited by the same permission rules as other apps (F9 AP-07).
- Android's security model is kept; our additions do not weaken it.

## 9. Later Option: Virtual-Machine Mode

A lightweight virtual machine could run the same Android image with stronger isolation. It costs more memory and some speed. It is not in Personal Edition. Decision point: after M5, if the owner wants stronger isolation.

## 10. Tests

| Test | Type | Pass criterion |
| --- | --- | --- |
| Lifecycle transitions (section 3.2) | Unit | Each transition produces the expected action |
| Cap enforcement | Integration | Container memory never exceeds `memory.max` |
| Freeze and thaw | Integration | Container processes stop and resume; no crash |
| Cold start stages | Integration | Each stage reported; total within AND-T1 |
| Warm launch | Integration | Within AND-T2 for the test app set |
| Host isolation | System | Native foreground app keeps frame gaps under 1 s (G-AND5) |
| Removal | Integration | No files, processes, or registry entries remain (G-OPT3) |
| Missing binder | Integration | Clear message; host unchanged |
