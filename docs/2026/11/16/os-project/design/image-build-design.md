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

## 2.1 Live USB Layout (P8-T01)

The live USB is a separate, disposable image built from the same Buildroot
artifacts; it does not touch the installed-disk layout above.

| Partition | Type | Size | Purpose |
| --- | --- | --- | --- |
| AETHEROS-ESP | FAT32 | 64 MB | GRUB2 EFI (`EFI/BOOT/BOOTX64.EFI`), `bzImage`, live `grub.cfg` |
| AETHEROS-ROOT | squashfs | rootfs size | Read-only live root |
| AETHEROS-PERSIST | ext4 | 1024 MB (default) | Live overlay store (`.aether-persistence/`) |

Build:

```bash
os-build/scripts/make-live-usb.sh --binaries os-build/buildroot/output/images \
  --out os-build/buildroot/output/images/aetheros-live.img --persist-mb 1024
```

- Runs without root and without loop mounts (GPT written with `sgdisk`, FAT with
  `mkfs.vfat`/`mcopy`, ext4 populated with `mke2fs -d`, partitions copied with
  `dd`), so CI can build it.
- Partition UUIDs are fixed (`a0e50001…`/`a0e50002…`/`a0e50003…`) so `grub.cfg`
  and the docs never drift.
- The live entry passes `live=1 persistence=PARTUUID=…`; the live overlay
  service (image overlay, S1) mounts the persistence partition. Without it the
  live session runs read-only.
- Verify with `os-build/test/disk-image-test.sh <image> uefi`, then write with
  `dd if=<image> of=/dev/sdX bs=4M status=progress`.
- BIOS live boot is out of scope for P8-T01; `aetheros-bios.img` remains the P1
  legacy test image.

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

### 6.1 RAUC wiring (P9-T01, 2026-11-26)

The A/B configuration is real, not a sketch:

- `update/config/system.conf` — `compatible=aetheros`, `bootloader=grub`,
  `grubenv=/boot/grub/grubenv`, `rootfs.0`/`rootfs.1` on
  `/dev/disk/by-partlabel/aetheros-a|b` as `type=raw` with boot names `A`/`B`.
  It is embedded in `update/src/rauc.rs` (`include_str!`) and validated by
  `validate_system_conf()`.
- `update/config/manifest.raucm.tmpl` — bundle manifest (verity format, one
  rootfs image with sha256 and size).
- `os-build/scripts/rauc-bundle.sh` — renders the manifest, copies the image and
  calls `rauc bundle --cert … --key …`, then `rauc info`.
- `aether-update` — `rauc-status` parses `rauc status --output-format=json`,
  `rauc-sync` makes the bootloader authoritative, `rauc-install` writes the
  inactive slot via RAUC, `rauc-mark-good`/`rauc-mark-bad` drive the health
  handshake. Development keys are in `update/config/README.md`; production keys
  stay offline (P9-T05).

### 6.2 Rollback and health checks (P9-T02, 2026-11-26)

The boot handshake is a separate service layer (`update/src/health.rs`) on top
of the slot state machine; the write of the inactive slot itself is RAUC's job
and still needs hardware validation.

State machine (unchanged by the service):

```
A active, B empty
install/stage_pending(B)  -> B pending, update_in_progress
activate                  -> B active, boot_attempts = 0
on_boot                   -> boot_attempts += 1; > max_boot_attempts -> rollback
health pass               -> B active, A good, last_good_version = B
health fail / deadline    -> B bad, A active
```

Health service:

- Probes (`ProbeKind`): a command with a timeout, a path that must exist, a
  systemd unit that must be active, and free space on the filesystem holding a
  path (`df -Pk`). Each probe is required or optional; only required probes
  gate the report, and an empty report never confirms an update.
- Default probes: `boot-services` (`multi-user.target`), `storage` (200 MiB
  free on `/var`), `update-state` (`/var/lib/aether/update` exists), plus
  optional `socket` (`graphical.target`) and `audio` (`pipewire`). They are the
  boot service's list and can be replaced by a policy file.
- Policy (`HealthPolicy`): `deadline_secs` (120 s to confirm), `max_retries`
  (2 failed reports) and `retry_after_secs` (15 s cooldown).
- `HealthService::evaluate` returns `confirmed`, `waiting`, `rolled-back` or
  `not-pending`. The retry counter and the last attempt time live in
  `UpdateState` (`health_attempts`, `last_health_at`), so a fresh
  `aether-update health-check` process — or the next boot — continues the same
  count instead of restarting it.
- `HealthService::enforce_deadline` rolls back a slot that never confirms; the
  deadline is measured from the boot timestamp that the boot service passes in.
- `install` and `stage_pending` refuse to stage a second image while
  `update_in_progress` is set, so a second update cannot overwrite the last
  good slot before the first one is confirmed.
- CLI: `aether-update probes [--save-report FILE]` runs the default probes and
  records the report; `aether-update health-check [--report FILE] [--boot-at
  EPOCH] [--policy-file FILE]` applies the policy. A recorded report can be
  evaluated without any A/B slot, which is how the policy is tested off
  hardware.

Hardware item still open: the actual write of the inactive slot, the
`grubenv` switch and a real reboot/rollback cycle on the reference machine.


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
