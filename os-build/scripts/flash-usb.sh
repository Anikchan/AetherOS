#!/bin/sh
# flash-usb.sh — write an AetherOS image to a USB stick, verify it, and
# optionally hand the next UEFI boot to it.
#
# Run on the machine the stick is plugged into (normally Ubuntu on the Dell):
#
#   sudo os-build/scripts/flash-usb.sh aetheros-efi.img
#   sudo os-build/scripts/flash-usb.sh aetheros-efi.img --device /dev/sda --switch
#   os-build/scripts/flash-usb.sh aetheros-efi.img --dry-run
#
# Safety rules (each one refuses rather than guessing):
#   - the image must exist and be at least 8 MiB (fat-finger guard);
#   - the target must be a *removable* block device whose sysfs entry says it is
#     USB (never an internal NVMe/SATA disk);
#   - no partition of the target may be mounted; a stale automount is the exact
#     reason the 2026-11-27 write produced squashfs read errors;
#   - the target must be at least as large as the image;
#   - after writing, the first <image size> bytes are read back and their
#     SHA-256 is compared with the image's. A mismatch is a failure, not a note.
#
# --switch sets `BootNext` to the AetherOS entry with aether-boot-switch (needs
# efibootmgr), so the next reboot lands in AetherOS without F12. AetherOS hands
# the boot back to the internal disk by itself on the following boot.

set -eu

IMAGE=""
DEVICE=""
SWITCH=0
PERSISTENT=0
DRY_RUN=0
MIN_IMAGE_BYTES=$((8 * 1024 * 1024))

usage() {
    cat <<'EOF'
usage: flash-usb.sh <image> [--device /dev/sdX] [--switch] [--dry-run]

  --device DEV  target block device (default: the only USB block device found)
  --switch      after a verified write, set BootNext to the AetherOS entry
                (one-shot: AetherOS hands the next boot back to this disk)
  --persistent  with --switch: write BootOrder instead of BootNext, so the
                choice survives a power cut (an ESP marker makes AetherOS
                restore the order on its next boot; see the boot-switch D6)
  --dry-run     validate everything and print the plan, write nothing
EOF
}

say() { printf '%s\n' "$*"; }
die() { printf 'flash-usb: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --device) [ $# -ge 2 ] || die "missing value for --device"; DEVICE="$2"; shift 2 ;;
        --device=*) DEVICE="${1#--device=}"; shift ;;
        --switch) SWITCH=1; shift ;;
        --persistent) PERSISTENT=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        -*) die "unknown option: $1" ;;
        *)
            [ -z "$IMAGE" ] || die "unexpected argument: $1"
            IMAGE="$1"; shift ;;
    esac
done

[ -n "$IMAGE" ] || { usage >&2; exit 2; }
[ -r "$IMAGE" ] || die "cannot read image: $IMAGE"

IMAGE_BYTES=$(wc -c < "$IMAGE" | tr -d ' ')
[ "$IMAGE_BYTES" -ge "$MIN_IMAGE_BYTES" ] || die "image is only $IMAGE_BYTES bytes; that is not a disk image"

# --- find the target -------------------------------------------------------
is_usb_removable() {
    dev="$1"
    name=$(basename "$dev")
    [ -e "/sys/block/$name/removable" ] || return 1
    [ "$(cat "/sys/block/$name/removable")" = "1" ] || return 1
    # Walk up to the USB device and confirm the bus.
    if [ -e "/sys/block/$name/device" ]; then
        real=$(readlink -f "/sys/block/$name/device" 2>/dev/null || true)
        case "$real" in *usb*) return 0 ;; esac
    fi
    # Some UAS bridges do not expose "usb" in the device link; accept when the
    # removable flag is set and any parent mentions usb.
    for p in /sys/block/$name/../*; do
        case "$p" in *usb*) return 0 ;; esac
    done
    return 1
}

if [ -z "$DEVICE" ]; then
    for candidate in /sys/block/sd*; do
        [ -e "$candidate" ] || continue
        dev="/dev/$(basename "$candidate")"
        if is_usb_removable "$dev"; then
            if [ -n "${FOUND:-}" ]; then
                die "more than one USB block device ($FOUND, $dev); pass --device"
            fi
            FOUND="$dev"
        fi
    done
    [ -n "${FOUND:-}" ] || die "no removable USB block device found; pass --device"
    DEVICE="$FOUND"
fi

[ -b "$DEVICE" ] || die "not a block device: $DEVICE"
is_usb_removable "$DEVICE" || die "$DEVICE does not look like a USB stick (refusing to write an internal disk)"

DEV_BYTES=$(blockdev --getsize64 "$DEVICE" 2>/dev/null || echo 0)
[ "$DEV_BYTES" -ge "$IMAGE_BYTES" ] || die "target is smaller than the image ($DEV_BYTES < $IMAGE_BYTES)"

# --- refuse a mounted target ---------------------------------------------
mounted=""
name=$(basename "$DEVICE")
for part in "/sys/block/$name/$name"*; do
    [ -e "$part" ] || continue
    pname=$(basename "$part")
    if awk -v d="/dev/$pname" '$2 == d || $1 == d { found = 1 } END { exit !found }' /proc/mounts 2>/dev/null; then
        mounted="$mounted /dev/$pname"
    fi
done
if awk -v d="$DEVICE" '$2 == d || $1 == d { found = 1 } END { exit !found }' /proc/mounts 2>/dev/null; then
    mounted="$mounted $DEVICE"
fi
[ -z "$mounted" ] || die "still mounted:$mounted — unmount it first (writing under a mount corrupted the stick on 2026-11-27)"

say "image : $IMAGE ($IMAGE_BYTES bytes)"
say "target: $DEVICE ($DEV_BYTES bytes, USB removable)"
say "sha256: $(sha256sum "$IMAGE" | cut -d' ' -f1)"

if [ "$DRY_RUN" = 1 ]; then
    say "dry-run: would write $IMAGE -> $DEVICE, sync, read back and compare"
    [ "$SWITCH" = 1 ] && say "dry-run: would then set the next boot to AetherOS ($([ "$PERSISTENT" = 1 ] && echo BootOrder || echo BootNext))"
    exit 0
fi

# --- write -----------------------------------------------------------------
say "writing (this takes a few minutes) ..."
if ! dd if="$IMAGE" of="$DEVICE" bs=4M conv=fsync status=progress; then
    die "dd failed; the stick is now incomplete — re-run before using it"
fi
sync

say "verifying ..."
IMAGE_SHA=$(sha256sum "$IMAGE" | cut -d' ' -f1)
READ_SHA=$(head -c "$IMAGE_BYTES" "$DEVICE" | sha256sum | cut -d' ' -f1)
if [ "$IMAGE_SHA" != "$READ_SHA" ]; then
    die "verification FAILED: image $IMAGE_SHA, device $READ_SHA"
fi
say "verified: $READ_SHA"

if [ "$SWITCH" = 1 ]; then
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    say "setting the next boot to AetherOS ..."
    if [ -x "$script_dir/aether-boot-switch.sh" ]; then
        if [ "$PERSISTENT" = 1 ]; then
            "$script_dir/aether-boot-switch.sh" aether --persistent
        else
            "$script_dir/aether-boot-switch.sh" aether
        fi
    elif command -v aether-boot-switch >/dev/null 2>&1; then
        if [ "$PERSISTENT" = 1 ]; then
            aether-boot-switch aether --persistent
        else
            aether-boot-switch aether
        fi
    else
        say "aether-boot-switch not found; the stick is written, use F12 for this boot"
    fi
fi

say "done. Reboot to boot AetherOS; it sets the next boot back to this disk."
