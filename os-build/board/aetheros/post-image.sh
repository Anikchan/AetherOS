#!/bin/sh
# P1-T05: UEFI boot chain — Unified Kernel Image (UKI) via systemd-boot stub.
#
# Runs as a Buildroot post-image script (env: BINARIES_DIR, TARGET_DIR,
# HOST_DIR). Produces ${BINARIES_DIR}/aetheros-efi.img:
#   p1  EFI system partition (FAT): UKI as \EFI\BOOT\BOOTX64.EFI
#   p2  rootfs.squashfs (read-only root, P1-T03)
export PATH="/usr/bin:/bin:${PATH}"

: "${BINARIES_DIR:?}" "${TARGET_DIR:?}" "${HOST_DIR:?}"
BOARD_DIR="$(dirname "$0")"

STUB="${TARGET_DIR}/usr/lib/systemd/boot/efi/linuxx64.efi.stub"
[ -f "${STUB}" ] || { echo "ERROR: systemd-boot stub not found at ${STUB}"; exit 1; }

CMDLINE="root=/dev/vda2 rootfstype=squashfs console=ttyS0 net.ifnames=0 biosdevname=0"

# Create UKI using objcopy
UKI="${BINARIES_DIR}/aetheros-uki.efi"
cp "${STUB}" "${UKI}"

# Write cmdline to a temp file
CMDLINE_FILE="${BINARIES_DIR}/cmdline.txt"
printf '%s' "${CMDLINE}" > "${CMDLINE_FILE}"

# Add sections using objcopy
OBJCOPY=""
for c in "${HOST_DIR}/bin/x86_64-buildroot-linux-gnu-objcopy" /usr/bin/objcopy llvm-objcopy; do
    if command -v "$c" >/dev/null 2>&1 || [ -x "$c" ]; then OBJCOPY="$c"; break; fi
done
if [ -z "$OBJCOPY" ]; then
    echo "ERROR: objcopy not found"
    exit 1
fi

# Add .linux section (kernel)
$OBJCOPY --add-section .linux="${BINARIES_DIR}/bzImage" \
         --change-section-vma .linux=0x2000000 \
         "${UKI}" "${UKI}.tmp" && mv "${UKI}.tmp" "${UKI}"

# Add .cmdline section
$OBJCOPY --add-section .cmdline="${CMDLINE_FILE}" \
         --change-section-vma .cmdline=0x3000000 \
         "${UKI}" "${UKI}.tmp" && mv "${UKI}.tmp" "${UKI}"

# --- Stage ESP contents for genimage ---
mkdir -p "${BINARIES_DIR}/efi-part/EFI/BOOT"
cp "${UKI}" "${BINARIES_DIR}/efi-part/EFI/BOOT/BOOTX64.EFI"

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
echo "post-image: aetheros-efi.img ready (UKI)"