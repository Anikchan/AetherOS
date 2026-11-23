# Results — P2-T08: Hardware coverage report (real-machine run on Dell)

- Date: 2026-11-23 (S1/S2 autonomous run on the spare Dell)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Tool: `os-build/test/coverage-report.sh` (run on the Dell, Ubuntu 22.04/26.04, kernel 7.0.0-44)
- Host: Dell Inspiron 5402, Intel Core i7-1165G7, 22 GB RAM, Intel Iris Xe + NVIDIA MX330
- Status: **Tool runs and produces a populated device table.** Coverage classification
  matches the 04 §6 checklists.

## CPU / memory

- Model: 11th Gen Intel Core i7-1165G7 @ 2.80 GHz (Tiger Lake)
- RAM: 22 GiB (2.8 used, 19 free)

## PCI devices (23 devices)

| Device | Driver | Coverage |
| --- | --- | --- |
| 00:00.0 Host bridge (Intel Tiger Lake) | — | No driver (expected, no driver needed) |
| 00:02.0 VGA (Intel Iris Xe) | i915 | **Working** |
| 00:04.0 Signal processing (Dynamic Tuning) | proc_thermal | Working |
| 00:06.0 / 00:07.0 PCI bridges | pcieport | Working |
| 00:0a.0 Telemetry aggregator | intel_vsec | Working |
| 00:0d.0 USB (Thunderbolt 4) | xhci_hcd | Working |
| 00:0d.2 USB (Thunderbolt 4 NHI) | thunderbolt | Working |
| 00:12.0 Serial (Integrated Sensor Hub) | intel_ish_ipc | Working |
| 00:14.0 USB 3.2 xHCI | xhci_hcd | Working |
| 00:14.2 Shared SRAM | — | No driver (expected) |
| 00:15.0 / 00:15.1 I2C (Serial IO) | intel-lpss | Working |
| 00:16.0 MEI | mei_me | Working |
| 00:1c.0 / 00:1c.6 PCI bridges | pcieport | Working |
| 00:1f.0 ISA bridge (LPC) | — | No driver (expected) |
| 00:1f.3 Audio (Smart Sound Tech) | sof-audio-pci-intel-tgl | **Working** |
| 00:1f.4 SMBus | i801_smbus | Working |
| 00:1f.5 SPI | intel-spi | Working |
| 01:00.0 NVMe (Sandisk SN530) | nvme | **Working** |
| 2c:00.0 3D (NVIDIA GeForce MX330) | nvidia | **Working** |
| 2d:00.0 Network (Qualcomm Atheros QCA6174 Wi-Fi) | ath10k_pci | **Working** |

## USB devices
- Realtek RTL8153 Gigabit Ethernet (USB) — Working
- Goodix fingerprint reader — present
- Realtek Integrated Webcam HD — present
- Qualcomm Atheros Bluetooth (0cf3:e007) — present (needs btusb, not shown in PCI report)

## Networking
- lo, enxa0cec8dc45e7 (USB Ethernet), wlp45s0 (Wi-Fi)

## Audio
- sof-hda-dsp: HDA Analog, HDMI1/2/3, Deep Buffer — Working (PipeWire/SOF)

## Battery
- BAT0: Full, 100%, 24.795 Wh, health Good, cycle_count 0, capacity 46.77%
- Charge control: end_threshold 90, start_threshold 50 (supported, not writable by non-root)

## Missing firmware (dmesg)
- No missing-firmware lines for bound drivers (all required firmware present on Ubuntu).

## P2-T07 hardware matrix highlights (04 §6)
- Graphics: Intel Iris Xe (i915) + NVIDIA MX330 (nvidia) — both Working.
- Wi-Fi: Qualcomm QCA6174 (ath10k_pci) — Working.
- Audio: Intel SST (sof-audio-pci-intel-tgl) — Working.
- Storage: NVMe (nvme) — Working.
- Input: I2C (intel-lpss), USB (xhci_hcd) — Working.
- Sleep/resume: not yet tested (P2-T06, needs 10 cycles).

## Acceptance (P2-T08)
- ✅ Tool runs on real hardware and produces a populated device table.
- ✅ Classification matches the 04 §6 checklists (Working/No driver; no "Needs firmware" entries).

## Notes
- "No driver" entries (host bridge, shared SRAM, ISA bridge) are expected — those
  devices need no kernel driver.
- NVIDIA MX330 is a discrete GPU; AetherOS targets Intel iGPU for the compositor
  (P3), with NVIDIA as a secondary adapter. Driver support for the NVIDIA GPU under
  the compositor is a P3/P10 concern.
- Bluetooth (Qualcomm Atheros 0cf3:e007) appears only in lsusb; needs btusb verification.
- Battery capacity at 46.77% of design — relevant for P5-T16 analytics (health trend).