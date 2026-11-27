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

1. **P1-T05 UEFI boot chain** — **Done (2026-11-23).** Root cause: GRUB2 EFI autoloaded `linuxefi.mod` at runtime but it wasn't on the ESP. Fix: prefer `linuxefi` over `linux` in `post-image.sh` grub-mkimage module list. Verified by CI run 45141636923: S_UEFI_BOOT_OK, 33 s TCG. M1 achieved.
2. **P1-T08 D6 boot-speed log** — **Done 2026-11-23**: image 15 MB (pass), direct boot 23–24 s (TCG), BIOS 27 s (TCG), UEFI n/a. Written to `docs/2026/11/18/results/p1-boot-speed.md`.
3. **P2 prep (local, no CI):** kernel fragment P2 additions + defconfig additions doc. Done 2026-11-20.
4. **P2-T01 Firmware list** — **Done 2026-11-23.** Populated from Dell coverage report (`docs/2026/11/18/results/p2-t01-firmware-list-template.md`). Base image ships i915 DMC/GuC/HuC + SOF audio; Wi-Fi/BT/NVIDIA optional. D6 log: `docs/2026/11/23/results/p2-t08-coverage-report.md`.
5. **P0-T10 Battery spike** — **Done 2026-11-23.** Ran `spike-s4.sh` on the Dell (battery present). All markers pass (S4_BATTERY_PRESENT, S4_SYSFS_OK, S4_UPOWER_OK, S4_CHARGE_LIMIT_SUPPORTED, S4_TIME_REMAINING_OK, S4_BATTERY_SPIKE_DONE). D6 log: `docs/2026/11/23/results/p0-t10-spike-s4-battery.md`.
6. **P2-T08 Coverage report** — **Done 2026-11-23.** Run on Dell; all PCI/USB/NVMe/Wi-Fi/audio Working. D6 log: `docs/2026/11/23/results/p2-t08-coverage-report.md`.
7. **P2-T07 Hardware matrix** — **Done 2026-11-23.** `04-test-and-acceptance.md` §6.2 updated with Dell Inspiron 5402 data.
8. **P2-T02 Graphics (Mesa+Vulkan)** — **Done 2026-11-23.** Merged into defconfig; CI build passes (image 27 MB, all boot chains green, run 45155405768). Boot times: direct 16 s, BIOS 17 s, UEFI 24 s (TCG).
9. **P2-T03 Wi-Fi/BT** — pending: needs AetherOS on the Dell (bare-metal).
10. **P2-T04 Audio (PipeWire)** — pending: needs AetherOS on the Dell.
11. **P2-T05 Input** — pending: needs AetherOS on the Dell.
12. **P2-T06 Sleep/resume** — pending: needs AetherOS on the Dell (10 cycles).
13. **M2 closure** — pending: owner's real machine passes the checklist (04 §6).
9. **P1-T05 UEFI debug** — **Done.** Fix applied and verified (run 45141636923: S_UEFI_BOOT_OK). Root cause: missing `linuxefi` module in grub-mkimage standalone image on GRUB 2.06 (CI runner); the `linux` command triggered a runtime autoload of `linuxefi.mod` which wasn't on the ESP. Fixed by preferring `linuxefi` over `linux` in `post-image.sh`.

## Files this session touches

- `os-build/buildroot/aetheros_defconfig`
- `os-build/kernel/aether.fragment`
- `os-build/test/ci-boot*.exp`
- `.github/workflows/buildroot-build.yml`
- `.github/workflows/kernel-config.yml`
- `docs/2026/11/16/os-project/03-roadmap-plan.md` (tracker only)
- `docs/2026/11/17/os-implementation/plan.md`

## Status (2026-11-26, M1 achieved, P2 hardware validation run 2)

**P1 DONE — M1 achieved.** All tasks T01–T08 complete (UEFI linuxefi + PARTUUID +
rootwait + VT/fbcon + getty@tty1 verified in QEMU and on the Dell display).
**P0** essentially complete. **P2** in progress:
- T01 firmware list ✅, T02 Mesa+Vulkan ✅ (i915 bound, 1920×1080, DMC v2.12),
  T04 audio ✅ (PipeWire + ALSA HDA, speaker-test + mic round-trip),
  T06 sleep/resume ✅ (10/10 cycles via RTC wakealarm),
  T07 matrix ✅, T08 coverage report ✅.
- T03 partial: ath10k firmware loaded, `iw dev wlan0 scan` finds 6 SSIDs;
  connect pending credentials; BT adapter up + bluetoothctl present.
- T05 partial: keyboard + PS/2 mouse + backlight 96000; touchpad is PS/2-only
  (Ubuntu sees I2C-HID "DLL09D9:00 04F3:3147 Touchpad"; AetherOS only "PS/2
  Generic Mouse" — I2C-HID binding needs investigation); webcam /dev/video0
  present; brightness/volume keys manual.
- Battery ✅ (100%, 24.795 Wh, 46.77%), temp ✅ (55 °C).
- Pending: Wi-Fi connect (SSID/password), touchpad gestures + key verification,
  external display hot-plug, reboot-to-Windows, M2 closure.
**Build** 45788955461 ✅ (38 MB, all boot chains). **CI** green (45869439628).
**Dell** on Ubuntu (192.168.68.112); USB has the latest AetherOS image.

### Remaining S1 work
1. Re-boot AetherOS on the Dell; run Wi-Fi connect (needs credentials), touchpad,
   brightness/volume keys, external display, reboot-to-Windows.
2. Close M2 (04 §6.1 checklist).
3. Keep the image build green (kernel-config now accepts DRM_I915=m).

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

## Progress log

- **2026-11-27 (S1, Nov27 round)** — image `45876776939` green (boot tests BIOS + UEFI), artifact `15150017702` flashed to the Dell USB and verified byte-identical (`sha256 60a65adc…`), `/root/test.sh` present in the squashfs. UI status recorded in `docs/2026/11/27/results/ui-readiness-status.md`: the shell and apps render headless (10 surfaces + 9 tabs) but no Rust binary is packaged into the image yet — the Buildroot UI package is the next task. CI fixes: `3e43953` (sandbox Landlock test asserted rule order; now asserts rule shape) and `00eb1f5` (compositor `XDG_TOPLEVEL` names its client; the no-hang test follows its own client instead of assuming client 0, which broke on the Linux runner where XWayland shares the client numbering).
- **Hardware/dev loop**: the Dell is reachable again (`ubuntu@192.168.68.112`, passwordless sudo); Rust 1.100 installed there so Linux-only test failures reproduce in minutes instead of a two-hour CI cycle. USB flashed with the image above; awaiting a boot pass for `sh /root/test.sh`.