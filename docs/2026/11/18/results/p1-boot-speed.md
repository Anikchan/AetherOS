# Results — P1-T06/T08: boot time and base image size (D6)

- Date: 2026-11-23 (updated after green direct-kernel/BIOS runs; UEFI still failing)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Source of numbers: `buildroot-build` run 44994939865 (commit 42649c0) artifacts
  `boot-test.log` and `boot-bios.log` (seconds from QEMU start to the login
  prompt, measured by `ci-boot.exp` / `ci-boot-bios.exp`). TCG = software
  emulation on the GitHub runner; not representative of bare-metal (D6 §6).

## Base image size (P1-T06 budget: ≤ 150 MB compressed, G-RES1)

| Image | Size (MB, compressed) | Budget | Status | Run |
| --- | --- | --- | --- | --- |
| rootfs.squashfs (minimal, P1) | 14 | ≤ 150 | **Pass** | 44936414669 |
| rootfs.squashfs (with P2 additions: Mesa/Vulkan/PipeWire/firmware) | 27 | ≤ 150 | **Pass** | 45155405768 |

## Boot time (P1-T08, G-SPD0 stage budget: ≤ 10 s in VM)

| Chain | Seconds to login prompt (TCG) | Budget | Status | Run |
| --- | --- | --- | --- | --- |
| Direct kernel (QEMU, virtio) | 25 (run 44936414669) / 16 (run 45155405768) | ≤ 10 | TCG only — not the measurement target; bare-metal (Dell) pending (P2) | 44936414669 / 45155405768 |
| Legacy BIOS (GRUB2 MBR) | 30 / 17 | ≤ 10 | TCG only — bare-metal pending (P2) | same |
| UEFI (GRUB2 x86_64-efi) | 33 / 24 | ≤ 10 | **Pass** (S_UEFI_BOOT_OK) | 44936414669 / 45155405768 |

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