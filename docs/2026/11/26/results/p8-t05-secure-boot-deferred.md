# P8-T05 — Secure Boot Path: Decision and Prerequisites (deferred)

- Session: S4 (`session - 4 (Nov26)`), 2026-11-26
- Task: roadmap P8-T05 / F5 INS-07 / D4 §5.1
- Status: **Deferred to public release** (owner decision recorded in the
  roadmap: "Deferred to public release"). No code or key material was added.

## Decision

Personal Edition ships without a Secure Boot path. The installer pre-checks
tell the user to disable Secure Boot; firmware with Secure Boot on that refuses
the unsigned loader shows the standard "security violation" and the F5
documentation explains how to turn it off. This keeps P8 (installer and dual
boot) closing on the MVP criteria that do not depend on a Microsoft-signed
chain.

Public release reopens P8-T05 with the plan below. Nothing implemented today
prevents it: the boot chain is a UKI (`aetheros.efi`) plus `systemd-boot`, both
signable without changing the partition layout.

## Current state (what exists today)

| Piece | State |
| --- | --- |
| Boot chain | `systemd-boot` on the ESP → UKI (`aetheros.efi`) with kernel + initramfs + cmdline (D4 §5) |
| Signing | None; builds are unsigned. RAUC bundles use development keys (`update/config/README.md`) |
| Installer | Pre-checks cover BitLocker/Fast Startup/battery; no Secure Boot state check |
| Firmware | Boots UEFI with Secure Boot off on the Dell; Windows keeps its own signed manager |

## Prerequisites (in dependency order)

1. **Signing key custody (P9-T05).** An offline key (HSM or air-gapped machine)
   signs `systemd-boot` and the UKI. Every release re-signs; the private key
   never enters CI. Requires an owner decision on key storage, rotation and a
   revocation/compromise procedure.
2. **Microsoft-signed `shim`.** The shim path needs an EV code-signing
   certificate and a shim-review submission (weeks of lead time). Alternative:
   shim-free, MOK enrollment at install time — simpler to ship, needs a clear
   user flow and firmware support for `mokutil`.
3. **UKI measurements and PCR policy.** Decide which PCRs gate the boot
   (PCR 7 Secure Boot state, PCR 11 UKI, PCR 14 MOK). BitLocker dual boot is
   sensitive to PCR 7 and boot-order changes: a Windows BitLocker recovery
   prompt after install is a release blocker.
4. **Kernel lockdown / IMA / module signatures.** Decide before the first
   signed build: lockdown `integrity` vs `confidentiality`, whether third-party
   modules (Wi-Fi, Windows engine drivers via DKMS) must be signed, and how
   unsigned modules fail. This is a policy decision, not just a signing step.
5. **Update interaction.** RAUC bundles stay signed independently; a signed
   boot path must still accept an A/B slot written by RAUC, and rollback must
   not depend on Secure Boot state.
6. **Revocation and recovery.** A documented recovery path when the key is
   rotated, the firmware DBX is updated, or the user loses the enrolled key
   (live USB with Secure Boot off + repair flow INS-10).

## Test matrix to run when P8-T05 starts

| # | Configuration | Expected |
| --- | --- | --- |
| 1 | Secure Boot off | Boots (today's behaviour, regression check) |
| 2 | Secure Boot on, our key enrolled via MOK | Boots with no warning |
| 3 | Secure Boot on, shim only (no MOK) | Boots through the shim review path |
| 4 | Secure Boot on, no key, no shim | Refuses, installer docs explain how to enroll/disable |
| 5 | Secure Boot on + Windows BitLocker | Both systems boot; no BitLocker recovery prompt from the install |
| 6 | Signed release, then Secure Boot disabled | Boots (signed artefacts must not require Secure Boot) |
| 7 | Update A→B under Secure Boot, health check fails | Rollback to A, still boots signed |

## Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Shim review rejected or delayed | No shim path for release | MOK enrollment as the fallback; start the submission early |
| Key compromise | All signed releases untrusted | Offline key, rotation plan, re-sign pipeline |
| BitLocker recovery prompt after install | Windows data access scare | Test matrix #5 on 3 machines before release; document recovery key |
| Lockdown policy too strict | Third-party drivers stop loading | Decide policy before signing; sign the driver set we ship |
| DBX update revokes our loader | Installed systems stop booting | Keep the recovery USB path; document how to enroll again |

## Related documents

- D4 §5 (boot chain) and §5.1 (this status).
- F5 INS-07 and INS-AC-08 (acceptance criteria remains "Not tested (Later,
  public release)").
- P9-T05 (checksums and signing pipeline), P9-T04 (firewall/sandbox defaults).
- Roadmap P8-T05 row and the P9 block.
