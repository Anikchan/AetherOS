# First hardware boot of the UI image (2026-11-28)

- Device: Dell Inspiron 5402, booted from the Cruzer Blade USB
- Image: artifact of build `46077087481` (`13a8fc0`), 207,261,696 bytes,
  sha256 `daf3b862af3642807db7fec77dc75c01696e52deabb62ce2347e6bbf4ebec2e4`
- Flashed with `os-build/scripts/flash-usb.sh` (read-back verified), switch armed
  with `aether-boot-switch aether`, reboot performed from Ubuntu

## What worked

| Check | Result |
| --- | --- |
| UEFI boot from the stick | ✅ AetherOS came up, SSH at `192.168.68.150` |
| Root, mounts | ✅ squashfs `/`, tmpfs `/var` `/home` `/run` |
| **Touchpad (P2-T05 root cause)** | ✅ `DLL09D9:00 04F3:3147 Mouse` in `libinput list-devices` (was `PS/2 Generic Mouse`); `gpiochip0` exists |
| Kernel modules | ✅ `cfg80211`, `mac80211`, `i2c_hid`, `i2c_hid_acpi` loaded as modules |
| USB image contents | ✅ `/usr/bin/aether-{compositor,shell,greeter}` + 11 apps (14 installed, none missing) |
| Session units | ✅ enabled (`aether-session`, `aether-shell`) |
| Boot-switch on real firmware | ✅ created entry `0006 AetherOS`, `BootNext` set, `status` reports the table |

## What failed, and why

1. **The image shipped a headless compositor.**
   `manifest: compositor_features: headless`. On the runner the
   `cargo build --features drm` failed and `post-build.sh` silently retried
   without the feature. On the device:
   ```
   AETHER_COMPOSITOR_BACKEND=drm but this binary was built without the `drm` feature
   ```
   Fix (`474acff`): the DRM build is mandatory (the image fails instead of
   falling back), the cargo error is printed, and the dev-package list gained
   `libegl-dev libgles-dev libwayland-dev libpixman-1-dev`.

2. **`aether-session` could not open its stdout.**
   ```
   Failed to set up standard output: No such file or directory
   Failed at step STDOUT spawning /usr/bin/aether-session
   ```
   `/var/log/aether` is created by `LogsDirectory=`, which is too late for
   `StandardOutput=append:/var/log/aether/compositor.log`. Fix (`474acff`):
   `ExecStartPre=/bin/mkdir -p /var/log/aether`.

3. **The capability manifest was aspirational.** The build gate failed on
   `stat`, `glxinfo`, `vulkaninfo`, `Xwayland` (none in the image by design) and
   on six names the probe does not cover. Fix (`b6c7087`): the probe list gained
   `touch/echo/dirname/basename/stty/free`, and the absent-by-design tools are
   documented instead of asserted.

4. **The first switch made AetherOS permanent.** `efibootmgr -c` prepends the
   new entry, so `BootOrder` became `0006,0005,…`; the boot-time unit then
   correctly treated "AetherOS first, no armed marker" as the owner's choice and
   left it — no return to Ubuntu. Fix (`d098dba`): creation appends the entry
   back to the end of `BootOrder` (BootNext still selects it once), with a
   selftest assertion (246 cases).

## Recovery that was verified

- tty2/SSH stayed available the whole time, exactly as designed.
- `aether-boot-switch ubuntu` on the device set `BootNext=0005 (Ubuntu)`, so the
  next reboot returns to Ubuntu despite the failed session.

## Next image must show

```
/etc/aether/ui-build.txt   → compositor_features: drm, wiring_guard: ok
systemctl is-active aether-session → active
/var/log/aether/compositor.log → BACKEND: drm, SOCKET_READY, CLIENT_MAPPED, FRAME
```
