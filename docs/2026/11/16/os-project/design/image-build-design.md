# D4 — Image Build Design

Scope: how the system image is built, the size budget, the firmware policy, the disk layout, the boot chain and Secure Boot path, and update layout. Build tool choice is ADR-002 in D2.

## 1. Build Pipeline

| Step | Tool | Output |
| --- | --- | --- |
| 1. Fetch sources at pinned versions | Buildroot download cache | Source archives with checksums |
| 2. Build cross-toolchain and base packages | Buildroot | Root filesystem tree |
| 3. Apply kernel config fragment | Buildroot kernel build | `bzImage` and modules |
| 4. Add AetherOS packages (services, shell, compositor) | Buildroot external tree (`os-build/`) | Packages in rootfs |
| 5. Pack root filesystem | `mksquashfs` with zstd compression | `root.squashfs` |
| 6. Build initramfs | Dracut or Buildroot cpio | `initrd` |
| 7. Build unified kernel image | `ukify` | Signed `nexa.efi` (section 5) |
| 8. Build installer ISO | `xorriso` | `aether-<version>.iso` |
| 9. Run boot test in QEMU | Test script (04) | Pass or fail report |
| 10. Sign and publish | Signing scripts (P9-T05) | Signed image and checksums |

Rules:
- Pin every source version. Builds must be reproducible from the same inputs.
- Each step writes a log file. Failed builds keep the logs.
- Build outputs are never edited by hand.

## 2. Partition Layout

Default layout for a 64 GB disk. Sizes are proposed.

| Partition | Type | Size | Purpose |
| --- | --- | --- | --- |
| EFI System (ESP) | FAT32 | 512 MB | Boot files. Shared with Windows if present; the existing ESP is reused. |
| Root A | squashfs (read-only) | 3 GB | System image slot A |
| Root B | squashfs (read-only) | 3 GB | System image slot B (for A/B updates) |
| Data | ext4 (or Btrfs by option) | Remaining space | `/var`, `/home`, Wine prefixes |
| Swap | zram (default); optional swap file | RAM-based | Compressed memory swap (F2) |

Notes:
- Windows partitions are never moved or modified except for the shrink step in F5 INS-04.
- Data partition is encrypted when the user chooses LUKS2 (F5 INS-08).

## 3. Size Budget

Compressed sizes. Targets with limits per component. Measured on every build.

| Component | Target (MB) | Limit (MB) | Notes |
| --- | --- | --- | --- |
| Kernel and initramfs | 15 | 25 | Config fragment in `os-build/` |
| Base userland | 60 | 90 | glibc, coreutils subset, systemd minimal, shell |
| Graphics stack | 80 | 120 | Mesa for Intel and AMD, Vulkan (ANV, RADV), llvmpipe fallback |
| Firmware | 40 | 60 | Selected per reference hardware (section 4) |
| Compositor, shell (Slint), fonts | 35 | 50 | Fonts are the largest part; the Slint runtime is small |
| Installer | 35 | 50 | Installs from the ISO |
| Default apps (Text, Images, Documents, Player, Terminal, and others) | 20 | 40 | Original apps; media and PDF libraries are the largest parts (addendum F8) |
| Networking | 25 | 30 | NetworkManager, wpa_supplicant (or iwd), Bluetooth minimal |
| Audio | 12 | 20 | PipeWire with WirePlumber minimal |
| **Total** | **322** | **485** | Limit includes margin to stay under 512 MB |

Actions if a build is over limit:
1. Find the largest package with `du` on the rootfs.
2. Remove optional codecs, locales, or documentation first.
3. Move firmware to an on-demand package.
4. If still over, record the decision in D2 (ADR) and the progress log.

## 4. Firmware Policy

- Ship only firmware needed by the reference machines and the certified hardware list.
- Other firmware is available as an optional package that is installed on demand from a signed repository.
- Firmware licenses are recorded in the release notes. Some firmware blobs have redistribution terms that must be checked (P0-T03).

## 5. Boot Chain and Secure Boot

Boot chain without Secure Boot (MVP):

```
UEFI firmware -> systemd-boot (on ESP) -> nexa.efi (UKI: kernel + initramfs + command line) -> root.squashfs
```

Windows entry: systemd-boot detects `\EFI\Microsoft\Boot\bootmgfw.efi` on the same ESP and adds a menu entry.

Legacy BIOS boot (ADR-016, Personal Edition):

```
BIOS -> GRUB (in the MBR and a small boot partition) -> kernel and initramfs from the root partition
```

- The Windows entry chain-loads the Windows boot partition.
- Legacy BIOS disks use MBR. They have no EFI System Partition; the small boot partition holds the GRUB files instead.

Secure Boot path (Later, for public release; not needed for Personal Edition):

```
UEFI (Secure Boot on, with Microsoft keys) -> shim (signed by a Microsoft-signed key) -> systemd-boot -> nexa.efi (signed by our key)
```

- Shim approval depends on a review process; plan at least several weeks for it. This is a P8-T05 risk.
- Our key signs `systemd-boot` and `nexa.efi`. Keys are stored offline. Signing is part of the release pipeline (P9-T05).
- Users can enroll our key or use the shim path. Both paths are documented in F5.

## 6. Update Layout (A/B)

| Item | Design |
| --- | --- |
| Slots | Root A and Root B (section 2) |
| Update bundle | Signed full image of the new root (RAUC bundle) |
| Install | Write to inactive slot, verify checksum and signature, mark slot as next-boot |
| Health check | After boot, `aether-updater` checks that the shell, compositor, and network start within 120 s. If it passes, the slot is confirmed. |
| Rollback | If the health check fails, the bootloader returns to the previous slot |
| Data | `/var` and `/home` are not part of the slots; they survive updates |

Update size is about the size of the root image (around 1 GB uncompressed for a full root). Delta updates are a later improvement (P9 evaluation).

## 7. Reproducibility and CI

| Item | Rule |
| --- | --- |
| Source pinning | Versions listed in `os-build/versions.env` (to be created in P0-T05) |
| Build environment | Container image with fixed tool versions |
| CI on every change | Build kernel config check, build base image, size check against budget, unit tests |
| CI nightly | Full build, QEMU boot test, installer test in a VM |
| Release candidate | Full hardware checklist on reference machines (04 section 6) |
| Artifacts | Image, checksums (SHA-256), signatures, build log, size report |

## 8. Open Questions

- Whether to use Dracut or a Buildroot-generated initramfs. Decide in P1-T01.
- Whether Btrfs should be the default data filesystem (snapshots help rollback) or ext4 (simpler and smaller). Proposed: ext4 default, Btrfs option.
- Target for the Slint runtime size. Measure in spike S1 (P0-T07) and in P4.
