# Results — P2 hardware validation, run 1 (bare-metal AetherOS on the Dell)

- Date: 2026-11-26
- Machine: Dell Inspiron 5402 (i7-1165G7, Iris Xe, 22 GB, NVMe)
- Image: run 45652558559 (SSH-enabled, PARTUUID + rootwait + display fixes)
- Access: SSH `root@192.168.68.150` (key auth)
- Kernel: 6.18.0; root squashfs read-only, /var + /home tmpfs

## Passed
| Check | Evidence |
| --- | --- |
| Boot to login (display + serial) | `buildroot login:` on VGA; SSH over USB Ethernet |
| Networking | eth0 = USB Ethernet (r8152) 192.168.68.150 (static) + .112 (DHCP) |
| Graphics | i915 bound to Intel Iris Xe; `i915drmfb` primary; eDP-1 connected 1920×1080 |
| DRM connectors | card0-eDP-1 connected; HDMI-A-1/DP-1 disconnected |
| Backlight | `intel_backlight` 96000/96000 |
| Battery | UPower: 100%, 24.795 Wh, design 53.01 Wh, capacity 46.77%; sysfs BAT0 100% Full |
| Temperature | x86_pkg_temp 68 °C |
| Input devices | keyboard, lid switch, power/sleep buttons, PS/2 pointer |
| Bluetooth adapter | `hci0` up (MGMT 1.23) |
| SSH | key-based root login works |

## Gaps found (fixed in build d0e57c8)
| Gap | Cause | Fix |
| --- | --- | --- |
| Wi-Fi interface absent | ath10k built-in probed before rootfs; firmware `-2` | ath10k as module (udev loads after rootfs) + `wireless-regdb` |
| regdb missing | `cfg80211: failed to load regulatory.db` | `BR2_PACKAGE_WIRELESS_REGDB` |
| Webcam no `/dev/video` | uvcvideo not built | `MEDIA_SUPPORT`/`MEDIA_USB_SUPPORT`/`USB_VIDEO_CLASS` |
| Touchpad only PS/2 generic | no I2C-HID | `I2C_HID`/`I2C_HID_ACPI` |
| `aplay`/`speaker-test` missing | alsa-utils sub-options off | enable ALSA_UTILS_APLAY/SPEAKER_TEST/AMIXER/BAT |
| `bluetoothctl` missing | bluez5-utils client not selected | `BLUEZ5_UTILS_CLIENT`/`TOOLS`/`DEPRECATED`/`PLUGINS_AUDIO` |
| i915 DMC firmware `-2` | i915 built-in probes before rootfs | known caveat (runtime PM disabled); display works. Revisit (module or CONFIG_EXTRA_FIRMWARE) for P5 battery |

## Sleep/resume (partial)
- First `echo mem > /sys/power/state` suspended successfully, but no wake source → machine stayed suspended.
- Fix for automated test: RTC wakealarm (`/sys/class/rtc/rtc0/wakealarm` `+15`) before each suspend. 10-cycle run pending.

## Next
- Re-flash the d0e57c8 image and re-validate Wi-Fi, webcam, touchpad, audio, Bluetooth, and the 10-cycle sleep/resume.
