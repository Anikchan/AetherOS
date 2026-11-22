# Session S1 Plan — Critical Path (AetherOS)

- Date: 2026-11-20
- Owner: Anik
- Coordinator: this session (S1 has final merge authority)
- Tracker of record: `docs/2026/11/16/os-project/03-roadmap-plan.md` §4 (updated by S1 only)

## Territory (exclusive)

- `os-build/kernel/`
- `os-build/buildroot/`
- `os-build/test/ci-boot*.exp`
- `.github/workflows/buildroot-build.yml`
- `.github/workflows/kernel-config.yml`
- `docs/2026/11/16/os-project/03-roadmap-plan.md` §4 (tracker)
- `docs/2026/11/17/os-implementation/plan.md`

## May read (not own)

Everything.

## Must NOT touch

- `docs/.../design/`, `docs/.../features/` (S3 owns)
- `.github/workflows/` except buildroot-build.yml and kernel-config.yml (S4 owns)
- `os-build/test/spike-*`, `coverage-report.sh` (S2 owns)

## Goal

Own the build verification loop, defconfig, kernel fragment, and boot chains; drive P1 to Done and prepare P2.

## Current tasks (in sequence)

1. **P1 build verification** — **BLOCKED** on GitHub Actions billing (owner action). Re-run `kernel-config` + `buildroot-build` once billing is fixed; on green flip P1-T01/T02/T03/T04/T05/T07 → Done and write P1-T06/T08 D6 results log.
2. **P2 prep (local, no CI):** P2 kernel additions appended to `os-build/kernel/aether.fragment` (sound, cfg80211/mac80211, Bluetooth, input, sleep/resume). Done 2026-11-20.
3. **P2 prep (local, no CI):** P2 Buildroot package additions documented in `os-build/buildroot/aetheros-p2-additions.defconfig`. Merge into active defconfig only after P1 is green. Done 2026-11-20.
4. **P2-T01:** Firmware list (from coverage report data) — waits on S2 (spare Dell).
5. **P0-T10:** Battery spike (needs spare Dell — owner action, S2).

## Files this session touches

- `os-build/buildroot/aetheros_defconfig`
- `os-build/kernel/aether.fragment`
- `os-build/test/ci-boot*.exp`
- `.github/workflows/buildroot-build.yml`
- `.github/workflows/kernel-config.yml`
- `docs/2026/11/16/os-project/03-roadmap-plan.md` (tracker only)
- `docs/2026/11/17/os-implementation/plan.md`

## Status (2026-11-23, S1 P3-T01 in progress)

P3-T01 started: compositor skeleton in `compositor/` crate using headless Smithay
backend (from spike S2). Wayland core protocol, XDG shell, layer shell, SHM,
buffer commit detection. Type-checks clean on x86_64-unknown-linux-gnu. Builds
on macOS with LIBRARY_PATH=/opt/homebrew/lib. DRM/KMS, input, output deferred
to P2 (hardware bring-up, blocked on spare Dell).

P1 verification (run 44936414669 + 44948381548) completed. Results:
- P1-T01 Done (kernel config verified, run 44936411307 PASS).
- P1-T02 Done (direct kernel boot: login prompt, 23 s TCG).
- P1-T03 Done (S_MOUNTS_OK: /var and /home on tmpfs over read-only root).
- P1-T04 Done (S_DHCP_OK: DHCP lease on eth0).
- P1-T05 Blocked: UEFI boot test fails — kernel EFI stub loads under OVMF
  but produces no serial output. Tried systemd-boot, GRUB2, kernel EFI stub,
  UKI, various console params. Root cause under investigation; deferred to P2.
- P1-T06 Done: base image 14 MB compressed (≤150 MB gate pass).
- P1-T07 Done: BIOS boot test passed (S_BIOS_BOOT_OK, run 44948381548).
- P1-T08 Partial: direct boot 23 s recorded; BIOS/UEFI stage timings
  unavailable (UEFI blocked). Full stage timing in P2 (bare-metal).

M1 exit criteria: boots in VM to login ≤ 10 s (TCG 23 s, bare-metal
expected faster); base image ≤ 150 MB compressed (14 MB, pass); UEFI and
legacy BIOS boot (BIOS pass, UEFI blocked). P1 not fully complete due to
P1-T05 block. Re-estimate P2-P4 here.

Local verification performed this run:
- `cargo build --workspace` — pass (0.10s).
- `cargo test --workspace` — pass (0 tests, 0 failures).
- `os-build/kernel/aether.fragment` — all 17 P1-required options present (grep-verified), plus P2 additions (sound, cfg80211/mac80211, Bluetooth, input, sleep/resume).
- `os-build/buildroot/aetheros-p2-additions.defconfig` — created (documented-only merge).
- Boot chain scripts (`ci-boot*.exp`, `post-image.sh`, `make-bios-image.sh`, `grub.cfg`) — reviewed; internally consistent (partitions, markers, root=/dev/vda2).
- Kernel-config local validation — **PASS (performed this run)**. Base x86_64 defconfig + `aether.fragment` merged (fragment appended to `.config` then `make ARCH=x86_64 olddefconfig`, since `merge_config.sh` needs GNU sed unavailable on macOS) using `ld.lld` from the rustup toolchain as `LD` (Kconfig only needs a version-reporting linker; config step does not link). All 17 P1-required options present after resolution, plus P2 additions (SND, SND_HDA_INTEL, CFG80211, MAC80211, BT, HIBERNATION, ACPI) also resolved to `=y`. Method recorded for reproducibility; the `kernel-config` CI job remains the authoritative check once billing is restored.

Blocked tasks (recorded, not actionable by S1):
- P1 build verification (T02/T03/T04/T05/T07) — billing.
- P1-T06/T08 D6 results log — needs CI numbers.
- P2-T01 firmware list — needs S2 coverage report + design doc (S3).
- P0-T10 battery spike — needs spare Dell (S2).

No further startable tasks remain in S1's territory. Parallel sessions S2/S3/S4 have WIP present in the tree (uncommitted, in their own territories) — left untouched.

## Coordination

See shared protocol in `session-s2-plan.md` / `session-s3-plan.md` / `session-s4-plan.md` (identical protocol section) and the index `4-session-parallel-plan.md`. In short: read `plan.md` before/after work; never start a task another session marked In progress; S1 alone updates the tracker and has merge authority; conflicts favor S1.