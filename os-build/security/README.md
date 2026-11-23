# Security defaults (P9-T04)

| File | Purpose |
|---|---|
| `aether-firewall.nft` | nftables rules: deny inbound, allow established/related and outbound; DHCP/mDNS/ICMP exceptions; Unix sockets for local services. |

Apply and check:

```bash
sudo nft -c -f os-build/security/aether-firewall.nft   # syntax check
sudo nft -f os-build/security/aether-firewall.nft      # apply
sudo nft list ruleset | grep -A5 'table inet aether'
```

`aether-sandbox doctor` reports whether bubblewrap and user namespaces are
available; apps refuse to launch unsandboxed. Android/Windows packs run in
their own namespaces and do not open host ports. Remote diagnostics stay off
by default.

Verification on AetherOS: run an external port scan against the machine and
confirm only the intended replies (P9-T04 "Test ports closed"). This cannot be
verified from macOS.
