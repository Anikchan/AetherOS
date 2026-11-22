# Results — P1-T06/T08: boot time and base image size (D6)

- Date: 2026-11-23 (S1, run 44936414669 + 44948381548)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Source of numbers: the `buildroot-build` run's `boot-test.log` (direct kernel),
  `boot-bios.log` (BIOS chain) artifacts.

## Base image size (P1-T06 budget: ≤ 150 MB compressed, G-RES1)

| Image | Size (MB, compressed) | Budget | Status |
| --- | --- | --- | --- |
| rootfs.squashfs | 14 | ≤ 150 | Pass |

## Boot time (P1-T08, G-SPD0 stage budget: ≤ 10 s in VM)

| Chain | Seconds to login prompt | Budget | Status |
| --- | --- | --- | --- |
| Direct kernel (BIOS, qemu, TCG) | 23 | ≤ 10 | Functional evidence only (TCG is slow; bare-metal numbers in P2) |
| UEFI (systemd-boot + UKI) | Blocked (P1-T05) | ≤ 10 | Blocked — kernel produces no serial output in UEFI mode |
| Legacy BIOS (GRUB2 MBR) | Pass (S_BIOS_BOOT_OK) | ≤ 10 | Functional evidence only |

## Notes
- TCG (software emulation) timings are functional evidence only — not
  representative of the bare-metal Dell target (D6 §6). The bare-metal
  numbers come from P2 hardware bring-up.
- P1-T05 (UEFI) blocked: the kernel's EFI stub loads under OVMF but
  produces no serial output. Tried systemd-boot, GRUB2, kernel EFI stub
  directly, UKI, various console params. Root cause under investigation;
  deferred to P2/P3. BIOS boot path works.
- P1 exit criteria (M1): boots in VM to login in ≤ 10 s (TCG shows 23 s,
  bare-metal expected to be faster); base image ≤ 150 MB compressed (14 MB,
  pass); UEFI and legacy BIOS boot (BIOS pass, UEFI blocked).