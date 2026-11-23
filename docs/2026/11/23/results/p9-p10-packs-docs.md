# P7-PK-T04 / P9-T04 / P9-T05 / P10-T03 — packs, security, checksums, guides

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run

## Verified

### P7-PK-T04 Pack G — storage analyzer

`apps/storage-analyzer` (`cargo test -p aether-storage-analyzer`: **5 tests
pass**):

- totals, per-directory rollup, largest files, symlinks not followed;
- `du -sb` comparison within 1% (gate wording from F15/P7-PK-T04);
- large-file cleanup candidates are listed, nothing is deleted.

CLI smoke:

```text
$ aether-storage-analyzer /tmp/storage-demo --top 3
Storage analyzer — /tmp/storage-demo
Total: 12.0 KB
Files: 2  Directories: 2
Largest directories: ... Largest files: ...
```

### P9-T05 image checksums

`os-build/scripts/image-checksum.sh` (S4 scripts territory):

```text
$ image-checksum.sh create /tmp/aether-test.iso
CHECKSUM_CREATED ... sha256=c3acfab8...
$ image-checksum.sh verify /tmp/aether-test.iso
CHECKSUM_OK /tmp/aether-test.iso
$ (append bytes) image-checksum.sh verify ...
CHECKSUM_MISMATCH ... (tamper detected)
```

Uses `sha256sum` or `shasum -a 256`; signing is deferred to the public release.

### P9-T04 firewall defaults

`os-build/security/aether-firewall.nft`: deny inbound, allow
established/related, DHCP/DHCPv6/mDNS/ICMP exceptions, outbound allowed. Local
services use Unix sockets, so no port is opened. `README.md` documents
`nft -c -f` validation and the external port-scan check.

**Limitation:** nftables is not available on the macOS host; the rules could
not be syntax-checked here. They must be validated on AetherOS before shipping.

### P10-T03 guides

- `docs/2026/11/23/guides/user-guide.md` — install preview, app install and
  permissions, Activity Manager, battery behaviour, aetherctl, packs,
  troubleshooting, explicit pending items.
- `docs/2026/11/23/guides/developer-guide.md` — repo map, build/test, contracts,
  adding a setting or package kind, sandbox rules, evidence rules, territories,
  release checks.

## Still pending (owner/hardware)

| Task | Blocker |
|---|---|
| P8-T01 live image / INS-AC-01 | image build + USB boot on reference machines |
| P9-T01/T02 A/B updates + rollback | RAUC integration in the image (S1 buildroot) |
| P9-T03 LUKS2 installer option | live image + disk writes |
| P9-T06 security review/fuzzing | dedicated fuzzing time; no critical findings claimed |
| P10-T01/T02/T04/T05 | hardware matrix, tuning, 30-day dogfood, release signing |
