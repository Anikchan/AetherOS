# P2-T01: Firmware list template (04 §6.2 matrix feeder)

- Status: In progress (2026-11-18, implementation-3) — tooling ready; populated at first real-machine boot.
- Tool: `os-build/test/coverage-report.sh` (P2-T08) produces the PCI/USB
  device list; this document is the firmware planning template.

## Purpose

For each device detected by the coverage report, list the firmware files
required by its driver, whether they are present in `linux-firmware`, and
whether they need to be shipped in the AetherOS base image (D4).

## Populated (Dell Inspiron 5402, first boot 2026-11-23)

| Device (lspci/lsusb) | Driver module | Firmware file(s) | In linux-firmware? | Ship in base image? | Notes |
| --- | --- | --- | --- | --- | --- |
| Intel Iris Xe (8086:9a49) | i915 | i915/tgl_dmc_ver2_12.bin, i915/tgl_guc_*.bin, i915/tgl_huc_*.bin | yes | Yes — boot-critical display | Required for full display init; DMC needed for low-power states |
| NVIDIA GeForce MX330 (10de:1d16) | nvidia | nvidia/gp108/* | yes (proprietary) | No — optional (P6/P10) | Discrete GPU; compositor targets iGPU; NVIDIA driver is optional pack |
| Intel Smart Sound (8086:a0c8) | sof-audio-pci-intel-tgl | intel/sof/sof-tgl.ri, intel/sof-tplg/* | yes | Yes — audio is core UX | Required for audio playback |
| Qualcomm QCA6174 Wi-Fi (168c:003e) | ath10k_pci | ath10k/QCA6174/hw3.0/* | yes | No — optional (P6) | Wi-Fi works after firmware install; not boot-critical |
| Qualcomm Atheros BT (0cf3:e007, USB) | btusb / ath3k | qca/* (ramps) | yes | No — optional (P6) | Bluetooth pairing needs firmware |
| Sandisk SN530 NVMe (15b7:5007) | nvme | none | n/a | No | Built-in, no firmware |
| Realtek RTL8153 USB Ethernet (0bda:8153) | r8152 | rtl_nic/rtl8153a-*.fw | yes | No — optional | USB Ethernet; firmware in linux-firmware |
| Intel Thunderbolt 4 (8086:9a13/9a1b) | thunderbolt | none (built-in) | n/a | No | Built-in |
| Intel I2C (8086:a0e8/a0e9) | intel-lpss | none | n/a | No | Built-in |
| Goodix fingerprint (27c6:538d) | — | goodix/* | yes | No — optional | Fingerprint reader; not core |
| Realtek webcam (0bda:565a) | uvcvideo | none | n/a | No | Built-in UVC |

## Decisions (updated 2026-11-23)

- **Base image ships:** i915 DMC/GuC/HuC (display), SOF audio firmware (audio). These are
  boot-critical for a usable desktop and are small.
- **Optional (not in base image, installed on demand or via P6):** ath10k (Wi-Fi),
  ath3k/btusb (Bluetooth), nvidia (discrete GPU), rtl_nic (USB Ethernet), goodix.
- **No firmware needed:** NVMe, USB xHCI, I2C, Thunderbolt, SMBus, SPI, MEI, pcieport.
- D4 (image build): add `BR2_PACKAGE_LINUX_FIRMWARE` with subset
  `BR2_PACKAGE_LINUX_FIRMWARE_I915` and `BR2_PACKAGE_LINUX_FIRMWARE_INTEL_SOF`.
  Keep total image under 150 MB (currently 15 MB, plenty of headroom).
- The NVIDIA MX330 is a secondary adapter; the compositor targets the Intel iGPU (P3).
  NVIDIA support is deferred to P10 (hardware matrix / optional packs).

## Common devices for the target hardware class (Intel i5 integrated)

| Device class | Likely driver | Firmware needed | Base image decision |
| --- | --- | --- | --- |
| Intel integrated graphics (Gen 9-12) | i915 | DMC, GuC, HuC (distro-packaged) | Yes — needed for display |
| Intel HDA audio | snd-hda-intel | none (built-in) | No |
| Intel Wi-Fi (AX200/AX201/AX210) | iwlwifi | iwlwifi-*.ucode | Optional (P6-AND defers Wi-Fi-heavy use) |
| Intel Bluetooth | btintel | ibt-*.sfi | Optional |
| Realtek Ethernet | r8169 | none (built-in) | No |
| Realtek Wi-Fi (RTL8821CE/RTL8852AE) | rtw89/rtw88 | rtw89*.bin | Optional |
| Generic USB | usbcore | none | No |
| NVMe | nvme | none | No |

## Decisions

- D4 (image build): firmware files are added via `BR2_PACKAGE_LINUX_FIRMWARE`
  with a subset selection (`BR2_PACKAGE_LINUX_FIRMWARE_INTEL` etc.) to keep
  the base image under 150 MB.
- Only firmware needed for the boot-critical path (display, basic input,
  storage) ships in the base image; optional firmware (Wi-Fi, Bluetooth) is
  installed on demand or via the installer.

## Next

- Run `os-build/test/coverage-report.sh` on the spare Dell at first boot.
- Fill the template table above.
- Update `os-build/buildroot/aetheros_defconfig` with the firmware subset
  (P2-T02 graphics + P2-T03 networking).
- Mark P2-T01 Done.
