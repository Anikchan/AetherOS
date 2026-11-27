# Morning runbook — flash, first graphical boot, and the two-OS loop

- Date: 2026-11-28 (written ~01:10 IST, during the overnight run)
- Image: the newest green `buildroot-build` run (check the run list; the
  candidate is the one containing `1dc8598` + `d60bd7d`)
- Device: Dell Inspiron 5402, USB `SanDisk Cruzer Blade` = `/dev/sda`

## 1. Flash from Ubuntu

```sh
# on Ubuntu (the Dell booted to the internal disk)
cd ~/Downloads                     # wherever the artifact zip was unpacked
sudo os-build/scripts/flash-usb.sh aetheros-efi.img --dry-run   # check the plan
sudo os-build/scripts/flash-usb.sh aetheros-efi.img --switch    # write + verify + BootNext
```

`flash-usb.sh` refuses an internal disk (it requires a *removable USB* block
device), refuses a mounted target (a stale automount under `/run/media` is what
produced the squashfs read errors on 2026-11-27), refuses a target smaller than
the image, and verifies by reading the first `<image size>` bytes back and
comparing SHA-256 — a mismatch is a failure, not a note. `--switch` then sets
`BootNext` to the AetherOS entry, so the next reboot lands in AetherOS without
F12; AetherOS hands the boot back to the internal disk by itself.

Manual equivalent:

```sh
sudo dd if=aetheros-efi.img of=/dev/sda bs=4M conv=fsync status=progress
sync
sudo head -c 107974656 /dev/sda | sha256sum    # compare with the artifact's sha
```

## 2. Switch the next boot to the USB (no F12)

```sh
sudo aether-boot-switch aether      # from a checkout: os-build/scripts/aether-boot-switch.sh
# expect: AETHER_BOOT_SWITCH: next=<idx> label=AetherOS current=<idx> result=switched|created
sudo reboot
```

AetherOS then sets the next boot back to Ubuntu automatically (one-shot
`BootNext`), so the loop is: **Ubuntu → flash → `aether-boot-switch aether` →
reboot → AetherOS → reboot → Ubuntu**.

To keep AetherOS for a longer session:

```sh
aether-boot-switch stay             # on AetherOS; buys exactly one extra boot
# or add aether.boot=stay to the kernel command line
```

Escape hatches: if the stick is removed, the firmware ignores the missing entry
and boots Ubuntu; if AetherOS cannot write `BootNext`, it logs the failure and
leaves the table alone.

## 3. First graphical boot — what should happen

| Expectation | Where to look |
| --- | --- |
| Desktop on tty1: wallpaper, menu bar (top), dock (bottom) | the panel |
| Console login still available | Ctrl+Alt+F2 → `root` |
| SSH reachable | `ssh root@192.168.68.150` (host key changes each boot) |
| Session manifest | `/etc/aether/ui-build.txt` → `result: ok`, `compositor_features: drm` |
| Units | `systemctl status aether-session aether-shell` |
| Compositor journal | `/var/log/aether/compositor.log` → `BACKEND: drm`, `SOCKET_READY`, `CLIENT_MAPPED`, `FRAME: n=…` |

Then run the acceptance script with the Wi-Fi credentials up front (it asks
before it creates the log, so nothing typed is captured):

```sh
sh /root/test.sh "SSID" "passphrase"
# or: mount /dev/sda1 /mnt && sh /mnt/test.sh "SSID" "passphrase"
```

The `### UI / GRAPHICAL SESSION (P3/P4)` section reports the unit states, the
manifest, the three binaries, DRM connectors, compositor marker counts and both
journals.

## 4. If the screen is black or the session fails

```sh
# tty2 (Ctrl+Alt+F2) or SSH
systemctl status aether-session aether-shell --no-pager
journalctl -b -u aether-session -u aether-shell -n 80 --no-pager
cat /var/log/aether/compositor.log | tail -40
cat /etc/aether/ui-build.txt
ls -l /dev/dri
```

- `ConditionPathExists=/dev/dri` not met → the GPU driver did not bind
  (`lsmod | grep i915`, `dmesg | grep i915`).
- `aether-session` failed 3 times → `systemctl reset-failed aether-session`
  after fixing, or use `systemctl start aether-session` to retry once.
- Black screen but the session is active → check the compositor log for
  `EGL`/`GBM` errors; software rendering (softpipe) is expected on this build.
- Console recovery always works: tty2 login or SSH on `192.168.68.150`.

## 5. Known gaps in this image (not failures)

1. **Hardware GLES is software** (`kms_swrast`). Real iris/radeonsi needs
   `BR2_PACKAGE_MESA3D_LLVM` and a size-budget decision.
2. **HiDPI** renders at logical resolution and is upscaled by the software
   renderer; layout is correct, text is soft at 2×.
3. **Settings/Overview/Lock** have no layer surface yet, so those overlays are
   not painted by the host (the launcher, control centre, notifications and
   now the wallpaper are).
4. **Keymap is built-in `us`**; layout-independent keys (Tab/Enter/Esc/arrows/
   Space/Super+key) are correct, letter shortcuts are not on other layouts.
5. **Activity Manager** still launches as a CLI (its window path is a
   follow-up).

## 6. Evidence to keep

Copy back and attach to `docs/2026/11/28/results/`:

- `/var/log/aether-test.log`
- `/etc/aether/ui-build.txt`
- `cat /var/log/aether/compositor.log | head -80`
- a photo/screenshot of the desktop (the acceptance harness cannot capture the
  panel)
