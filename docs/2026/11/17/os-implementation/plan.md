# Implementation Session Plan — os-implementation

- Session: implementation-1
- Date: 2026-11-17
- Working directory: `/Users/anik004/Anik/Work/scripts/my_applications/my_os`
- Tracker of record: `docs/2026/11/16/os-project/03-roadmap-plan.md` section 4 (Progress Tracker)

## Goal

Start the implementation per the roadmap, keeping the progress tracker and this
plan updated at runtime (before and after each task) so any parallel session can
resume without duplication.

## Checklist

| Task | State | Notes |
| --- | --- | --- |
| P0-T05 Repository layout and CI skeleton | **Done (2026-11-17)** | Cargo workspace: `aether-compositor`, `aether-shell`, `aether-lifecycled`, `aether-installer`; `apps/`, `os-build/` placeholders; GitHub Actions CI. Verified: `cargo build --workspace` and `cargo test --workspace` pass (4/4). Rust 1.100.0 installed via Homebrew. Git repo initialized on `main`; CI runs on first push (no remote yet). |
| P0-T03 Legal notes | **Done (2026-11-17)** | Licenses verified via researcher (cited web research) and recorded in 02 section 3.1. Key rules: never ship genuine Windows DLLs with Wine; Slint royalty-free path needs attribution (About screen or download-page badge); keep H.264/AAC out of any publicly distributed image. Public-release re-check stays in P0-T03b. |
| P0-T06 Linux test environment | **Done (2026-11-17)** | QEMU 11.1.3 + sevenzip installed via Homebrew. `os-build/test/boot-test.sh` boots the Alpine 3.24.4 x86-64 placeholder ISO to the login prompt in BIOS (SeaBIOS) and UEFI (EDK2 pflash; QEMU 11 rejects edk2 via `-bios`) modes. Results (D6 format): `docs/2026/11/17/results/p0-t06-boot-test.md`. Spare-PC validation moves to P2. |
| P0-T04 Hardware list | **Done (2026-11-18)** | 04 section 6.2: Reference 1 = spare Dell laptop (i5, integrated graphics, 24 GB RAM, Windows installed; primary OS test machine); dev host recorded. Device details pending P2-T08. |
| Remote / CI | **Done (2026-11-18)** | `origin` = `https://github.com/Anikchan/AetherOS.git`; `main` pushed and tracking. Owner should confirm the Actions run (private repo — anonymous API check returns 404). |
| P1-T01 Buildroot config + kernel fragment | **In progress** | `os-build/kernel/aether.fragment`, `os-build/buildroot/aetheros_defconfig` + README, `kernel-config` CI workflow. First Buildroot build pending in the Linux VM. |
| P0-T02 ADR review | **Done (2026-11-17)** | Owner accepted ADR-001 to ADR-023: Groups 1 and 2 as proposed; ADR-004/009/021 accepted with spike safeguards (S1/S5/S6 still validate; fallbacks unchanged). Statuses updated in D2 section 9; D7 kernel note updated. |
| P0-T07 Spike S1 (Slint layer-shell + blur) | Pending | Needs a Linux/Wayland session — run inside the QEMU VM or a Linux environment; VM has no Wayland compositor session yet. Slint evaluation can start on macOS headless partially, but layer-shell requires Linux. |
| P0-T08 Spike S2 (Smithay compositor) | Pending | Needs Linux (DRM/KMS); run in VM or on the spare PC. |
| P0-T09 Spike S3 (cgroup.freeze, memory.reclaim, PSI) | **Done, partial (2026-11-18)** | Freeze Pass, reclaim Pass (~40 MB reclaimed). PSI absent from the Alpine virt kernel (CONFIG_PSI not built; `psi=1` has no effect) — re-test folded into P1-T01. Report: `docs/2026/11/18/results/p0-t09-spike-s3.md`. 2 harness retries used (frozen-cgroup reuse; TCG-too-slow hog); marker hygiene fixed. |
| P0-T10 Spike S4 (UPower/sysfs battery) | Pending | Needs a machine with a battery (spare PC or a laptop VM passthrough). |
| P0-T11 Verification setup (budgets.toml) | **Done (2026-11-17)** | `os-build/budgets.toml`, `os-build/metrics/FORMAT.md`, `os-build/check-budgets.py`, CI step added. Verified: pass case exit 0, over-limit case exit 1, full build+test+budget green. |
| P0-T12 Spike S5 (Wine in VM) | Pending | Needs the Linux VM with X/Wayland; heavy. |

## Next steps for this session

1. P1-T01: first real Buildroot build in the Linux VM (clone Buildroot into
   `os-build/buildroot/`, run `make`), then G-RES1/base-size check — needs a
   persistent VM disk or a GitHub Actions build job.
2. P0-T07/T08 spikes need a graphical Linux environment — set up in the VM
   (software rendering + Wayland) or defer to the spare PC; owner decision.
3. P0-T10 (battery) and P0-T12 (Wine) spikes need real hardware / graphical VM.
4. Owner: confirm the GitHub Actions run is green (private repo — not
   verifiable from the host).

## Working rules applied

- Roadmap section 6: task marked In progress in the tracker before work; will be
  marked Done with a progress-log line only after verification.
- No task owned by another session will be touched.

## Progress log (this session)

- 2026-11-17: P0-T05 marked In progress; repository skeleton created; Rust
  toolchain installation started (Rust 1.100.0 via Homebrew; build and tests
  pass). P0-T05 Done; commit on `main`.
- 2026-11-17: P0-T03 and P0-T06 marked In progress; license research launched;
  QEMU install started.
- 2026-11-17: P0-T03 Done (02 section 3.1 updated with verified SPDX licenses
  and rules). P0-T06 Done: BIOS and UEFI boot tests pass (two retries on the
  UEFI path: hdiutil mount failed → switched to sevenzip extraction; QEMU 11
  rejects edk2 via `-bios` → switched to pflash). Results recorded in D6
  format.
- 2026-11-17: P0-T11 Done: budgets.toml + check-budgets.py + metrics format +
  CI step; pass and fail cases verified locally. Commit on `main`.
- 2026-11-17: P0-T02 Done: owner accepted all ADRs (ADR-001 to ADR-023) in
  D2 section 9; spike safeguards annotated on ADR-003/004/009/021; D7 note
  updated. Commit on `main`.
- 2026-11-18: P0-T09 Done (partial): spike S3 harness (`spike-s3.sh` +
  `spike-s3.exp`) run three times; freeze and memory.reclaim pass with real
  numbers; PSI absent from the Alpine virt kernel (finding recorded,
  re-test in P1-T01). Report at `docs/2026/11/18/results/p0-t09-spike-s3.md`.
  Session name for the tracker is "implementation-2" (new day).
- 2026-11-18: P0-T04 Done: hardware matrix recorded in 04 section 6.2 —
  Reference 1 is the spare Dell laptop (Intel i5, integrated graphics,
  24 GB RAM, Windows installed; primary OS test machine); dev host (Apple
  M3 Pro) recorded as VM-only. Device-level details pending P2-T08.
- 2026-11-18: Remote added (`https://github.com/Anikchan/AetherOS.git`),
  `main` pushed and tracking. CI runs remotely; anonymous API check returns
  404 (private repo), so the owner should confirm the Actions run — the
  workflow commands are the same ones verified locally.
- 2026-11-18: P1-T01 In progress: `os-build/kernel/aether.fragment` (PSI per
  spike S3 finding, cgroup v2/freezer, zram, squashfs/ext4/Btrfs, EFI stub,
  DRM Intel/AMD, tickless, virtio, binder) and
  `os-build/buildroot/aetheros_defconfig` + README drafted;
  `kernel-config` CI workflow added to validate the fragment on every push.
  First full Buildroot build pending in the Linux VM.
- 2026-11-18: Session resumed after server restart; all work verified intact
  (`main` pushed to `dccc5d8`, clean tree). P0 phase row updated for P0-T04
  Done and the pushed remote. Commit and push.
- 2026-11-18: Owner decisions: graphical spikes (P0-T07/T08) will run in the
  VM; a GitHub Buildroot build job was approved. Added
  `.github/workflows/buildroot-build.yml` (pinned Buildroot 2025.11, full
  build, image-size gate ≤ 150 MB, artifact upload); defconfig layout aligned
  to it. P1-T01 stays In progress until the first CI/VM build is green.
- 2026-11-18: Owner confirmed both CI jobs green → P1-T01 Done (kernel
  builds; base image inside the 150 MB gate). Project AGENTS.md created
  (autonomous development is now this project's default instruction).
- 2026-11-18: P1-T02/T03/T04 In progress: `ci-boot.exp` boots the built
  image in CI and verifies login prompt, tmpfs /var and /home over the
  read-only squashfs root, and DHCP on eth0; `init-compare` workflow
  (dispatch-only) added for the ADR-003 init comparison; boot-seconds
  measurement added for P1-T08/G-SPD0. Committed and pushed (`5e52bc5`);
  evidence pending owner confirmation of the CI run.
- 2026-11-18: Next in sequence after T02–T04 confirm green: P1-T05 (UKI +
  systemd-boot ESP boot chain — needs a genimage/ESP disk-image session),
  P1-T07 (legacy BIOS GRUB), P1-T06/T08 (numbers from CI logs).
- 2026-11-18: P1-T05/T07 In progress: post-image.sh builds the UKI
  (linuxx64.efi.stub + bzImage + embedded cmdline) and a genimage
  `aetheros-efi.img` (ESP + squashfs root); make-bios-image.sh builds a GRUB2
  i386-pc MBR `aetheros-bios.img`; CI runs OVMF and SeaBIOS boot tests for
  both chains plus the existing direct-kernel smoke test. Pushed; evidence
  pending owner confirmation of the CI run. Next after green: P1-T06/T08
  numbers → D6 results log; then P2 or remaining P0 graphics spikes (VM).
- 2026-11-18: P0-T12 (spike S5) **Done (headless scope)**: wine-spike run
  44232865848 green — Wine 10.0, Windows console exe ran, 20 ms startup.
  D6 log: docs/2026/11/18/results/p0-t12-spike-s5-wine.md. GUI-app residual
  noted. gh CLI access confirmed (owner logged in) — CI now self-verifiable.
- 2026-11-18: CI failure round 1 fixed: buildroot/init-compare pinned to
  ubuntu-22.04 (host-m4 gnulib vs new gcc C23 `_Generic`); graphics-spikes
  gained fontconfig. Round 2: slint `compat-1-18` enabled.
- 2026-11-18: P0-T07 **Blocked** — retry budget exhausted; six Wayland API
  compile errors recorded in the roadmap row (S1 binary kept non-blocking in
  CI so its finding still gets captured). P0-T08 unaffected: workflow split
  so S2+client build and run independently; first real S2 run triggered.
- 2026-11-18: buildroot-build (run 44234669522, ubuntu-22.04 pin) in progress
  (~1 h); boot-chain flips (P1-T02..T05, T07) and P1-T06/T08 D6 numbers wait
  on its verdict.
- 2026-11-18: Owner approved a fresh round on the P0-T07 blocker: developer
  agent (session ses_e13244ee0ffeeZ8CGFtGadg8B5) fixing the six recorded
  Wayland API errors (edits only; coordinating session commits after the
  in-flight S2 run 44235635068 completes). P0-T07 moves Blocked → In progress
  on the next push.
- 2026-11-18: Owner heading out ~5-6h; autonomous run continues. coverage-report.sh
  refined (Needs firmware classification note; clean local smoke). P1-T06/T08 D6
  skeleton written at docs/2026/11/18/results/p1-boot-speed.md awaiting the
  definitive buildroot-build verdict. Root cause of header failure traced and
  fixed; run 44240785348 is the one to watch.
- 2026-11-18: LOCAL LINUX TYPE-CHECK ESTABLISHED (unblocks all spike work):
  brew rustup + `rustup toolchain install stable -t aarch64-apple-darwin,
  x86_64-unknown-linux-gnu`; run checks as
  `RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin
  RUSTC=~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc
  $(brew --prefix rustup)/bin/cargo check --target x86_64-unknown-linux-gnu`
  from spikes/ (RUSTC override is required — cargo otherwise picks up brew's
  rustc without the linux std). s1's fontconfig build script cannot run
  cross, but s1 already compiles on CI; s2 + client fully check locally.
- 2026-11-18: S2 fixes pushed (delegate macro imports, SeatHandler impl with
  WlSurface focus types + SeatState field, accept() io::Result handling);
  local check clean; gate run 44236833280 watching. buildroot-build
  (44234669522) still building.
- 2026-11-18: graphics-spikes run 44236833280 **green**. P0-T07 **Done**
  (S1_LAYERSHELL_SURFACE_OK; blur finding recorded: needs
  org_kde_kwin_blur_manager — Smithay extension queued for P2; ADR-004
  confirmed) and P0-T08 **Done** (S2_CLIENT_MAPPED; ADR-009/016 validated).
  D6 report: docs/2026/11/18/results/p0-t07-t08-spikes-s1-s2.md. P0 now has
  only T10 (battery) open — blocked on real hardware. buildroot-build verdict
  pending for the P1 boot-chain flips and P1-T06/T08 numbers.
- 2026-11-18: coverage-report.sh verified on the runner — populated PCI rows
  (device + driver + Working) after two portability fixes (mawk interval
  regex; full PCI domain `0000:00:xx.x` slot format). P2-T08 tool ready for
  the first real-machine boot.
- 2026-11-18: buildroot-build failure 2 diagnosed — `expected 2.6.x headers`
  exposed that the defconfig never pinned headers AND used the default
  uclibc toolchain, so glibc-only packages (systemd, NetworkManager, Upower)
  were silently dropped from the P1-T01 green build (evidence caveat recorded
  in the tracker). Fix: BR2_TOOLCHAIN_BUILDROOT_GLIBC=y +
  BR2_KERNEL_HEADERS_AS_KERNEL=y. Next run re-verifies P1-T01 with the real
  userspace; watch the 150 MB gate (glibc adds bulk).
- 2026-11-20: **Billing block.** All GitHub Actions runs now fail to start
  with "recent account payments have failed or your spending limit needs to
  be increased". Owner must fix billing before P1 verification. S1 pivoted to
  local-only P2 prep: appended a P2 section (sound, cfg80211/mac80211,
  Bluetooth, input, sleep/resume) to `os-build/kernel/aether.fragment`, and
  created `os-build/buildroot/aetheros-p2-additions.defconfig` documenting the
  Mesa/Vulkan + PipeWire + firmware Buildroot symbols to merge when P2 starts.
  Tracker P1 row updated with the billing block. No active defconfig change
  (keeps the P1 150 MB gate meaningful).
