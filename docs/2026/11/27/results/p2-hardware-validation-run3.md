# P2 hardware validation — run 3 (Dell, AetherOS from USB)

- Date: 2026-11-27
- Session: S1 (`session - 1 (Nov27)`)
- Device: Dell Inspiron 5402, i5-1135G7 (Tiger Lake), Iris Xe + MX330, 24 GB
- Image: artifact `15150017702` written to the SanDisk Cruzer Blade, booted UEFI
- Kernel: `6.18.0` (`#1 SMP PREEMPT_DYNAMIC Thu Nov 26 19:57:10 UTC 2026`)
- Script: the previous `/root/test.sh` (pre-Wi-Fi-arguments build)
- Raw log: `docs/2026/11/27/results/logs/p2-run3-aether-test.log`

## Result summary

| Area | Result | Evidence |
| --- | --- | --- |
| Boot chain | **PASS** | `root=PARTUUID=9f8e7d6c-…` resolved, squashfs `/` read-only, tmpfs `/var` `/home` `/run`, uptime 29 s at script start |
| Graphics (P2-T02) | **PASS** | i915 bound to `00:02.0` (tigerlake/uy 9a49), DMC firmware v2.12 loaded, `card0-eDP-1` connected, `fb0 i915drmfb`, /dev/fb0 |
| Audio (P2-T04) | **PASS (ALSA), wiring gap** | ALC3204 analog + 4 HDMI devices, `speaker-test` played 2 channels, `MIC_OK` (record → play), PipeWire running (pid 271) but **WirePlumber missing** |
| Input (P2-T05) | **PARTIAL** | keyboard + `PS/2 Generic Mouse` (touchpad still not I2C-HID), backlight `96000/96000`, LEDs present |
| Battery / thermal | **PASS** | 100 %, 24.795 Wh, design capacity 46.77 %, `x86_pkg_temp` 58 °C |
| Webcam | **PASS** | `/dev/video0` + `/dev/video1` (0bda:565a) |
| Wi-Fi (P2-T03) | **PASS (driver/scan)** | ath10k QCA6174 firmware `WLAN.RM.4.4.1-00309`, scan sees `Raj_IoT`, `Raj`, `Raj_Guest`, `Airtel_Adi_anora143`… |
| Bluetooth | **PASS** | `hci0` present, controller up |
| Sleep / resume (P2-T06) | **PASS 10/10** | cycles resumed in 10–16 s, `r8152 … carrier on` after each resume, `PM: suspend exit` |
| Wi-Fi connect | **not run** | old script had no credential path |

## Problems found (with the fixes)

1. **WirePlumber is not in the image.** `systemctl is-enabled wireplumber` →
   `not-found`; the CI log says `Build wireplumber : NO`. Buildroot drops the
   entry silently because `package/wireplumber/Config.in` requires
   `BR2_PACKAGE_LUA_5_3 || BR2_PACKAGE_LUA_5_4`, and the defconfig selected
   neither. Fixed by adding `BR2_PACKAGE_LUA_5_4=y`; the next image build is the
   verification (look for `Build wireplumber : YES` and `wireplumber: running`).
2. **`pgrep` does not exist in the image** (busybox built without it), so the
   audio check reported "pipewire: not running" while `systemctl` said
   `active/enabled` and `pidof pipewire` = 271. The test now uses `pidof` with a
   `ps | grep` fallback, checks `pipewire`/`pipewire-pulse`/`wireplumber` unit
   state, and prints the connector modes through `tee` so they reach the log
   (they were printed only to the console before).
3. **`lsusb` prints "unable to initialize usb spec"** — the image has no
   `/usr/share/hwdata/usb.ids` (`BR2_PACKAGE_HWDATA` + usb.ids). Cosmetic, noted.
4. **`regulatory.db` fails to load** although `/lib/firmware/regulatory.db` and
   `.p7s` are present: `Direct firmware load for regulatory.db failed with error
   -2` during early boot, so `iw reg get` shows the world domain (`country 00`,
   passive scan on many channels). The firmware request happens before the
   squashfs root is mounted for the built-in cfg80211 path; a retry after boot or
   carrying the file in the initramfs is the likely fix. Wi-Fi scan works, but
   the regulatory domain stays world-wide.
5. **Branding:** `hostname` and the Bluetooth adapter name are `buildroot`; the
   image's `os-release` says `Buildroot 2025.11`. Owner branding territory
   (`docs/2026/11/17/branding/`), recorded, not modified.
6. **Address note:** AetherOS took a DHCP lease of `192.168.68.112` on eth0 in
   addition to the static `192.168.68.150`, i.e. the same address the Ubuntu
   install uses. Same machine, so no conflict in practice, but the Ubuntu host is
   not reachable while AetherOS is running (the `ubuntu` SSH alias now lands on
   AetherOS).

## Follow-ups

- Re-run `sh /root/test.sh "SSID" "passphrase"` after the next image build to
  close P2-T03 (connect + reconnect after suspend).
- Touchpad: still `PS/2 Generic Mouse` (I2C-HID on `00:15.0` not binding) — the
  open P2-T05 item.
- Decide whether WirePlumber's absence blocked anything user-visible yet
  (ALSA playback works; PipeWire clients need a session manager).
