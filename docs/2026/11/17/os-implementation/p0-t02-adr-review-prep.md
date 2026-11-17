# P0-T02 — ADR Review Preparation (for owner acceptance)

- Prepared by: session "implementation-1", 2026-11-17
- Source: `docs/2026/11/16/os-project/design/internal-architecture.md` section 9 (ADR table),
  `docs/2026/11/17/os-addendum-1/D7-kernel-and-macos-strategy.md`
- Status: **Prepared only.** P0-T02 is an owner-acceptance task and is NOT marked
  Done. The owner must accept or change each ADR.

## Summary of ADR statuses

| ADR | Decision | Current status | Blocked by |
| --- | --- | --- | --- |
| ADR-001 | Linux LTS kernel with custom config | Proposed | Owner acceptance |
| ADR-002 | Buildroot base image | Proposed | Owner acceptance |
| ADR-003 | systemd minimal profile as init | Proposed | P1-T02 measures size/boot time; owner acceptance |
| ADR-004 | Shell UI in Slint (Rust) | Proposed | Spike S1 (P0-T07); iced fallback if it fails |
| ADR-005 | Compositor on Smithay | Proposed | Owner acceptance; revisit if P3 blocked |
| ADR-006 | Lifecycle daemon in Rust | Proposed | Owner acceptance |
| ADR-007 | TOML config, Lua scripting | Proposed | Owner acceptance |
| ADR-008 | Flatpak first, AppImage second | Proposed | Owner acceptance |
| ADR-009 | Windows apps via Wine/Proton in `nexa-compat` | Proposed | Spike S5 (P0-T12) |
| ADR-010 | systemd-boot + signed UKI | Proposed | Secure Boot path deferred (P8) |
| ADR-011 | A/B updates with RAUC | Proposed | P9 |
| ADR-012 | Name AetherOS; prefix placeholder | **Accepted (owner, 2026-11-17)** | Trademark check deferred to P0-T03b |
| ADR-013 | Rust for all custom code | Proposed | Owner acceptance (Q7 already pointed to Rust) |
| ADR-014 | `nexa-powerd` on UPower + power-profiles-daemon | Proposed | Owner acceptance |
| ADR-015 | Personal Edition first | **Accepted (owner, 2026-11-16)** | — |
| ADR-016 | x86-64 only; legacy BIOS included | Proposed | Q8/Q5 already answered; formal acceptance pending |
| ADR-017 | Mac dev host; Linux VMs + spare PC testing | Proposed (owner answered Q9) | Formal acceptance pending |
| ADR-018 | Kernel-specific code isolated in one crate | Proposed | Owner acceptance |
| ADR-019 | Own bundle format/sandbox/store; hidden Wine engine | Proposed (owner answered Q11, Q13) | Formal acceptance pending |
| ADR-020 | Optional local AI assistant, off by default | Proposed (owner answered Q14) | Formal acceptance pending |
| ADR-021 | Android in host-kernel container; no shipped ARM translators | Proposed | Spike S6; Q18 answer |
| ADR-022 | Activity Manager is the single monitoring app | Proposed (owner answered Q20) | Formal acceptance pending |
| ADR-023 | Heavier features as optional packs | Proposed | Gate G-OPT1 |

## Proposal to the owner

The Proposed ADRs that are already backed by an owner answer (Q7 → ADR-013,
Q9 → ADR-017, Q11/Q13 → ADR-019, Q14 → ADR-020, Q20 → ADR-022) can be flipped
to Accepted in one pass. ADR-004 (Slint), ADR-009 (Wine), and ADR-021 (Android
container) should stay Proposed until their spikes (S1, S5, S6) report.

## Note on the component prefix (ADR-012)

ADR-012 says the `nexa-` placeholder prefix is renamed to `aether-` "when code
starts". Code has now started (P0-T05). The P0-T05 skeleton already uses the
`aether-` package prefix (`aether-compositor`, `aether-shell`,
`aether-lifecycled`, `aether-installer`). The plan tables still say `nexa-*`;
those should be renamed in a docs pass when the owner confirms.
