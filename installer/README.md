# aether-installer — installer core (P8)

Detection, pre-checks, partition planning, boot menu rendering and uninstall
planning are pure and tested. Disk writes run only from the AetherOS live
image; this crate produces and validates the dry run.

## Commands

```bash
aether-installer detect [--json]
aether-installer hardware [--json]
aether-installer plan --disk /dev/nvme0n1 --mode alongside --root-gib 64 \
  --backup-confirmed [--json]
aether-installer plan --disk /dev/nvme0n1 --mode erase --confirm /dev/nvme0n1
aether-installer uninstall-plan --disk /dev/nvme0n1 [--grow-windows]
aether-installer apply --plan plan.json   # refused outside the live image
```

## Rules

- Alongside install needs ≥ 32 GiB free (64 GiB recommended), an existing ESP,
  and a confirmed backup. Without free space the planner proposes an NTFS
  shrink (20% of the Windows partition) and warns that the NTFS check must run.
- Erase install requires the disk name to be passed to `--confirm`.
- BitLocker without a saved recovery key, Fast Startup on, an unclean NTFS
  volume, low battery on DC, or too little space block the plan.
- The boot menu adds AetherOS and keeps the Windows Boot Manager entry.

## Honest limits

- No live image yet (P8-T01) and no disk writes; INS-AC-01/03/04/05/06/07/09
  need real hardware.
- BitLocker/Fast Startup markers are read from a mounted Windows volume; the
  full `chkdsk`/BitLocker probing on the live image is a follow-up.
- Secure Boot (P8-T05) is deferred to the public release.
