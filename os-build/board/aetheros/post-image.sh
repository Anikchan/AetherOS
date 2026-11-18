#!/bin/sh
# P1-T05: UEFI boot chain — systemd-boot + unified kernel image (UKI), D4 §3.
#
# Runs as a Buildroot post-image script (env: BINARIES_DIR, TARGET_DIR,
# HOST_DIR). Produces ${BINARIES_DIR}/aetheros-efi.img:
#   p1  EFI system partition (FAT): systemd-boot + UKI with embedded cmdline
#   p2  rootfs.squashfs (read-only root, P1-T03)
set -eu

: "${BINARIES_DIR:?}" "${TARGET_DIR:?}" "${HOST_DIR:?}"
BOARD_DIR="$(dirname "$0")"
STUB="${TARGET_DIR}/usr/lib/systemd/boot/efi/linuxx64.efi.stub"
SD_BOOT="${TARGET_DIR}/usr/lib/systemd/boot/efi/systemd-bootx64.efi"

[ -f "${STUB}" ] || { echo "ERROR: EFI stub not found at ${STUB} (need BR2_PACKAGE_SYSTEMD_BOOT)"; exit 1; }
[ -f "${SD_BOOT}" ] || { echo "ERROR: systemd-boot not found at ${SD_BOOT}"; exit 1; }

# --- UKI: kernel + cmdline in the EFI stub (cmdline fixed: vda2 is rootfs) ---
OBJCOPY=objcopy
[ -x "${HOST_DIR}/bin/objcopy" ] && OBJCOPY="${HOST_DIR}/bin/objcopy"
printf 'root=/dev/vda2 rootfstype=squashfs console=ttyS0\n' > "${BINARIES_DIR}/aetheros.cmdline"
printf 'ID=aetheros\nNAME=AetherOS\nVERSION=0.1\n' > "${BINARIES_DIR}/aetheros.osrel"
${OBJCOPY} \
    --add-section .osrel="${BINARIES_DIR}/aetheros.osrel" --change-section-vma .osrel=0x20000 \
    --add-section .cmdline="${BINARIES_DIR}/aetheros.cmdline" --change-section-vma .cmdline=0x30000 \
    --add-section .linux="${BINARIES_DIR}/bzImage" --change-section-vma .linux=0x40000 \
    --set-section-flags .osrel=data,contents --set-section-flags .cmdline=data,contents \
    --set-section-flags .linux=data,contents \
    "${STUB}" "${BINARIES_DIR}/aetheros.efi"

# --- Stage ESP contents for genimage ---
mkdir -p "${BINARIES_DIR}/efi-part/EFI/BOOT" "${BINARIES_DIR}/efi-part/EFI/Linux"
cp "${SD_BOOT}" "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI"
cp "${BINARIES_DIR}/aetheros.efi" "${BINARIES_DIR}/efi-part/EFI/Linux/aetheros.efi"

# --- Assemble the bootable UEFI disk image ---
"${HOST_DIR}/bin/genimage" \
    --config "${BOARD_DIR}/genimage-efi.cfg" \
    --rootpath "${TARGET_DIR}" \
    --tmppath "${BINARIES_DIR}/genimage.tmp" \
    --inputpath "${BINARIES_DIR}" \
    --outputpath "${BINARIES_DIR}"
echo "post-image: aetheros-efi.img ready"
