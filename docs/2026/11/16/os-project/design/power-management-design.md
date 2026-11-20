# D5 — Power Management Design (`aether-powerd`)

This document defines the contracts first (section 4), then the behavior. Feature requirements are in F7. Process management interactions are in D3.

## 1. Purpose

Choose the best power profile and device settings for the current situation, estimate battery life accurately, protect battery health, and explain every decision. Keep the decision logic separate from the kernel writes, so that the logic can be tested without hardware.

## 1.1 Implementation notes (spikes, 2026-11-18)

- **Battery telemetry (spike S4, P0-T10)** is still **blocked on real
  hardware** (a machine with a battery; the spare Dell or a laptop VM
  passthrough). The design assumes UPower-first with a sysfs fallback
  (section 10) and a "not reported" display for missing values (risk R15);
  both are retained. No battery numbers are recorded yet.
- **PSI (spike S3, P0-T09)** was absent from the Alpine virt kernel; the
  P1-T01 fragment sets `CONFIG_PSI=y`. Powerd does not depend on PSI
  directly, but any future pressure-aware tuning must check availability
  (see D3 note).

## 2. Mechanisms

| Mechanism | Source | Used for |
| --- | --- | --- |
| Battery telemetry | UPower (D-Bus), or sysfs `/sys/class/power_supply/BAT*/` | Percentage, state, energy rate, full and design capacity, cycle count |
| Power profile | power-profiles-daemon (D-Bus) | Profile switch (Performance, Balanced, Saver) |
| Platform profile | `/sys/firmware/acpi/platform_profile` (where present) | Laptop-level profile |
| CPU energy preference | `/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference` | CPU performance hint (EPP) |
| Charge limit | `/sys/class/power_supply/BAT*/charge_control_end_threshold` (where present) | Battery health |
| PCIe power saving | `/sys/module/pcie_aspm/parameters/policy` | Link power |
| USB autosuspend | `/sys/bus/usb/devices/*/power/control` | Device idle power |
| Runtime power management | `/sys/bus/*/devices/*/power/control` | Per-device idle power |
| Wi-Fi power save | NetworkManager setting `wifi.powersave` | Wi-Fi idle power |
| Thermal | `/sys/class/thermal/`; thermal daemon for Intel | Temperature control |
| Sleep | `/sys/power/mem_sleep`, systemd sleep targets | Suspend-to-idle, suspend-then-hibernate |
| Lid and power events | systemd-logind (D-Bus) | Lid close, power button, AC change |

Paths are the standard Linux interfaces. Each one is checked at startup. A missing path disables the related feature and is shown in the power view.

## 3. Components and Responsibilities

| Component | Responsibility |
| --- | --- |
| `aether-powerd` (Rust) | Policy: decides profile, device settings, actions, and notifications. Owns the tuner and the estimator. |
| UPower | Reads battery state. Used instead of writing a new battery reader (ADR-014). |
| power-profiles-daemon | Applies profiles to the platform and CPU. |
| thermald | Intel thermal control (where used). |
| `aether-lifecycled` (D3) | Receives background limits from `aether-powerd` and applies them to app scopes. |
| `aether-shell` (control center and power view) | Shows status and accepts user choices. |

## 4. Contracts (Rust)

Types first. The policy has no I/O. Mechanisms hold all I/O.

```rust
pub enum PowerSource { Ac, Battery, Unknown }

pub enum PowerProfile { Performance, Balanced, Saver }

pub struct BatteryStatus {
    pub percent: f32,                       // 0.0 to 100.0
    pub source: PowerSource,
    pub charging: bool,
    pub energy_rate_w: Option<f32>,         // watts; None when unknown
    pub time_to_empty: Option<std::time::Duration>,
    pub time_to_full: Option<std::time::Duration>,
    pub health_percent: Option<f32>,        // full capacity / design capacity
    pub cycle_count: Option<u32>,
}

pub enum ThermalLevel { Normal, Warm, Hot }

pub enum PowerEvent {
    SourceChanged(PowerSource),
    BatteryUpdated(BatteryStatus),
    LidChanged { closed: bool },
    UserProfileRequest(PowerProfile),
    ThermalChanged(ThermalLevel),
    UserIdle(bool),
    DeviceError { device: String },         // from kernel log watcher after tuning
}

pub enum PowerAction {
    SetProfile(PowerProfile),
    SetDevicePowerSaving { device: String, enabled: bool },
    SetChargeLimit(u8),
    SetBackgroundPolicy(BackgroundPolicy),
    SetLowBatteryAction(LowBatteryAction),
    Notify(String),
    Suspend,
    Hibernate,
    Log(String),
}

pub struct BackgroundPolicy {
    pub grace_period: std::time::Duration,
    pub hibernate_after: std::time::Duration,
    pub cpu_weight: u32,
}

pub enum LowBatteryAction { Suspend, Hibernate }

pub trait PowerPolicy {
    fn on_event(&mut self, event: PowerEvent) -> Vec<PowerAction>;
}

pub trait BatteryTelemetry {
    fn read(&self) -> std::io::Result<BatteryStatus>;
}

pub trait ProfileBackend {
    fn apply(&self, profile: PowerProfile) -> Result<(), PowerError>;
}

pub trait DeviceTuner {
    fn apply(&self, device: &str, enabled: bool) -> std::io::Result<()>;
    fn revert_all(&self) -> std::io::Result<()>;
}

pub trait ChargeLimiter {
    fn supported(&self) -> bool;
    fn set_limit(&self, percent: u8) -> std::io::Result<()>;
}

pub trait Estimator {
    fn update(&mut self, sample: &BatteryStatus, at: std::time::Instant);
    fn time_to_empty(&self) -> Option<std::time::Duration>;   // None while estimating
}

#[derive(Debug)]
pub enum PowerError { Unsupported(String), Io(std::io::Error), Backend(String) }
```

## 5. Policy Rules

| Condition | Action |
| --- | --- |
| Source changes to Battery | Profile to Saver (unless the user chose a profile for this session); background policy to battery values |
| Source changes to AC | Profile to Balanced (or the user's AC choice); background policy to AC values |
| User requests a profile | Apply it and keep it until the next source change |
| Thermal level Hot | Profile to Balanced at most, even on battery; reduce effects tier (F6) |
| Battery at 20% | Notify once |
| Battery at 10% and not charging | Notify; profile to Saver |
| Battery at 5% and not charging | Run low-battery action (suspend or hibernate, per config) |
| Battery at 3% and not charging | Hibernate if a swap file exists; otherwise suspend |
| Lid closed and not charging | Suspend (or suspend-then-hibernate if enabled) |
| Device error within 10 minutes of tuning | Revert that device, add it to the exclusion list, log the event |

Rule priority: safety rules (low battery, thermal) > user choice > automatic profile switching.

## 6. Battery Estimate Algorithm

1. Take one sample each time UPower reports an update (normally every 10 to 30 seconds).
2. Keep a moving average of the energy rate over 2 to 5 minutes (window length from config).
3. Time to empty = remaining energy / average rate. Remaining energy uses the full capacity reported by the kernel.
4. While fewer than 2 minutes have passed since the last source change, return `None` ("Estimating...").
5. Reject a sample whose rate is more than 3 times the average, unless the rate stays high for 3 samples in a row (a real load change).

## 7. Low-Battery State Machine

```
Normal --(20%)--> Warned20 --(10%)--> Saver10 --(5%)--> ActionDue5 --(3%)--> Critical3
  ^                                                                              |
  +------------------------------ (AC plugged in, reset) -------------------------+
```

- Each state runs once per discharge cycle. A state change is logged with the battery percentage and time.
- `ActionDue5` runs the configured low-battery action. If the action fails, `Critical3` runs the fallback.

## 8. Sleep Policy

| Option | Mechanism | Requirement |
| --- | --- | --- |
| Suspend-to-idle (default) | `mem_sleep` = `s2idle` | Works on most modern laptops |
| Deep sleep | `mem_sleep` = `deep` | Only if the firmware offers it; checked at startup |
| Suspend-then-hibernate | systemd `suspend-then-hibernate` | Disk swap file at least as large as RAM (zram cannot be used) |

- The installer offers to create the swap file (D4 section 2, F5). It is off by default on disks under 64 GB.
- Hibernation resume checks the swap file before resuming. A failed resume boots normally and logs the reason.

## 9. Device Tuner

| Step | Rule |
| --- | --- |
| Safe list | PCIe ASPM, USB autosuspend, Wi-Fi power save, audio codec power save, SATA link power. Each is enabled on Saver. |
| Exclusion list | Stored in `~/.config/nexa/power/exclusions.toml` and in the system defaults. Devices with known problems are excluded (for example, certain USB keyboards or mice). |
| Apply | Write the setting, then record the old value. |
| Watch | Read kernel messages for 10 minutes after each change. A device error or disconnect for a tuned device triggers `DeviceError`. |
| Revert | Restore the old value for that device, add it to the exclusion list, and log the event. |
| Automatic reset | If the exclusion list grows past 10 devices, stop tuning and notify the owner. |

Tuning never changes devices that are in use by a safety-critical function (for example, the keyboard used for the lock screen).

## 10. Failure Modes

| Failure | Response |
| --- | --- |
| UPower not running | Read sysfs directly through `BatteryTelemetry` fallback; show a warning in the power view |
| Profile backend fails | Keep the last working profile; log; retry at most twice |
| Charge limit write fails | Hide the control; log the reason |
| Device tuning causes errors | Revert and exclude (section 9) |
| Estimator has no data | Show "Estimating..." |
| Daemon crash | Restart; reread state from UPower and sysfs; keep the last applied profile |
| Hibernation resume fails | Boot normally; log the reason; notify on the next login |

## 11. Metrics and Logs

| Metric | Source | Use |
| --- | --- | --- |
| Battery percentage over time | Telemetry log | Battery benchmark (04 section 11.2) |
| Profile changes and reasons | Policy log | Power view and power report |
| Device tuning changes and reverts | Tuner log | Stability checks |
| Estimate error | Compare estimate with actual time | PWR-AC-03 |
| Idle drain per hour | Telemetry over 1-hour idle | PWR-AC-06 |

Every action is logged with its reason. Example:

```
2026-11-16T19:40:02 aether-powerd: profile Balanced->Saver reason=source_changed(battery)
```

## 12. Tests

| Test | Type | Pass criterion |
| --- | --- | --- |
| Policy rules (every row in section 5) | Unit | Expected actions produced for each event |
| Low-battery state machine (section 7) | Unit | Each state runs once per cycle; reset on AC |
| Estimator (section 6) | Unit | "Estimating..." for 2 minutes; spike rejection works |
| Tuner revert on error | Integration | Error triggers revert and exclusion |
| Charge limit | Integration (hardware with support) | Limit applied and read back |
| Battery benchmark | System | 04 section 11.2 passes |
| Sleep and resume | System | 10 of 10 cycles |
| Suspend-then-hibernate | System | Resumes after hibernation delay; falls back on failure |

## 13. Open Questions

- Which Windows-like power plan names to show. Proposed: Performance, Balanced, Saver (already used in F7).
- Whether to use power-profiles-daemon as-is or to ship a small patched version. Proposed: as-is, with our policy on top (ADR-014).
