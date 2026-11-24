# Results — P1-T05: bare-metal boot and display (UEFI)

- Date: 2026-11-24
- Machine: Dell Inspiron 5402 (Intel i7-1165G7, Iris Xe, 22 GB RAM)
- Image: `aetheros-efi.img` from run 45412192212 (92 MB)
- D6 format: `os-build/metrics/FORMAT.md`

## Issue 1 — black screen (no display output)
- Symptom: GRUB2 menu shown, then black screen after selecting AetherOS.
- Cause: kernel had no framebuffer console; only serial getty.
- Fix: `console=tty0` + `earlycon=efi` in grub.cfg; `CONFIG_VT`, `CONFIG_VT_CONSOLE`,
  `CONFIG_FRAMEBUFFER_CONSOLE`, `CONFIG_DRM_SIMPLEDRM`, `CONFIG_DRM_FBDEV_EMULATION`,
  `CONFIG_FB_EFI` in the kernel fragment; `getty@tty1` enabled in the rootfs overlay.
- Verification: QEMU `screendump` (1280×800) shows "Welcome to Buildroot /
  buildroot login:" on the VGA framebuffer (run 45369420054 and 45412192212).

## Issue 2 — kernel panic: cannot open root device `/dev/vda2`
- Symptom on bare metal: `VFS: Cannot open root device "/dev/vda2" or unknown-block(0,0):
  error -6` → kernel panic.
- Cause: `root=/dev/vda2` hardcodes the virtio device name. On bare metal the USB stick
  is `/dev/sda` (USB storage → SCSI), so `/dev/vda2` does not exist.
- Fix: use a device-name-independent root:
  - GPT (UEFI): fixed partition UUID `9f8e7d6c-1a2b-4c3d-8e4f-5a6b7c8d9e0f` via
    `partition-uuid` in `genimage-efi.cfg`; grub.cfg `root=PARTUUID=9f8e7d6c-…`.
  - MBR (BIOS): fixed disk id `0xa37e5c21` via `sfdisk label-id`; grub.cfg
    `root=PARTUUID=a37e5c21-02`.
- Verification:
  - GPT partition 2 has `uuid=9F8E7D6C-1A2B-4C3D-8E4F-5A6B7C8D9E0F` (sfdisk -d).
  - USB `/dev/sda2` has `PARTUUID=9f8e7d6c-1a2b-4c3d-8e4f-5a6b7c8d9e0f` (blkid).
  - VM boot (virtio) reaches `buildroot login:` (serial login count 1) with the same image.

## Acceptance
- QEMU UEFI/BIOS boot: PASS.
- Display login prompt: PASS (screendump).
- Bare-metal USB boot: expected PASS via PARTUUID (owner to confirm on the Dell).
