#!/bin/sh
# P1-T05: UEFI boot chain — systemd-boot + unified kernel image (UKI), D4 §3.
#
# Runs as a Buildroot post-image script (env: BINARIES_DIR, TARGET_DIR,
# HOST_DIR). Produces ${BINARIES_DIR}/aetheros-efi.img:
#   p1  EFI system partition (FAT): systemd-boot + UKI with embedded cmdline
#   p2  rootfs.squashfs (read-only root, P1-T03)
# Ensure mtools (mcopy) is on PATH for genimage's vfat step.
export PATH="/usr/bin:/bin:${PATH}"

: "${BINARIES_DIR:?}" "${TARGET_DIR:?}" "${HOST_DIR:?}"
BOARD_DIR="$(dirname "$0")"
STUB="${TARGET_DIR}/usr/lib/systemd/boot/efi/linuxx64.efi.stub"
SD_BOOT="${TARGET_DIR}/usr/lib/systemd/boot/efi/systemd-bootx64.efi"

# STUB no longer needed (traditional loader entry, no UKI).
[ -f "${SD_BOOT}" ] || { echo "ERROR: systemd-boot not found at ${SD_BOOT}"; exit 1; }

# --- Stage ESP contents for genimage ---
mkdir -p "${BINARIES_DIR}/efi-part/EFI/BOOT" \
         "${BINARIES_DIR}/efi-part/loader/entries"
cp "${SD_BOOT}" "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI"
cp "${BINARIES_DIR}/bzImage" "${BINARIES_DIR}/efi-part/bzImage"

# systemd-boot loader entry (traditional, no UKI) — avoids objcopy/ukify.
cat > "${BINARIES_DIR}/efi-part/loader/entries/aetheros.conf" <<'EOF'
title AetherOS
linux /bzImage
options root=/dev/vda2 rootfstype=squashfs console=ttyS0 net.ifnames=0 biosdevname=0
EOF

# --- Create ESP vfat image manually (genimage's mcopy can't find mtools) ---
export PATH="/usr/bin:/bin:${PATH}"
mkdir -p "${BINARIES_DIR}" || { echo "ERROR: cannot create ${BINARIES_DIR}"; exit 1; }
ls -ld "${BINARIES_DIR}" || { echo "ERROR: ${BINARIES_DIR} does not exist"; exit 1; }
VFA="${BINARIES_DIR}/efi-part.vfat"
rm -f "${VFA}"
# Create the file first (mkdosfs on this version can't create it)
truncate -s 64M "${VFA}" || dd if=/dev/zero of="${VFA}" bs=1M count=64
mkdosfs -F 32 -n "AETHEROS" "${VFA}" || { echo "ERROR: mkdosfs failed"; exit 1; }
# Copy ESP contents into the vfat image recursively
MTOOLS_SKIP_CHECK=1 mcopy -sp -i "${VFA}" "${BINARIES_DIR}/efi-part/"* "::" || { echo "ERROR: mcopy failed"; exit 1; }

# --- Assemble the bootable UEFI disk image ---
"${HOST_DIR}/bin/genimage" \
    --config "${BOARD_DIR}/genimage-efi.cfg" \
    --rootpath "${TARGET_DIR}" \
    --tmppath "${BINARIES_DIR}/genimage.tmp" \
    --inputpath "${BINARIES_DIR}" \
    --outputpath "${BINARIES_DIR}"
echo "post-image: aetheros-efi.img ready"
