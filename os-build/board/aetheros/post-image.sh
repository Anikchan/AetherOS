#!/bin/sh
# P1-T05: UEFI boot chain — GRUB2 (x86_64-efi) for UEFI disks, D4 §3.
#
# Runs as a Buildroot post-image script (env: BINARIES_DIR, TARGET_DIR,
# HOST_DIR). Produces ${BINARIES_DIR}/aetheros-efi.img:
#   p1  EFI system partition (FAT): GRUB2 + bzImage + grub.cfg
#   p2  rootfs.squashfs (read-only root, P1-T03)
# Ensure mtools (mcopy) is on PATH for genimage's vfat step.
export PATH="/usr/bin:/bin:${PATH}"

: "${BINARIES_DIR:?}" "${TARGET_DIR:?}" "${HOST_DIR:?}"
BOARD_DIR="$(dirname "$0")"

# --- Locate GRUB2 modules directory and the linux module name ---
# Ubuntu 22.04 (GRUB 2.06) provides linuxefi.mod; 24.04+ (GRUB 2.12+) provides
# linux.mod. grub-mkimage fails if a listed module does not exist, so pick the
# one that is actually present.
GRUBMOD_DIR=""
for d in "${HOST_DIR}/lib/grub/x86_64-efi" /usr/lib/grub/x86_64-efi; do
    if [ -d "$d" ]; then GRUBMOD_DIR="$d"; break; fi
done
[ -n "$GRUBMOD_DIR" ] || { echo "ERROR: GRUB2 modules directory not found"; exit 1; }

# Prefer linuxefi (GRUB 2.06 / Ubuntu 22.04) over linux (GRUB 2.12+), because
# on 2.06 the `linux` command is an alias that triggers an autoload of
# linuxefi.mod which is NOT on the ESP. Embedding linuxefi avoids the autoload.
LINUX_MOD=""
for m in linuxefi linux; do
    if [ -f "${GRUBMOD_DIR}/${m}.mod" ]; then LINUX_MOD="$m"; break; fi
done
[ -n "$LINUX_MOD" ] || { echo "ERROR: neither linuxefi nor linux module found in $GRUBMOD_DIR"; exit 1; }

# --- Stage ESP contents for genimage ---
mkdir -p "${BINARIES_DIR}/efi-part/EFI/BOOT" \
         "${BINARIES_DIR}/efi-part/boot/grub"

# GRUB2 EFI binary (standalone, with embedded modules).
if [ -f "${GRUBMOD_DIR}/grubx64.efi" ]; then
    cp "${GRUBMOD_DIR}/grubx64.efi" "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI"
else
    # Build a standalone EFI binary with grub-mkimage.
    grub-mkimage -O x86_64-efi \
        -o "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI" \
        -p /boot/grub \
        normal "$LINUX_MOD" boot part_gpt part_msdos ext2 fat iso9660 \
        configfile search search_fs_uuid terminal serial all_video efi_gop \
        echo ls cat test true sleep reboot halt help loadenv \
        2>/dev/null || \
    grub-mkimage -O x86_64-efi \
        -o "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI" \
        -p /boot/grub \
        normal "$LINUX_MOD" boot part_gpt part_msdos fat configfile search \
        2>/dev/null
fi

# Verify the GRUB2 EFI binary was actually produced.
[ -f "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI" ] && [ -s "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI" ] || {
    echo "ERROR: BOOTX64.EFI was not created (grub-mkimage failed and no grubx64.efi found)"
    exit 1
}

# Kernel + GRUB config on the ESP.
cp "${BINARIES_DIR}/bzImage" "${BINARIES_DIR}/efi-part/bzImage"
cat > "${BINARIES_DIR}/efi-part/boot/grub/grub.cfg" <<'EOF'
set timeout=3
set default=0
serial --unit=0 --speed=115200
terminal_input serial console
terminal_output serial console
menuentry "AetherOS" {
    linux /bzImage root=PARTUUID=9f8e7d6c-1a2b-4c3d-8e4f-5a6b7c8d9e0f rootfstype=squashfs earlycon=efi console=tty0 console=ttyS0,115200n8 net.ifnames=0 biosdevname=0
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