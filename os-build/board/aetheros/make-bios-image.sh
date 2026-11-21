#!/bin/bash
# P1-T07: Legacy BIOS boot path — GRUB2 (i386-pc) for MBR disks, D4 §3.
#
# Needs root (loop device + grub-install), so it runs as a CI step, not as a
# Buildroot post-image script. Produces aetheros-bios.img:
#   p1  ext4 /boot (grub2 + bzImage), bootable, MBR label dos
#   p2  rootfs.squashfs raw content
# Usage: make-bios-image.sh <binaries_dir> <board_dir>
set -euo pipefail

BIN="${1:?binaries dir}"
BOARD="${2:?board dir}"
IMG="${BIN}/aetheros-bios.img"
BZ="${BIN}/bzImage"
ROOTFS="${BIN}/rootfs.squashfs"

# Partition geometry (512-byte sectors): p1 @2048, 64 MiB; p2 @133120.
P1_START=2048
P2_START=133120
SECTOR=512

[ -f "${BZ}" ] || { echo "ERROR: ${BZ} not found"; exit 1; }
[ -f "${ROOTFS}" ] || { echo "ERROR: ${ROOTFS} not found"; exit 1; }
command -v grub-install >/dev/null || { echo "ERROR: grub-install (grub-pc-bin) not installed"; exit 1; }

rm -f "${IMG}"
dd if=/dev/zero of="${IMG}" bs=1M count=512 status=none

# MBR partition table: p1 bootable ext4, p2 squashfs root.
sfdisk -q "${IMG}" <<EOF
label: dos
${P1_START},131072,83,*
${P2_START},,83
EOF

LO="$(sudo losetup --find --show --partscan "${IMG}")"
trap 'sudo losetup -d ${LO} 2>/dev/null || true' EXIT
sudo partprobe "${LO}"

sudo mkfs.ext4 -F -q "${LO}p1"
MNT="$(mktemp -d)"
sudo mount "${LO}p1" "${MNT}"

sudo mkdir -p "${MNT}/boot/grub"
sudo cp "${BZ}" "${MNT}/boot/bzImage"
sudo cp "${BOARD}/grub.cfg" "${MNT}/boot/grub/grub.cfg"
sudo grub-install --target=i386-pc \
    --boot-directory="${MNT}/boot" \
    --modules="biosdisk part_msdos ext2" \
    "${LO}"

sudo umount "${MNT}"
rmdir "${MNT}"
sync
sudo losetup -d "${LO}"
trap - EXIT

# Write the rootfs as partition 2 content.
dd if="${ROOTFS}" of="${IMG}" bs=${SECTOR} seek=${P2_START} conv=notrunc status=none
# Ensure the image is owned/readable by the (non-root) user that runs QEMU in CI.
# The script runs under sudo, so the file would otherwise be root-owned and the
# runner user could not open it (Permission denied).
chown --reference="${BIN}" "${IMG}" 2>/dev/null || chown "$(stat -c %u:%g "${BIN}")" "${IMG}"
chmod 644 "${IMG}"
echo "make-bios-image: ${IMG} ready"
