# Results Log — P0-T06 Boot Test

Gate: P0-T06 Linux test environment (QEMU x86-64 VM, BIOS and UEFI)
Date: 2026-11-17
Machine: Apple M3 Pro (host), 18 GB RAM, macOS 27.2.1, QEMU 11.1.3 (TCG emulation)
Guest: Alpine Linux 3.24.4 "virt" x86_64 ISO (placeholder image), linux 6.18.60-0-virt
ISO SHA-256: 157d353f8e54252cad2d9812a0d56bb7fa2231fc154a110e1d180d7480ef8b4f

## Run 1 — BIOS (SeaBIOS, QEMU default firmware)

Command: `os-build/test/boot-test.sh os-build/test/images/alpine-virt-3.24.4-x86_64.iso bios 240`
Values: reached login prompt on serial console; serial log 408 lines; within the 240 s budget.
Result: **Pass**

## Run 2 — UEFI (EDK2/OVMF via pflash)

Command: `os-build/test/boot-test.sh os-build/test/images/alpine-virt-3.24.4-x86_64.iso uefi 240`
Values: reached login prompt on serial console; serial log 441 lines; within the 240 s budget.
Result: **Pass**

## Notes

- Boot tests use direct kernel boot (`-kernel`/`-initrd` from the ISO with
  `console=ttyS0`) so the serial console is observable; P1-T05 replaces this
  with the real systemd-boot/UKI boot chain.
- QEMU 11 rejects the EDK2 images via `-bios`; the script uses the supported
  pflash interface (`-drive if=pflash,readonly=on`).
- The spare x86-64 PC part of P0-T06 (real-hardware boot) is covered later by
  P2 hardware bring-up (M2), which uses the owner's machines.
- TCG emulation is slow; boot-time budgets (P1-T08) will be re-measured with
  KVM acceleration on the spare PC or a Linux CI runner.
