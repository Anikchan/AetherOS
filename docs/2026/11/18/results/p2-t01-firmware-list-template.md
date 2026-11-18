# P2-T01: Firmware list template (04 §6.2 matrix feeder)

- Status: In progress (2026-11-18, implementation-3) — tooling ready; populated at first real-machine boot.
- Tool: `os-build/test/coverage-report.sh` (P2-T08) produces the PCI/USB
  device list; this document is the firmware planning template.

## Purpose

For each device detected by the coverage report, list the firmware files
required by its driver, whether they are present in `linux-firmware`, and
whether they need to be shipped in the AetherOS base image (D4).

## Template (fill at first Dell boot)

| Device (lspci/lsusb) | Driver module | Firmware file(s) | In linux-firmware? | Ship in base image? | Notes |
| --- | --- | --- | --- | --- | --- |
| _example: Intel Wi-Fi 6 AX201_ | _iwlwifi_ | _iwlwifi-cc-a0-72.ucode_ | _yes_ | _no (P6 optional)_ | _needed for Wi-Fi_ |

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
