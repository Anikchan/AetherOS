# 04 — Test and Acceptance

## 1. Test Levels

| Level | What is tested | Tools | Where it runs |
| --- | --- | --- | --- |
| Unit | Rust logic (state machine, bands, config parsing), QML components | `cargo test`, QML test runner | CI on every change |
| Integration | Boot, services, compositor with test clients | QEMU with OVMF, scripted boot test | CI nightly and on every release candidate |
| System | Desktop features, installer, updates | Manual checklist (section 5), scripted UI tests where possible | Reference machines |
| Hardware | Drivers, power, peripherals | Hardware checklist (section 6) | Reference machines |
| Performance | Boot time, RAM, CPU, GPU frame rate, 50-app benchmark | `systemd-analyze`, `/proc/pressure/memory`, `zramctl`, `smem`, `perf`, `bpftrace`, `intel_gpu_top`, `radeontop` | Reference machines |
| Compatibility | Windows and Linux apps | Test list (section 7) | Reference machines |
| Security | Installer, daemon, update path | `cargo-fuzz`, manual review, signature checks | CI and before release |

## 2. Performance Targets

| ID | Metric | Target | Measured by |
| --- | --- | --- | --- |
| PT-01 | Boot to login prompt (QEMU) | ≤ 10 s | `systemd-analyze` in QEMU |
| PT-02 | Boot to desktop ready (4 GB SSD laptop) | ≤ 10 s target, 15 s limit | Timed boot script (addendum F11) |
| PT-03 | Base image size (ISO, compressed) | ≤ 512 MB; target ≤ 400 MB | Build output |
| PT-04 | Idle RAM with desktop open and no apps (4 GB machine) | ≤ 300 MB used | `free`, `smem` |
| PT-05 | Idle CPU average over 5 minutes | < 2% | `top -l 2` sampling |
| PT-06 | Desktop frame rate, Full glass tier, 3 windows (reference integrated GPU) | 60 fps | Compositor frame counter |
| PT-07 | Desktop frame rate, Reduced tier | ≥ 30 fps | Compositor frame counter |
| PT-08 | Suspend and resume to desktop | ≤ 3 s | Timed cycle |
| PT-09 | Restore a hibernated app to foreground | ≤ 500 ms if hibernated less than 10 minutes | Timed switch with test app |
| PT-10 | Resident memory of one hibernated simple GUI app | ≤ 5 MB | `memory.current` in its cgroup |
| PT-11 | Idle battery drain on a reference laptop | ≤ 6% per hour | Battery reading over 1 hour |
| PT-12 | Process manager decision time per event | ≤ 1 ms average | Built-in trace |
| PT-13 | Cold launch of a native shell app to first frame | ≤ 500 ms | Timed launch log (11.3) |
| PT-14 | Input-to-screen latency added by the compositor | ≤ 1 frame at 60 Hz | Compositor timestamps; photodiode tester if available (11.3) |
| PT-15 | Menu bar, launcher, or control center open | ≤ 100 ms | Timed input log (11.3) |
| PT-16 | Power profile switch after plugging or unplugging | ≤ 5 s | Timed event log (F7 PWR-AC-01) |

## 3. Acceptance Criteria by Area

Each feature document (F1 to F6) lists its own acceptance criteria. This section lists the release-level gates.

| Gate | Criterion |
| --- | --- |
| G1 | Boots in QEMU (PT-01) and on each reference machine marked Pass in section 6 |
| G2 | All MVP features in F1 to F6 marked Pass in their acceptance tables |
| G3 | Performance targets PT-01 to PT-12 met, or each miss is documented with a plan |
| G4 | 50-app benchmark (section 4) passes |
| G5 | Installer dual-boot test passes 10 of 10 cycles on 3 Windows 11 machines |
| G6 | Update and rollback test passes 20 of 20 |
| G7 | Security review has no open critical findings |

## 4. 50-App Benchmark (Process Management)

Purpose: prove that inactive apps use little memory and that the foreground app stays responsive.

### 4.1 Setup
1. Reference machines: one with 8 GB RAM and one with 4 GB RAM.
2. Boot AetherOS with no other apps running. Record baseline memory.
3. Launch 50 test apps. Use a mix: 30 synthetic Wayland test clients (each allocates about 20 MB, with a 5 MB working set) and 20 real apps (browser tab set, text editor, file manager, music player, chat, terminal, and others).

### 4.2 Procedure
1. Leave all 50 apps idle for 5 minutes. Record total used memory, memory in suspended and hibernated apps, and `memory.pressure`.
2. Switch focus to 10 random apps in sequence. Measure restore time for each.
3. Play audio in one app and start a download in another. Keep both in the background for 10 minutes. Confirm they keep working.
4. Run a memory-heavy test app in the foreground until pressure is high. Confirm the foreground app is never killed and that background apps are killed in priority order.

### 4.3 Pass Criteria
| Check | Criterion |
| --- | --- |
| B1 | Memory used by suspended and hibernated apps ≤ 25% of total used memory after 5 minutes |
| B2 | Restore time for hibernated apps ≤ 500 ms for apps hibernated less than 10 minutes (PT-09) |
| B3 | Foreground app killed: 0 times |
| B4 | Audio and download exceptions keep working for 10 minutes |
| B5 | Under memory pressure, the killer ends apps by priority band (D3) and logs the reason |
| B6 | Share of time in "full" memory pressure below 5% during the test (excluding step 4) |

## 5. Daily-Driver Checklist (M3)

Run once per day for 7 consecutive days. All items must pass on the owner's laptop.

- [ ] Boot, log in, and reach the desktop
- [ ] Wi-Fi connects after reboot and after sleep
- [ ] Bluetooth headphones pair and play audio
- [ ] Two monitors work with different scaling
- [ ] Windows, snap, tiling, and workspace switching work
- [ ] Menu bar, dock, launcher, and control center open and close
- [ ] Notifications appear and Focus mode silences them
- [ ] Lock screen locks and unlocks
- [ ] Install an app from the store and launch it
- [ ] Run one Windows app through Wine
- [ ] Suspend and resume without errors
- [ ] Screenshot and screen recording work
- [ ] Settings change from the settings app is reflected in the config file

## 6. Hardware Checklist and Matrix

### 6.1 Checklist (per reference machine)

| Item | Pass criterion |
| --- | --- |
| Display output | Native resolution and refresh rate |
| External display (HDMI or USB-C) | Works, with hot-plug |
| Graphics acceleration | Vulkan and OpenGL report the real GPU |
| Wi-Fi | Connects, reconnects after sleep |
| Bluetooth | Pairs, plays audio |
| Audio out and in | Speakers, headphone jack, microphone |
| Touchpad | Tap, scroll, three-finger gestures |
| Keyboard | All keys, backlight if present |
| Brightness and volume keys | Work |
| Webcam | Video works |
| Suspend and resume | 10 of 10 cycles succeed |
| Battery | Percentage and time remaining are reported |
| Fan and temperature | Readable; fan control if supported |
| Reboot into Windows and back | Both boot |

### 6.2 Matrix

| Machine | CPU and GPU | Wi-Fi | Status | Notes |
| --- | --- | --- | --- | --- |
| Reference 1 (to be chosen, P0-T04) | — | — | Not tested | — |
| Reference 2 | — | — | Not tested | — |
| Reference 3 | — | — | Not tested | — |
| Reference 4 | — | — | Not tested | — |
| Reference 5 | — | — | Not tested | — |

## 7. Compatibility Test List

Choose one popular app per category. The tier system is defined in F3.

| Category | Count | Examples (choose the app, not the brand) |
| --- | --- | --- |
| Browser | 3 | Chromium-based, Firefox-based, one other |
| Office and productivity | 8 | Word processor, spreadsheet, presentation, PDF reader, note app |
| Media | 6 | Video player, audio player, video editor, image editor |
| Development | 7 | Code editor, IDE, terminal tool, database client |
| Games | 8 | DirectX 11 game, DirectX 12 game, indie game, launcher-based game |
| Utilities | 8 | Archive tool, file sync, password manager, system monitor |

Total: 40 apps. Tiers:

| Tier | Meaning |
| --- | --- |
| A | Works with no changes |
| B | Works after a documented setting or prefix tweak |
| C | Partly works (some functions fail) |
| D | Does not work; recommended to use dual boot |

## 8. Bug Severity

| Severity | Meaning | Release rule |
| --- | --- | --- |
| S1 Critical | Data loss, boot failure, security issue, foreground app killed | Must be fixed before release |
| S2 Major | Feature broken with no workaround | Must be fixed or documented before release |
| S3 Minor | Feature works with a workaround or cosmetic issue | Can ship with a known issue |
| S4 Trivial | Typos, small layout issues | Fix when convenient |

## 9. Beta Release Criteria (P10-T05)

1. Gates G1 to G7 pass or have a documented plan.
2. No open S1 or S2 bugs.
3. Hardware matrix has at least 10 machines recorded.
4. Owner dogfooding for 30 days completed (P10-T04).
5. Release notes list the certified hardware and the compatibility tiers.

## 10. Evidence Rule

A test passes only when its result is recorded with the command used, the machine, the date, and the outcome. A build succeeding or a code review alone does not count as a test result.

## 11. Hardware Coverage, Battery, and Speed Tests

### 11.1 Hardware Coverage Goal (Q2, Q10)

| ID | Rule |
| --- | --- |
| HW-COV-01 | Every device on the owner's machines that has a Linux driver in the chosen kernel works and passes the checklist in section 6 |
| HW-COV-02 | The coverage report (P2-T08) lists each device as Working, Partly working, Needs firmware, or No driver |
| HW-COV-03 | Target: at least 90% of listed device functions working on the owner's machines by M2; on all test machines by M5 |
| HW-COV-04 | Devices with only Windows drivers are reported as "No driver". They are not a defect |
| HW-COV-05 | Both UEFI and legacy BIOS boot are tested on at least one machine each |
| HW-COV-06 | Owner answer Q10 (2026-11-17): broader hardware coverage is preferred. The test matrix is expanded toward more devices; devices with only Windows drivers stay unsupported |

### 11.2 Battery Benchmark (F7)

Setup: the owner's laptop, full charge, charge limit at 100%, brightness fixed at 50%, Wi-Fi on, same browser profile and same test pages.

| Workload | Length | Measure |
| --- | --- | --- |
| W1 Idle at Balanced profile, lid open | 1 hour | Battery percentage drop every 5 minutes |
| W2 Scripted browsing (fixed list of 20 pages) | 1 hour | Battery percentage drop; time to 10% |
| W3 Local video playback (fixed file) | 1 hour | Battery percentage drop |

Procedure:
1. Run W1, W2, and W3 on AetherOS. Repeat 3 times and take the median.
2. Run the same three workloads on Windows 11 on the same laptop (dual boot). Same brightness, Wi-Fi, and browser.
3. Record the readings with the sampler script in `powerd/bench/`.

Pass criteria:
- W1 idle drain ≤ 6% per hour (limit) and ≤ 4% per hour (target) (PWR-AC-06).
- W2 and W3 battery life within 15% of Windows 11 (PWR-AC-07).
- No failed device tuning (D5 section 9) during the runs.

### 11.3 Speed Tests (F6)

| Measure | Method | Pass |
| --- | --- | --- |
| Boot to desktop (PT-02) | 10 boots, median of the timed boot script | ≤ 10 s target, 15 s limit |
| Cold launch of shell apps (PT-13) | 20 timed launches per app, median | ≤ 500 ms |
| Compositor latency (PT-14) | Compositor timestamps (estimate, marked as estimate); photodiode tester if available (marked as measured) | ≤ 1 frame at 60 Hz |
| Menu, launcher, and control center open (PT-15) | 20 timed input events per panel, median | ≤ 100 ms |
| Direct scanout for full-screen apps | Compositor log shows scanout path in use | Active during full-screen video |

Results are recorded with the command, the machine, the date, and whether each value is an estimate or a measurement.

Gate definitions, telemetry, and the results log format are in `docs/2026/11/17/os-addendum-1/D6-verification-framework.md`.
