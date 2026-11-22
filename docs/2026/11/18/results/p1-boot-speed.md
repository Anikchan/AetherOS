# Results — P1-T06/T08: boot time and base image size (D6)

- Date: 2026-11-23 (updated after green direct-kernel/BIOS runs; UEFI still failing)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Source of numbers: `buildroot-build` run 44994939865 (commit 42649c0) artifacts
  `boot-test.log` and `boot-bios.log` (seconds from QEMU start to the login
  prompt, measured by `ci-boot.exp` / `ci-boot-bios.exp`). TCG = software
  emulation on the GitHub runner; not representative of bare-metal (D6 §6).

## Base image size (P1-T06 budget: ≤ 150 MB compressed, G-RES1)

| Image | Size (MB, compressed) | Budget | Status |
| --- | --- | --- | --- |
| rootfs.squashfs | 15 | ≤ 150 | **Pass** |

Evidence: `Report image sizes and check budgets` step printed
`-rw-r--r-- 1 runner runner 15M ... rootfs.squashfs` and
`base_image_compressed_mb = 14`.

## Boot time (P1-T08, G-SPD0 stage budget: ≤ 10 s in VM)

| Chain | Seconds to login prompt (TCG) | Budget | Status |
| --- | --- | --- | --- |
| Direct kernel (QEMU, virtio) | 23–24 | ≤ 10 | TCG only — not the measurement target; bare-metal (Dell) pending (P2) |
| Legacy BIOS (GRUB2 MBR) | 27 | ≤ 10 | TCG only — not the measurement target; bare-metal (Dell) pending (P2) |
| UEFI (GRUB2 x86_64-efi) | n/a (timeout, no login) | ≤ 10 | **Fail** — P1-T05 blocked; see tracker |

Notes:
- TCG timings are functional evidence only (D6 §6). The bare-metal numbers come
  from P2 hardware bring-up on the Dell.
- UEFI did not reach a login prompt in any run so far (run 44994939865 timed
  out; previous runs failed with "BdsDxe: failed to load Boot0001 … Not Found"
  or no serial output). P1-T05 is Blocked (retry budget exhausted, 2026-11-23).

## P1 exit criteria (M1) status
- Boots in a VM to login: ✅ (direct kernel + BIOS; UEFI ❌).
- Base image ≤ 150 MB compressed: ✅ (15 MB).
- UEFI and legacy BIOS boot: ❌ UEFI not yet working.

## Next
- P1-T05 UEFI debug: candidate next steps are (a) inspect the GRUB2 EFI ESP
  contents from the build artifact, (b) test on the Dell's real UEFI firmware
  (new environment, not a retry of the QEMU test), (c) try OVMF with different
  settings. Retry budget for the QEMU-based test is exhausted; real-hardware
  investigation is the recommended path now that the Dell is ready.
- P1-T08 bare-metal timings: measured in P2 on the Dell.