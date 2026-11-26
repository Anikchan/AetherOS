# P8-T01 — Live USB image build path (D6 evidence)

- Date: 2026-11-26
- Session: S4
- Gate: P8-T01 "Live image that boots from USB" — build path ready; real stick
  boot pending hardware.

## Delivered

| Part | File |
|---|---|
| Rootless GPT live image assembler | `os-build/scripts/make-live-usb.sh` |
| Runbook + layout | `installer/LIVE-USB.md` |
| Design update (D4 §2.1) | `docs/2026/11/16/os-project/design/image-build-design.md` |
| Make targets | `os-build/Makefile` (`live-usb`, `live-usb-plan`) |

Layout: p1 64 MB FAT32 ESP (GRUB2 EFI + bzImage + live `grub.cfg`), p2
`rootfs.squashfs`, p3 ext4 persistence. Fixed PARTUUIDs
`a0e50001…`/`a0e50002…`/`a0e50003…`. The build uses `sgdisk`, `mkfs.vfat` +
`mcopy`, `mke2fs -d` and `dd`, so it needs no root and no loop mounts.

## Verification on this host (macOS)

```text
$ bash -n os-build/scripts/make-live-usb.sh
$ bash os-build/scripts/make-live-usb.sh --binaries /tmp/aeth-live --dry-run
AetherOS live USB plan
  p1 ESP:     64 MiB @ sector 2048
  p2 root:    <rootfs> @ sector 133120
  p3 persist: 1024 MiB ext4 @ sector 135168
LIVE_USB_DRY_RUN output=/tmp/aeth-live/out.img
$ bash os-build/scripts/make-live-usb.sh --binaries /tmp/aeth-live --check
Missing tools: sgdisk
  Debian/Ubuntu: sudo apt-get install gdisk dosfstools mtools e2fsprogs
```

The dry run proves the arithmetic and grub command line; `--check` honestly
reports the missing Linux tools on macOS. A real build needs the Linux runner
with the Buildroot artifacts.

## Remaining work

- Build the image from real artifacts and boot it in QEMU
  (`os-build/test/disk-image-test.sh <image> uefi`).
- Boot a real stick on the Dell (hardware paused).
- Live persistence service in the image overlay (S1) and BIOS live boot (out of
  scope for P8-T01).
