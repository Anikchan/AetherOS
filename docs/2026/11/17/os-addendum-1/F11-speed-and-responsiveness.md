# F11 — Speed, Fast Boot, and No-Hang Design

Owner: whole system. Targets are in F6 (RES-05, RES-09 to RES-12). Measurement is in D6. This document answers how apps open fast and how the system avoids hangs.

## 1. Boot Speed

### 1.1 Targets

| Measure | Target | Limit | Measured from |
| --- | --- | --- | --- |
| Bootloader start to login prompt | ≤ 5 s | 7 s | Bootloader timestamp |
| Bootloader start to desktop ready | ≤ 10 s | 15 s (RES-05) | Compositor first frame |
| Stretch goal, desktop ready | 8 s | — | Same |

Firmware time (the machine's own startup before the bootloader) is not ours. It is measured and reported separately. Disk encryption adds the time the user needs to type a password; that is reported separately too.

### 1.2 Stage Budget

| Stage | Budget | How it is achieved |
| --- | --- | --- |
| Bootloader | ≤ 0.5 s | systemd-boot with no menu delay on single-OS machines; menu only when two systems are installed |
| Kernel | ≤ 2 s | Drivers needed for the owner's hardware are built into the kernel, not loaded as modules; unused subsystems removed; quiet boot |
| Early userspace | ≤ 1 s | Minimal initramfs; root is read-only squashfs; no checks on every boot (journaled file systems check only when dirty) |
| Services to login | ≤ 2 s | Socket activation; services start in parallel; no service waits for the network unless it needs it |
| Login to desktop | ≤ 3 s | Compositor and shell start first; greeter and shell are pre-initialized; fonts pre-built in the image |

### 1.3 Boot Techniques

| ID | Technique | Effect |
| --- | --- | --- |
| BT-01 | Unified kernel image (single signed or unsigned EFI file) | Fewer file reads at boot |
| BT-02 | Built-in drivers for the owner's hardware | No module loading delay |
| BT-03 | Minimal initramfs (only what root mount needs) | Shorter early boot |
| BT-04 | Parallel service start with socket activation | Shorter services stage |
| BT-05 | Shell and compositor prioritized in the start order | Earlier desktop |
| BT-06 | Pre-built font cache and icon cache in the image | No first-run cache build |
| BT-07 | Readahead of the boot and desktop files | Fewer slow disk reads |
| BT-08 | Hibernation resume as a fast-boot option (requires swap, D5 section 8) | Returns to the previous session in a few seconds |
| BT-09 | Boot-time report: each service timed; the slowest one is listed in the power and speed view | Ongoing tuning |

## 2. App Launch Speed

### 2.1 Targets

| App type | Target (cold launch to first frame) | Limit |
| --- | --- | --- |
| Built-in default apps (F8) | 500 ms | 1 s |
| Native OS bundles | 1 s | 2 s |
| Windows programs (through the engine) | 3 s (engine startup adds time) | 6 s |
| Restore of a hibernated app | 500 ms | 1 s (RES-12) |

### 2.2 Launch Techniques

| ID | Technique | Effect |
| --- | --- | --- |
| LA-01 | Prewarm: the three apps used most often (learned from usage on the device) are read into memory after login, at low priority | Faster first use |
| LA-02 | Pre-initialized GUI runtime: a helper process has the UI library already loaded; each new app is forked from it instead of loading everything again. The helper is kept single-threaded so forking is safe. | Lower launch time |
| LA-03 | Compiled UI: Slint compiles interface files into Rust at build time, so no interface parsing at runtime | Faster first frame |
| LA-04 | Lazy loading: each app loads a view only when it is shown | Faster start, less memory |
| LA-05 | Hibernated apps restore from compressed memory (D3) | Restore in about 0.5 s |
| LA-06 | No network calls on the startup path of any built-in app | No waiting on slow networks |
| LA-07 | Disk reads for apps come from the read-only image through the page cache | Warm reads are fast |
| LA-08 | Windows programs: the engine keeps a warm server process between launches when the user allows it (costs a small amount of memory) | Lower Windows startup time |

## 3. No-Hang Design

A hang means the user sees no response for more than 1 second while the system is not in a full resource crunch.

### 3.1 Rules

| ID | Rule | Effect |
| --- | --- | --- |
| NH-01 | The compositor never waits for an app. Every app update is asynchronous. | One app cannot freeze the desktop |
| NH-02 | Unresponsive detection with the standard Wayland ping. The compositor sends a ping to each window; if the app does not reply within 5 s, the window is marked "Not responding" with Wait and Force Quit options. | The user knows what is wrong and can act |
| NH-03 | The shell and compositor run under a watchdog. If the shell stops responding, the service manager restarts only the shell, and the compositor keeps the windows. | Desktop recovers without losing open apps |
| NH-04 | Priorities: the foreground app gets a higher CPU weight, a memory protection (memory.low), and a higher disk I/O weight (D3) | Foreground stays responsive under load |
| NH-05 | Pressure response order: warn, then hibernate background apps, then ask the user (for large apps), then terminate by priority band (D3 section 6). The kill happens only at Critical pressure. | Crunch is handled in order |
| NH-06 | No blocking disk or network calls on the UI thread. Enforced by code review rules and a lint in CI. | Smooth interaction |
| NH-07 | Frame-gap logger records every gap over 1 s with its cause (CPU, memory, I/O, or unknown). | Hangs are measured, not guessed |
| NH-08 | "Hang" is counted only outside the Critical pressure level. Gaps inside Critical are reported separately. | Fair measurement |

### 3.2 Definition of "full resource crunch"

Memory: Linux PSI "full" average over 10 seconds at or above 5% (the Critical level in D3 section 6). CPU and I/O crunch use the same method with their own PSI files.

## 4. Power Link

- Frame rate and effects drop in Saver mode (F7). This is a deliberate choice, not a hang.
- Launch targets apply in Balanced mode. In Saver mode, the target is allowed to be 1.5 times longer. Both values are recorded.

## 5. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| FS-AC-01 | Bootloader to login prompt ≤ 5 s on the reference SSD machine (median of 10 boots) | Not tested |
| FS-AC-02 | Bootloader to desktop ready ≤ 10 s (median of 10 boots) | Not tested |
| FS-AC-03 | Each built-in default app cold launch ≤ 500 ms (median of 20 launches) | Not tested |
| FS-AC-04 | Hibernated app restore ≤ 500 ms for apps hibernated less than 10 minutes | Not tested |
| FS-AC-05 | A test app that stops responding is marked "Not responding" within 6 s and can be force-quit | Not tested |
| FS-AC-06 | A hung app does not freeze the desktop or other apps | Not tested |
| FS-AC-07 | Shell watchdog restarts a stopped shell within 5 s without closing open windows | Not tested |
| FS-AC-08 | 24-hour daily-use test: zero hangs over 1 s outside the Critical pressure level | Not tested |
| FS-AC-09 | Frame-gap log shows the cause for every recorded gap | Not tested |

## 6. Open Questions

- Whether LA-08 (warm Windows engine server) is acceptable for memory use on 8 GB machines. Measure in P6.
- Whether the owner wants the boot-time report (BT-09) visible in the UI or only in the command line.
