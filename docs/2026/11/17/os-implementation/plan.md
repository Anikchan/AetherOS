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
| P0-T02 ADR review | **Done (2026-11-17)** | Owner accepted ADR-001 to ADR-023: Groups 1 and 2 as proposed; ADR-004/009/021 accepted with spike safeguards (S1/S5/S6 still validate; fallbacks unchanged). Statuses updated in D2 section 9; D7 kernel note updated. |
| P0-T04 Hardware list | Pending | Owner must supply their own PC list and spare machine details (04 section 6). |
| P0-T07 Spike S1 (Slint layer-shell + blur) | Pending | Needs a Linux/Wayland session — run inside the QEMU VM or a Linux environment; VM has no Wayland compositor session yet. Slint evaluation can start on macOS headless partially, but layer-shell requires Linux. |
| P0-T08 Spike S2 (Smithay compositor) | Pending | Needs Linux (DRM/KMS); run in VM or on the spare PC. |
| P0-T09 Spike S3 (cgroup.freeze, memory.reclaim, PSI) | **Done, partial (2026-11-18)** | Freeze Pass, reclaim Pass (~40 MB reclaimed). PSI absent from the Alpine virt kernel (CONFIG_PSI not built; `psi=1` has no effect) — re-test folded into P1-T01. Report: `docs/2026/11/18/results/p0-t09-spike-s3.md`. 2 harness retries used (frozen-cgroup reuse; TCG-too-slow hog); marker hygiene fixed. |
| P0-T10 Spike S4 (UPower/sysfs battery) | Pending | Needs a machine with a battery (spare PC or a laptop VM passthrough). |
| P0-T11 Verification setup (budgets.toml) | **Done (2026-11-17)** | `os-build/budgets.toml`, `os-build/metrics/FORMAT.md`, `os-build/check-budgets.py`, CI step added. Verified: pass case exit 0, over-limit case exit 1, full build+test+budget green. |
| P0-T12 Spike S5 (Wine in VM) | Pending | Needs the Linux VM with X/Wayland; heavy. |

## Next steps for this session

1. P0-T09 spike S3 inside the QEMU VM (needs a writable Alpine rootfs —
   install Alpine to a disk image, then test cgroup.freeze, memory.reclaim,
   PSI).
2. P0-T07/T08 spikes need a graphical Linux environment — set up in the VM
   (software rendering + Wayland) or defer to the spare PC; propose to the
   owner.
3. Owner items waiting: P0-T02 ADR acceptance, P0-T04 hardware list, git
   remote for CI.

## Working rules applied

- Roadmap section 6: task marked In progress in the tracker before work; will be
  marked Done with a progress-log line only after verification.
- No task owned by another session will be touched.

## Progress log (this session)

- 2026-11-17: P0-T05 marked In progress; repository skeleton created; Rust
  toolchain installation started.
- 2026-11-17: Rust 1.100.0 installed (Homebrew). `cargo build --workspace` and
  `cargo test --workspace` pass (4/4 tests). P0-T05 marked Done in the roadmap
  tracker with a progress-log line. README status updated. P0-T02 prep doc
  written. Commit created on `main`.
- 2026-11-17: P0-T03 and P0-T06 marked In progress in the tracker; license
  research launched; QEMU install started.
- 2026-11-17: P0-T03 Done (02 section 3.1 updated with verified SPDX licenses
  and rules). P0-T06 Done: BIOS and UEFI boot tests pass (two retries on the
  UEFI path: hdiutil mount failed → switched to sevenzip extraction; QEMU 11
  rejects edk2 via `-bios` → switched to pflash). Results recorded in D6
  format. Tracker and progress log updated.
- 2026-11-17: P0-T11 Done: budgets.toml + check-budgets.py + metrics format +
  CI step; pass and fail cases verified locally. Commit created on `main`.
- 2026-11-17: P0-T02 Done: owner accepted all ADRs (ADR-001 to ADR-023) in
  D2 section 9; spike safeguards annotated on ADR-003/004/009/021; D7 note
  updated. Commit created on `main`.
- 2026-11-18: P0-T09 Done (partial): spike S3 harness (`spike-s3.sh` +
  `spike-s3.exp`) run three times; freeze and memory.reclaim pass with real
  numbers; PSI absent from the Alpine virt kernel (finding recorded,
  re-test in P1-T01). Report at `docs/2026/11/18/results/p0-t09-spike-s3.md`.
  Session name for the tracker is "implementation-2" (new day).
