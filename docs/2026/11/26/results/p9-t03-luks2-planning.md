# P9-T03 — Optional LUKS2 Disk Encryption in the Installer (planning)

- Session: S4 (`session - 4 (Nov26)`), 2026-11-26
- Task: roadmap P9-T03, F5 INS-08, design D4 §2.2
- Status: **planning path Done and tested; the encrypted boot is hardware-gated**
  (real `luksFormat`/`luksOpen` and an encrypted reboot need the live image on
  the reference machine).

## What was implemented

| Area | Change |
| --- | --- |
| `installer/src/planner.rs` | `LuksEncryption` (cipher, key size, PBKDF, LUKS2-only, mapper name), `PlanOptions`, `plan_with_options`, `LUKS2_HEADER_BYTES`, `LUKS_UUID_TOKEN`, three new `OperationKind`s (`create-luks-container`, `open-luks-container`, `write-crypttab`), `InstallPlan::encryption` + `set_luks_uuid`, encryption section in `render_text` |
| `installer/src/lib.rs` | Re-exports the new types/functions (`plan_install_with_options`, `PlanOptions`, `LuksEncryption`, …) |
| `installer/src/bin/aether-installer.rs` | `plan --encrypt [--luks-mapper NAME]`, `PLAN_OK … encrypted=<bool>`; passphrase never accepted from argv |
| `installer/tests/luks_plan.rs` | 5 integration tests (ordering, mapper paths, crypttab, boot command line, JSON round trip, invalid settings) |
| Docs | D4 §2.2 (plan steps + prerequisites), F5 §5.1 and INS-AC-10 |

## Plan order (the invariant the tests lock in)

```
create-partition(root)
create-luks-container  cryptsetup luksFormat --type luks2 --cipher aes-xts-plain64
                       --key-size 512 --pbkdf argon2id <root partition>
open-luks-container    cryptsetup luksOpen --type luks2 <root> <mapper>
write-crypttab         /etc/crypttab: <mapper> UUID=<LUKS-UUID> none luks,discard
format-partition       mkfs.ext4 /dev/mapper/<mapper>
write-system-image
create-swap-file
```

- Defaults: `aes-xts-plain64`, 512-bit key, `argon2id`, LUKS2, mapper
  `aether-root`. LUKS1, non-256/512 key sizes, empty cipher/PBKDF and unsafe
  mapper names are refused with `PlanError::InvalidEncryption`.
- The LUKS2 header (16 MiB) is reserved inside the root partition, so the
  ext4 filesystem is `partition - 16 MiB`; the root allocation reports both
  the container and the filesystem size.
- The AetherOS boot entry gets
  `rd.luks.name=<LUKS-UUID>=aether-root root=/dev/mapper/aether-root`; after
  `luksFormat`, `set_luks_uuid(uuid)` rewrites the command line and the
  crypttab operation, so no placeholder reaches the bootloader.
- Warnings added to the plan: recovery key shown once, initramfs
  cryptsetup/udev hooks + verified encrypted boot, hibernation resume offset.
- Unencrypted plans are bit-for-bit unchanged (`encryption: None`, same
  operations), so P8-T03/P8-T04 output and tests keep working.

## Commands and results

```
$ cargo test -p aether-installer
unittests: 28 passed
tests/fuzz_inputs.rs: 3 passed
tests/luks_plan.rs: 5 passed
→ 36 passed, 0 failed

$ cargo run -q -p aether-installer -- plan --disk /dev/nvme0n1 --encrypt --json
# not runnable on this macOS host: no /dev/nvme0n1, `detect` reports no disks.
# The same code path is covered by the integration tests above (fixture disk).
```

The CLI's `plan` subcommand needs a detected disk, so the host cannot exercise
it end to end; `plan_install_with_options` (the function the CLI calls) is
covered directly by the 5 integration tests. The 5 tests fail if the operation
order, mapper target, crypttab line, boot command line, JSON shape or
validation change.

## Not done (hardware-gated)

1. `cryptsetup`/`udev` in the initramfs (image build, S1 territory) — without
   it the encrypted root cannot be unlocked at boot.
2. One encrypted erase install and one encrypted install-beside on the Dell,
   with a wrong-passphrase and a correct-passphrase boot.
3. Recovery-key ceremony in the live installer UI (showing it once, copy to a
   file, re-entry confirmation).
4. Uninstall of an encrypted install (remove container, restore Windows
   entry) — the existing uninstall planner already treats LUKS partitions as
   `Other`/encrypted rows and only removes AetherOS rows; a targeted test is
   still to be added when the hardware run defines the real layout.

None of these change the plan format, so the hardware work can start from the
plan above.
