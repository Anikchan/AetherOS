# AetherOS Live USB (P8-T01)

The live USB boots AetherOS from a stick and is the carrier for the installer
later in P8. Build it from the Buildroot artifacts; nothing here changes the
installed-disk layout (D4 §2).

## Build (Linux, rootless)

```bash
# after the Buildroot build produced output/images/{bzImage,rootfs.squashfs}
# and post-image.sh produced output/images/efi-part/EFI/BOOT/BOOTX64.EFI
os-build/scripts/make-live-usb.sh \
  --binaries os-build/buildroot/output/images \
  --out os-build/buildroot/output/images/aetheros-live.img \
  --persist-mb 1024
```

Inspect the plan anywhere (macOS included) without writing a file:

```bash
os-build/scripts/make-live-usb.sh --binaries <dir> --dry-run
```

Verify the host toolchain and the input artifacts without writing anything
(the CI/checkout-friendly mode):

```bash
os-build/scripts/make-live-usb.sh --binaries <dir> --check
# LIVE_USB_CHECK_OK binaries=… rootfs=… esp=…
```

`--check` reports missing tools (`sgdisk`, `mkfs.vfat`, `mcopy`, `mke2fs`) with
the distro package names and fails when `bzImage`, `rootfs.squashfs` or
`efi-part/EFI/BOOT/BOOTX64.EFI` is absent. A full build runs
`sgdisk --verify` on the finished image, so a corrupt GPT fails the build
instead of the stick.

## Layout

| Partition | Label | Filesystem | Size |
| --- | --- | --- | --- |
| 1 | AETHEROS-ESP | FAT32 | 64 MB |
| 2 | AETHEROS-ROOT | squashfs | rootfs size |
| 3 | AETHEROS-PERSIST | ext4 | 1024 MB (configurable) |

Fixed PARTUUIDs: ESP `a0e50001-0000-4000-8000-000000000001`, root
`…0002`, persistence `…0003`. `grub.cfg` has a normal entry and a
`safe graphics` (`nomodeset`) entry.

## Verify

```bash
os-build/test/disk-image-test.sh os-build/buildroot/output/images/aetheros-live.img uefi
```

The gate (P8-T01 "boots from USB") is only closed after a real stick boots on
the Dell; QEMU proves the image chain in the meantime.

## Write to a stick

```bash
dd if=aetheros-live.img of=/dev/sdX bs=4M status=progress conv=fsync
```

Double-check the device name first: `dd` overwrites it without confirmation.

## Known gaps

- The live overlay service that mounts AETHEROS-PERSIST is image overlay work
  (S1). Without it the live session is read-only.
- BIOS live boot is not part of P8-T01.
- The installer is present in the rootfs once P8-T02 wiring lands; the live
  image already reserves the persistence space for it.
