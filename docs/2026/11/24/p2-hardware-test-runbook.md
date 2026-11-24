# P2 Hardware Test Runbook — Dell Inspiron 5402 (AetherOS bare-metal)

- Date: 2026-11-24
- Machine: Dell Inspiron 5402, Intel Core i7-1165G7 (Tiger Lake), Intel Iris Xe (i915) + NVIDIA GeForce MX330, 22 GB RAM, 512 GB NVMe (Sandisk SN530)
- Wi-Fi: Qualcomm Atheros QCA6174 (ath10k_pci)
- Bluetooth: Qualcomm Atheros (USB 0cf3:e007)
- Audio: Intel Smart Sound (sof-audio-pci-intel-tgl)
- Image: `aetheros-efi.img` from run 45369420054 (confirmed boots to `buildroot login:` on the display)
- Pass criteria source: `docs/2026/11/16/os-project/04-test-and-acceptance.md` §6.1 and §11

## 0. Boot and login
1. Boot the Dell from the AetherOS USB (F12 → UEFI: Cruzer Blade).
2. GRUB2 → "AetherOS" (auto after 3 s).
3. Confirm boot messages + `buildroot login:` appear on the **display**.
4. Log in as `root` (no password).

Record: boot-to-login seconds on the display (compare to the ≤10 s target; note TCG vs bare-metal).

## 1. P2-T02 Graphics acceleration (04 §6.1)
| Test | Command | Pass |
| --- | --- | --- |
| Display native resolution | `cat /sys/class/drm/card*/modes \| head` ; `cat /sys/class/drm/card*/status` | native res/mode reported |
| i915 bound | `lspci -k \| grep -A2 VGA` ; `dmesg \| grep i915` | i915 driver bound |
| OpenGL | `glxinfo -B` (if mesa-utils present) | reports Intel Iris Xe |
| Vulkan | `vulkaninfo \| head` | reports the real GPU (ANV) |

If `glxinfo`/`vulkaninfo` are not in the image, note "tool not shipped" and rely on the i915/dmesg evidence.

## 2. P2-T03 Wi-Fi (04 §6.1)
```bash
ip link show wlp45s0
iw dev wlp45s0 scan | head -20
wpa_passphrase "YourSSID" "YourPassword" > /tmp/wpa.conf
wpa_supplicant -B -i wlp45s0 -c /tmp/wpa.conf
udhcpc -i wlp45s0
ping -c 3 8.8.8.8
```
Pass: connects and gets an IP; ping works. Reconnect after sleep (see P2-T06).

## 3. P2-T03 Bluetooth (04 §6.1)
```bash
bluetoothctl power on
bluetoothctl scan on        # wait ~5 s
bluetoothctl pair <MAC>
bluetoothctl connect <MAC>
bluetoothctl info <MAC>
```
Pass: pairs and connects; audio playback over BT (if a headset is available).

## 4. P2-T04 Audio (04 §6.1)
```bash
pw-cli info
aplay -l
speaker-test -t wav -c 2 -l 1
# HDMI audio
aplay -l | grep -i hdmi
aplay -D hw:0,3 /usr/share/sounds/alsa/Front_Center.wav 2>/dev/null
# microphone (if present)
arecord -d 3 -f cd /tmp/mic.wav && aplay /tmp/mic.wav
```
Pass: speakers, headphone jack, microphone, HDMI audio all work; PipeWire running.

## 5. P2-T05 Input (04 §6.1)
```bash
libinput list-devices
ls /sys/class/leds/ | grep -i kbd
cat /sys/class/leds/*kbd*/brightness 2>/dev/null
cat /sys/class/backlight/*/brightness
```
Manual: touchpad tap, two-finger scroll, three-finger gesture; all keyboard keys; keyboard backlight toggle; brightness keys (Fn+Up/Down); volume keys.

Pass: all checklist items work.

## 6. P2-T06 Sleep / resume (04 §6.1 — 10 of 10 cycles)
```bash
for i in $(seq 1 10); do
  echo "=== Cycle $i ==="
  echo mem > /sys/power/state
  sleep 5
  dmesg | tail -5 | grep -iE "resume|suspend" || echo "resumed (no dmesg line)"
done
```
Also test lid switch: close the lid → suspend; open → resume.

Pass: 10/10 cycles succeed; no failed devices in `dmesg` after resume; Wi-Fi reconnects after resume.

## 7. Battery (04 §6.1 / §11.2)
```bash
upower -i $(upower -e | grep battery)
cat /sys/class/power_supply/BAT0/capacity
cat /sys/class/power_supply/BAT0/status
```
Pass: percentage and time-remaining reported (not zero/“unknown”).
Battery benchmark (D5 §11.2): W1 idle 1 h, W2 scripted browsing 1 h, W3 local video 1 h — record % drop every 5 min.

## 8. Webcam (04 §6.1)
```bash
ls /dev/video*
# if v4l2-utils present:
v4l2-ctl --list-devices
# capture a frame:
ffmpeg -f v4l2 -i /dev/video0 -frames:v 1 /tmp/cam.jpg 2>/dev/null || echo "no ffmpeg"
```
Pass: webcam video works.

## 9. Fan / temperature (04 §6.1)
```bash
cat /sys/class/thermal/thermal_zone*/temp
cat /sys/class/hwmon/hwmon*/temp*_input 2>/dev/null
sensors 2>/dev/null
```
Pass: readable; fan control if supported.

## 10. External display (04 §6.1)
- Connect an HDMI/USB-C display; check hot-plug and native mode:
```bash
cat /sys/class/drm/card*-HDMI-A-1/status
```
Pass: works with hot-plug.

## 11. Reboot into Windows and back (04 §6.1)
- Reboot → GRUB/UEFI boot menu → Windows → reboot → AetherOS.
Pass: both boot.

## Recording results
Create `docs/2026/11/24/results/` files:
- `p2-t02-graphics.md`
- `p2-t03-wifi-bt.md`
- `p2-t04-audio.md`
- `p2-t05-input.md`
- `p2-t06-sleep-resume.md`
- `p2-t07-hardware-matrix.md` (final verdicts)
- `p2-t08-coverage-report.md` (re-run on AetherOS)

Use the D6 format (command, machine, date, value, pass/fail). Then mark P2 tasks Done in the tracker and update M2.

## Known notes
- Wi-Fi firmware (ath10k) is **not** in the base image (P2-T01 decision: optional). If Wi-Fi fails, copy `ath10k/QCA6174/hw3.0/*` firmware into `/lib/firmware/ath10k/QCA6174/hw3.0/` on the running system or rebuild with `BR2_PACKAGE_LINUX_FIRMWARE_ATH10K`.
- Bluetooth firmware may also be missing for the same reason.
- `glxinfo`/`vulkaninfo`/`v4l2-ctl` may not be in the minimal image; use dmesg/sysfs evidence when tools are absent.