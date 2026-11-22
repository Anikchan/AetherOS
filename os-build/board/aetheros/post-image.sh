#!/bin/sh
# P1-T05: UEFI boot chain — kernel EFI stub directly (experimental).
#
# Runs as a Buildroot post-image script (env: BINARIES_DIR, TARGET_DIR,
# HOST_DIR). Produces ${BINARIES_DIR}/aetheros-efi.img:
#   p1  EFI system partition (FAT): kernel EFI stub as BOOTX64.EFI
#   p2  rootfs.squashfs (read-only root, P1-T03)
# The kernel has CONFIG_EFI_STUB=y and CONFIG_CMDLINE set, so it boots
# with the embedded command line (root=/dev/vda2).
export PATH="/usr/bin:/bin:${PATH}"

: "${BINARIES_DIR:?}" "${TARGET_DIR:?}" "${HOST_DIR:?}"
BOARD_DIR="$(dirname "$0")"

# --- Stage ESP contents for genimage ---
mkdir -p "${BINARIES_DIR}/efi-part/EFI/BOOT"
cp "${BINARIES_DIR}/bzImage" "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI"

# --- Create ESP vfat image manually ---
export PATH="/usr/bin:/bin:${PATH}"
mkdir -p "${BINARIES_DIR}" || { echo "ERROR: cannot create ${BINARIES_DIR}"; exit 1; }
VFA="${BINARIES_DIR}/efi-part.vfat"
rm -f "${VFA}"
truncate -s 64M "${VFA}" || dd if=/dev/zero of="${VFA}" bs=1M count=64
mkdosfs -F 32 -n "AETHEROS" "${VFA}" || { echo "ERROR: mkdosfs failed"; exit 1; }
MTOOLS_SKIP_CHECK=1 mcopy -s -i "${VFA}" "${BINARIES_DIR}/efi-part/EFI" "::/"

# --- Assemble the bootable UEFI disk image ---
"${HOST_DIR}/bin/genimage" \
    --config "${BOARD_DIR}/genimage-efi.cfg" \
    --rootpath "${TARGET_DIR}" \
    --tmppath "${BINARIES_DIR}/genimage.tmp" \
    --inputpath "${BINARIES_DIR}" \
    --outputpath "${BINARIES_DIR}"
echo "post-image: aetheros-efi.img ready"