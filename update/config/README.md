# RAUC A/B configuration (P9-T01/T02)

This directory is the real configuration the update state machine is wired to.
`update/src/rauc.rs` embeds both files (`include_str!`) so the shipped config and
the code can never drift silently; `validate_system_conf()` and the tests fail if
a slot or boot name disappears.

## Slots

| RAUC slot | Boot name | Device | D4 name |
| --- | --- | --- | --- |
| `rootfs.0` | `A` | `/dev/disk/by-partlabel/aetheros-a` | Root A |
| `rootfs.1` | `B` | `/dev/disk/by-partlabel/aetheros-b` | Root B |

The root slots hold a raw squashfs image, so `type=raw` writes the bundle image
directly into the inactive partition. `/var` and `/home` live on the data
partition and survive updates (D4 §2, §6).

## Keys (development)

Production keys are offline (P9-T05 is deferred). For local bundles:

```bash
openssl req -x509 -newkey rsa:4096 -nodes -days 3650 \
  -keyout /etc/rauc/dev-ca.key -out /etc/rauc/dev-ca.cert.pem \
  -subj "/CN=AetherOS Development CA"
openssl req -newkey rsa:4096 -nodes \
  -keyout /etc/rauc/dev-signing.key -out /tmp/dev-signing.csr \
  -subj "/CN=AetherOS Development Signing"
openssl x509 -req -in /tmp/dev-signing.csr -CA /etc/rauc/dev-ca.cert.pem \
  -CAkey /etc/rauc/dev-ca.key -CAcreateserial -days 3650 \
  -out /etc/rauc/dev-signing.cert.pem
```

`system.conf` points `[keyring] path` at the CA certificate. Never ship the
development CA in a release image.

## Bundle

```bash
# from the build artifacts
os-build/scripts/rauc-bundle.sh \
  --image os-build/buildroot/output/images/rootfs.squashfs \
  --version 0.1.0 \
  --out out/aetheros-0.1.0.raucb
```

The script fills `manifest.raucm.tmpl`, calls `rauc bundle --cert
/etc/rauc/dev-signing.cert.pem --key /etc/rauc/dev-signing.key`, and verifies the
bundle with `rauc info`.

## Daemon wiring

`aether-update` uses the state machine for policy and RAUC for the write:

1. `rauc-sync` reads `rauc status --output-format=json`, makes the booted slot
   authoritative, and keeps a freshly booted pending slot pending.
2. `boot` counts boot attempts; the fourth unattended boot rolls back.
3. `health-pass` runs `rauc status mark-good <bootname>`; `health-fail` runs
   `rauc status mark-bad <bootname>` and returns to the last good slot.
4. `install` writes the inactive slot through `rauc install <bundle>`.

The RAUC commands are printed with `--dry-run` and executed otherwise, so the
whole flow is testable without hardware.

`update/tests/rauc_flow.rs` walks the integration: parse `rauc status` →
`stage_pending` after `rauc install` → boot the pending slot and stay pending →
`rauc status mark-good` confirms it (the old slot becomes the rollback target)
or `mark-bad` rolls back and marks the slot bad. It also pins that `system.conf`
still names both slots and that the rendered bundle manifest has no unfilled
placeholders.
