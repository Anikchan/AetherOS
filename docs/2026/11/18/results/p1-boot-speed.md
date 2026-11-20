# Results — P1-T06/T08: boot time and base image size (D6)

- Date: 2026-11-18 (implementation-3)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Source of numbers: the green `buildroot-build` run's `boot-test.log`,
  `boot-uefi.log` and `boot-bios.log` artifacts (seconds from QEMU start to
  the login prompt, measured by `ci-boot.exp` / `ci-boot-uefi.exp` /
  `ci-boot-bios.exp`).

## Base image size (P1-T06 budget: ≤ 150 MB compressed, G-RES1)

| Image | Size (MB, compressed) | Budget | Status |
| --- | --- | --- | --- |
| rootfs.squashfs | _pending CI_ | ≤ 150 | _pending_ |

## Boot time (P1-T08, G-SPD0 stage budget: ≤ 10 s in VM)

| Chain | Seconds to login prompt | Budget | Status |
| --- | --- | --- | --- |
| Direct kernel (BIOS, qemu) | _pending CI_ | ≤ 10 | _pending_ |
| UEFI (systemd-boot + UKI) | _pending CI_ | ≤ 10 | _pending_ |
| Legacy BIOS (GRUB2 MBR) | _pending CI_ | ≤ 10 | _pending_ |

## Notes

- TCG (software emulation) timings are functional evidence only — not
  representative of the bare-metal Dell target (D6 §6). The bare-metal
  numbers come from P2 hardware bring-up.
- Filled in once the definitive `buildroot-build` run lands green. **Blocked (2026-11-20):** all GitHub Actions runs fail to start with
  "recent account payments have failed or your spending limit needs to be increased" — the owner must fix billing before P1
  verification can complete. Numbers above stay `_pending_` until then; do not fabricate. S1's local P2 prep (kernel fragment +
  defconfig additions) is documented in `os-build/kernel/aether.fragment` and `os-build/buildroot/aetheros-p2-additions.defconfig`.
