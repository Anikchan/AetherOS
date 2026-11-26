# P9-T01/T02 — RAUC A/B wiring (D6 evidence)

- Date: 2026-11-26
- Session: S4
- Gate: P9-T01 "A/B updates with RAUC" — state machine wired to the real
  config; bundle install/mark on hardware pending.

## Delivered

| Part | File |
|---|---|
| Shipped A/B config | `update/config/system.conf` |
| Bundle manifest template | `update/config/manifest.raucm.tmpl` |
| Key + bundle runbook | `update/config/README.md` |
| Status parsing, sync, commands | `update/src/rauc.rs` |
| Bundle builder | `os-build/scripts/rauc-bundle.sh` |
| CLI | `update/src/bin/aether-update.rs` |

`system.conf` uses `compatible=aetheros`, `bootloader=grub`,
`rootfs.0`/`rootfs.1` on `/dev/disk/by-partlabel/aetheros-a|b` as `type=raw`
with boot names `A`/`B`. It is embedded in the crate with `include_str!` and
validated by `validate_system_conf()`, so config and code cannot drift.

## Verification

```text
$ cargo test -p aether-update
18 passed; 0 failed

$ aether-update rauc-config
... [slot.rootfs.1] ...
RAUC_CONFIG_OK

$ aether-update rauc-status --rauc-json /tmp/rauc-status.json
{"compatible":"aetheros","booted":"b","slots":[{"id":"a","bootname":"A",...}]}

$ aether-update rauc-sync --rauc-json /tmp/rauc-status.json --state /tmp/upd-state.json
RAUC_SYNC booted=B active=B changed=true

$ aether-update rauc-mark-good --state /tmp/upd-state.json --dry-run
RAUC_CMD rauc status mark-good B
HEALTH_OK active=B

$ aether-update bundle-manifest --version 1.2.0 --image /tmp/rootfs.squashfs
[update] compatible=aetheros version=1.2.0
[image.rootfs] filename=rootfs.squashfs sha256=… size=…

$ bash os-build/scripts/rauc-bundle.sh --image /tmp/rootfs2.squashfs --version 0.1.0 --dry-run
RAUC_BUNDLE_DRY_RUN out=out/aetheros-0.1.0.raucb
```

Unit tests cover: RAUC JSON parsing (`rootfs.0` key form and boot-name form),
bootloader-authoritative sync, pending slot staying pending across sync, bad
slot mapping, command argv, config validation and manifest rendering.

## Honest limits

- `rauc status`/`rauc install` against a real slot and the GRUB env switch need
  AetherOS on the Dell; the commands are exercised with JSON fixtures and
  `--dry-run`.
- Signing uses the development CA documented in `update/config/README.md`;
  production keys stay offline (P9-T05 deferred).
