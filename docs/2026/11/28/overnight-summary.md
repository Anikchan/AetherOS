# Overnight run summary — 2026-11-27 22:40 → 2026-11-28 08:00 IST

Nine parallel sessions, one image build cycle, ~1006 tests green on the
development host. This is the morning report; per-session evidence lives in
`docs/2026/11/27/results/` and `docs/2026/11/28/results/`.

## Headline

The image now **contains and starts a graphical AetherOS session** (compositor on
tty1, shell with wallpaper/menu bar/dock/panels, console login preserved on
tty2), and the laptop can **ping-pong between Ubuntu and AetherOS with no F12**
so an image can be flashed and tested by rebooting twice.

## What changed tonight

### Image and boot
| Item | State |
| --- | --- |
| Graphical session in the image | 14 binaries installed by `os-build/post-build.sh` (compositor with `--features drm`), `aether-session.service` on tty1, `aether-shell.service`, getty moved to tty2, `getty@tty1` masked |
| Runtime packages | fontconfig + DejaVu, freetype, libxkbcommon + xkb data, seatd (builtin), Mesa EGL/GLES/GBM + softpipe, `usb.ids`, `efibootmgr`, C++ toolchain (Mesa was being dropped for lack of C++) |
| WirePlumber | root cause was Lua: `BR2_PACKAGE_LUA=y` **and** `_5_4=y` (the version symbol alone is dropped) |
| Automatic boot switching | `aether-boot-switch {status,aether,ubuntu,stay,clear}` writes one-shot `BootNext`, verifies by re-reading, refuses ambiguity; `aether-boot-return.service` hands the next boot back to Ubuntu; 93-assertion selftest |
| Kernel fixes | `cfg80211=m` + `mac80211=m` + `iwlwifi=m` (regulatory.db loads after the root mount), `PINCTRL=y` + `PINCTRL_TIGERLAKE=y` (**the touchpad's root cause was no gpiochip at all**), `I2C_HID(_ACPI)=m` |
| Flash helper | `flash-usb.sh`: USB-only target, refuses mounted/small, writes with `conv=fsync`, verifies by read-back SHA-256, optional `--switch` |
| Diagnostics | `aether-support [--usb]` bundles 23 members from tty2 (units, journals, compositor log, DRM, manifest) with passphrase redaction |

### Shell and compositor
| Item | State |
| --- | --- |
| Real layer-shell client | shell connects to the compositor over a real socket (no fake journal); fixed two shipping bugs it exposed (zero-size axis without both anchors; panels taking exclusive keyboard) |
| Slint pixels | the real scene painted into layer buffers, premultiplied ARGB, change-driven; canvas 0.32 ms @1920×1080 (release) |
| Wallpaper | full-output `Background` surface, generated gradient, no black pixel (verified by sampling the PNG) |
| Nine surfaces | background, menubar, dock, launcher, control-centre, notifications + lazy `Overlay` surfaces for settings/overview/lock |
| Pacing | `wl_surface.frame` callbacks + two buffers + `wl_buffer.release`; idle shell paints once and stops |
| Keyboard | **off-by-8 keycode fixed** (every key was wrong), event loop that never read the socket fixed |
| Pointer | `wl_pointer` routed into the scene; injected dock click activates a tile end to end |
| Compositor | frame callbacks including empty frames, buffer release, pointer forwarding (plus a pre-existing DRM pointer-coordinate bug), checks 10/11 in `kms-checklist.sh` (23 checks, 6-case selftest) |

### Apps
| Item | State |
| --- | --- |
| Window backend | the 10 GUI apps enable Slint winit/Wayland per crate (softbuffer → `wl_shm`, no GPU); `--render` unchanged; Activity Manager added afterwards |
| Launcher | registry-driven model + spawn, `--launch <id>` opens the app, `--launch-check` for tests |
| Fixes | flaky `aether-files` navigation test (temp-dir collision), Terminal exit statuses/resize, Calculator scientific mode |

### Verification infrastructure
- `check-defconfig-drop.sh` — catches silent kconfig drops (8 real drops found on its first run).
- `capabilities.sh` + `check-image-capabilities.sh` — the image has no `pgrep`/`script`; scripts must declare what they use.
- `check-test-summary.sh` — a CI log with no verdict fails.
- `boot-switch-selftest.sh` (93) and `support-selftest.sh` (42) now run in CI and `ci-local.sh`.
- Full workspace on macOS: **1006 tests passed**.

## The most important finding

**Hardware GPU acceleration and XWayland were never in the image.** The defconfig
asked for `iris`, `radeonsi`, `ANV`, `RADV` (all need `BR2_PACKAGE_MESA3D_LLVM`),
`BR2_PACKAGE_XWAYLAND` and `BR2_PACKAGE_POWER_PROFILES_DAEMON` (neither is a
Buildroot 2025.11 symbol), and two udev symbols that cannot exist beside systemd.
kconfig removed all eight silently, so every earlier "graphics ✅" was KMS plus
software rendering. Now explicit; enabling hardware GL is one line plus a
size/time budget, documented in `docs/2026/11/28/results/defconfig-drops.md`.

## What is still open

1. **First boot on the Dell** — the image is built (see the build section of the
   handover message); flash with `flash-usb.sh --switch` and boot.
2. **Hardware GL** — `BR2_PACKAGE_MESA3D_LLVM=y` (owner decision).
3. **Wi-Fi connect** — `test.sh` now takes the SSID/passphrase up front; the
   passphrase is needed to close P2-T03.
4. **HiDPI** — software renderer upscales; layout correct, text soft at 2×.
5. **Settings persistence** — `/var` and `/home` are tmpfs on the read-only
   root, so settings do not survive a reboot (D4 design item).
6. **On-device checks** that cannot run headless: real page flips and pacing,
   libinput pointer focus/cursor, touchpad gestures, second display, AT-SPI,
   overlay focus return.

## Reading order for the morning

1. `docs/2026/11/28/morning-runbook.md` — flash, boot, recovery, known gaps.
2. `docs/2026/11/28/results/defconfig-drops.md` — the GPU/XWayland finding.
3. `docs/2026/11/28/results/boot-switch-ubuntu-aetheros.md` — the two-OS loop.
4. `docs/2026/11/27/results/shell-overlays-pacing.md` — what the desktop paints.
5. `docs/2026/11/27/results/ui-image-packaging.md` — what is in the image and why.
