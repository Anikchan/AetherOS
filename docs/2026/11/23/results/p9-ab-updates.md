# P9-T01/T02 — A/B update slots, health check, rollback

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run
- Scope: `update/`

## Verified

```bash
cargo test -p aether-update     # 9 tests pass
```

| Check | Test |
|---|---|
| Pending image is written to the inactive slot | install → slot B `Pending`, active stays A |
| Checksum mismatch stops the update | manifest hash ≠ image hash → error, slot unchanged |
| Bad signature stops the update | signed manifest + invalid signature → error |
| Downgrades are refused | 0.9.0 over 1.0.0 → error; `min_version` also enforced |
| Boot attempts roll back | 4th boot on a pending slot → active returns to A, B becomes `Bad` |
| Health pass confirms | active becomes `Active`, previous becomes `Good` |
| Health fail rolls back | active returns to the good slot |
| No good slot | rollback returns `NoRollbackTarget` |
| Persistence | state JSON round trip |
| Version compare | numeric (`1.10` > `1.9`) |

CLI transcript:

```text
$ aether-update install --manifest manifest.toml --image image.img
STAGED 1.1.0 checksum=25c18ace...
$ aether-update activate
ACTIVATED B
$ aether-update boot   (three times)
BOOT_OK attempts=1 / 2 / 3
$ aether-update boot
ROLLED_BACK active=A
```

## Limits

- RAUC performs the actual image write and bootloader switch; this module is
  the slot/health/rollback state machine that RAUC integration will call. The
  RAUC wiring in the image (S1 Buildroot) is still pending P9-T01.
- Signature validation is a boolean supplied by the caller; the signing
  pipeline is deferred to the public release (P9-T05 signed path).
- Real boot-failure detection needs the boot service and hardware.

## Reproduce

```bash
cargo test -p aether-update
aether-update status --state /tmp/state.json
```
