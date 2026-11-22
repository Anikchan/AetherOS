#!/bin/sh
# P1-T05: UEFI boot chain — GRUB2 (x86_64-efi) for UEFI disks.
#
# Runs as a Buildroot post-image script (env: BINARIES_DIR, TARGET_DIR,
# HOST_DIR). Produces ${BINARIES_DIR}/aetheros-efi.img:
#   p1  EFI system partition (FAT): GRUB2 + bzImage
#   p2  rootfs.squashfs (read-only root, P1-T03)
export PATH="/usr/bin:/bin:${PATH}"

: "${BINARIES_DIR:?}" "${TARGET_DIR:?}" "${HOST_DIR:?}"
BOARD_DIR="$(dirname "$0")"

# --- Stage ESP contents for genimage ---
mkdir -p "${BINARIES_DIR}/efi-part/EFI/BOOT" \
         "${BINARIES_DIR}/efi-part/boot/grub"

# GRUB2 EFI binary (from host grub-efi-amd64-bin)
GRUB_EFI="${HOST_DIR}/lib/grub/x86_64-efi/moddep.lst"
# Find grubx64.efi
GRUBX64=""
for p in "${HOST_DIR}/lib/grub/x86_64-efi"/*.efi "${HOST_DIR}/bin/grubx64.efi" /usr/lib/grub/x86_64-efi/grubx64.efi; do
    if [ -f "$p" ]; then GRUBX64="$p"; break; fi
done
# If not found, try grub-mkimage
if [ -z "$GRUBX64" ]; then
    # Use grub-mkimage to create a standalone EFI binary
    if command -v grub-mkimage >/dev/null 2>&1; then
        grub-mkimage -O x86_64-efi -o "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI" \
            -p /boot/grub \
            normal linux boot part_gpt part_msdos ext2 fat iso9660 \
            configfile search search_fs_uuid 2>/dev/null || \
        grub-mkimage -O x86_64-efi -o "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI" \
            -p /boot/grub \
            normal linux boot part_gpt part_msdos fat 2>/dev/null
    fi
else
    cp "$GRUBX64" "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI"
fi

# GRUB config
cp "${BINARIES_DIR}/bzImage" "${BINARIES_DIR}/efi-part/bzImage"
cat > "${BINARIES_DIR}/efi-part/boot/grub/grub.cfg" <<'EOF'
set timeout=3
set default=0
menuentry "AetherOS" {
    linux /bzImage root=/dev/vda2 rootfstype=squashfs console=ttyS0 net.ifnames=0 biosdevname=0
}
EOF

# --- Create ESP vfat image manually ---
export PATH="/usr/bin:/bin:${PATH}"
mkdir -p "${BINARIES_DIR}" || { echo "ERROR: cannot create ${BINARIES_DIR}"; exit 1; }
VFA="${BINARIES_DIR}/efi-part.vfat"
rm -f "${VFA}"
truncate -s 64M "${VFA}" || dd if=/dev/zero of="${VFA}" bs=1M count=64
mkdosfs -F 32 -n "AETHEROS" "${VFA}" || { echo "ERROR: mkdosfs failed"; exit 1; }
MTOOLS_SKIP_CHECK=1 mcopy -s -i "${VFA}" "${BINARIES_DIR}/efi-part/EFI" "::/"
MTOOLS_SKIP_CHECK=1 mcopy -s -i "${VFA}" "${BINARIES_DIR}/efi-part/boot" "::/"
MTOOLS_SKIP_CHECK=1 mcopy -i "${VFA}" "${BINARIES_DIR}/efi-part/bzImage" "::/"

# --- Assemble the bootable UEFI disk image ---
"${HOST_DIR}/bin/genimage" \
    --config "${BOARD_DIR}/genimage-efi.cfg" \
    --rootpath "${TARGET_DIR}" \
    --tmppath "${BINARIES_DIR}/genimage.tmp" \
    --inputpath "${BINARIES_DIR}" \
    --outputpath "${BINARIES_DIR}"
echo "post-image: aetheros-efi.img ready"