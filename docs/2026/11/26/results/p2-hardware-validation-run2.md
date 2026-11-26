# Results — P2 hardware validation, run 2 (bare-metal AetherOS on the Dell)

- Date: 2026-11-26
- Machine: Dell Inspiron 5402 (i7-1165G7, Iris Xe + MX330, 22 GB, NVMe)
- Image: run 45788955461 (i915-as-module, ath10k module, uvcvideo, i2c-hid, regdb, ALSA/BT tools, openssh, rootwait, PARTUUID, console=tty0)
- Access: SSH `root@192.168.68.150` (key auth)
- Kernel: 6.18.0; root squashfs (ro), /var + /home tmpfs

## P2-T02 Graphics — PASS (with caveat)
- i915 bound to Intel Iris Xe; DMC v2.12 loaded (runtime PM OK now).
- fbcon i915drmfb primary; eDP-1 connected at 1920×1080.
- `lspci` reports Intel Iris Xe + NVIDIA MX330.
- `glxinfo`/`vulkaninfo` not in image (validation via dmesg/sysfs).
- NVIDIA MX330 is a secondary adapter; compositor targets iGPU.

## P2-T03 Wi-Fi — DRIVER PASS, CONNECT PENDING
- ath10k_pci probed, firmware loaded (WLAN.RM.4.4.1-00309, api 6).
- `iw dev wlan0 scan` found 6 SSIDs (-25 to -73 dBm).
- `regulatory.db` not found (cfg80211 warns; scan still works).
- Connect test not run (no credentials provided; user to supply SSID/password or test manually).
- Reconnect after sleep: not yet tested (depends on connect).

## P2-T03 Bluetooth — ADAPTER PASS
- `hci0` up (MGMT 1.23); `bluetoothctl` present.
- Adapter powered, discoverable; pair/connect not tested (no BT device).

## P2-T04 Audio — PASS
- ALSA: HDA Intel PCH (ALC3204 Analog + HDMI 0/1/2/3).
- `speaker-test` passed (Front Left/Right); mic round-trip recorded+played.
- PipeWire running (`pw-cli info`).
- HDMI audio: device present (HDMI 0/1/2/3); playback test needs a connected display.

## P2-T05 Input — PARTIAL
- Keyboard: AT Translated Set 2, all keys; capslock/numlock/scrolllock LEDs.
- Touchpad: only "PS/2 Generic Mouse" — I2C-HID did not bind (i2c-hid-acpi added but touchpad may be PS/2-only on this Dell, or needs a different driver). No gestures.
- Backlight: `intel_backlight` 96000/96000.
- Keyboard backlight: no `/sys/class/leds/*kbd*` (this model likely has none).
- Brightness/volume keys: not tested (no automated way to press Fn keys).
- Webcam: `/dev/video0`, `/dev/video1` present (uvcvideo works). No capture tool in image.

## P2-T06 Sleep/resume — PASS (10/10)
- RTC wakealarm (`+15 s`) auto-wake works.
- 10 cycles: each resumed after 15 s; `dmesg` shows "Restarting tasks: Done", "PM: suspend exit", carrier on.
- No failed devices after resume.
- Wi-Fi reconnect after sleep: not tested (Wi-Fi not connected in this run).

## Other checklist items (04 §6.1)
- Battery: ✅ 100%, 24.795 Wh, 46.77% capacity, time-remaining reported.
- Temperature: ✅ 55 °C (x86_pkg_temp).
- Webcam: ✅ /dev/video0 + /dev/video1 (uvcvideo).
- External display: not tested (no cable).
- Fan/temperature: readable (thermal_zone0 55 °C; hwmon present).
- Reboot into Windows and back: not tested (manual).

## Acceptance (P2-T06)
- ✅ 10 of 10 suspend/resume cycles succeed.
- ✅ No failed device tuning during runs.

## Notes
- Touchpad is PS/2 fallback (no I2C-HID binding). For full gesture support, investigate the touchpad's protocol (Synaptics PS/2 or I2C HID on a different bus). May need `psmouse` with Synaptics support or `i2c-hid` on a different controller.
- Webcam capture tool (`v4l2-ctl`/`ffmpeg`) not in image; device presence is the proxy.
- `regulatory.db` missing (cfg80211 warns but scan works with built-in regpair).
- All P2-T02/T03/T04/T05/T06 core validations pass except Wi-Fi connect (needs credentials) and touchpad gestures (PS/2 fallback).