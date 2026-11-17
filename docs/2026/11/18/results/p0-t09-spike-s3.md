# Results Log — P0-T09 Spike S3 (cgroup.freeze, memory.reclaim, PSI)

Gate: P0-T09 Spike S3
Date: 2026-11-18
Machine: Apple M3 Pro host, 18 GB RAM, macOS 27.2.1; QEMU 11.1.3 TCG, 1 GB guest RAM
Guest: Alpine Linux 3.24.4 "virt" live ISO, linux 6.18.60-0-virt (x86_64)
Command: `os-build/test/spike-s3.sh os-build/test/images/alpine-virt-3.24.4-x86_64.iso`
Result: **Partial — 2 of 3 pass; PSI finding recorded (see Notes)**

## Findings

### 1. cgroup.freeze — Pass

- cgroup v2 mounted at `/sys/fs/cgroup` on the guest (live ISO).
- Test process moved into a subgroup; `echo 1 > cgroup.freeze` →
  `cgroup.events` reports `frozen 1`.
- Progress stopped while frozen: appended counter file size frozen at
  189 bytes across a 3-second window (189 == 189), then resumed after
  `echo 0 > cgroup.freeze`.
- Conclusion: the freezer mechanism AetherOS needs for app suspension
  (D3) works as expected.

### 2. memory.reclaim — Pass

- Fresh subgroup `s3r` with the memory controller enabled.
- Hog charged 67,538,944 bytes (64.4 MB, clean block-device page cache
  from `/dev/sr0`) into the subgroup: `memory.current` confirmed.
- `echo "40M" > memory.reclaim` returned 0; `memory.current` dropped to
  25,464,832 bytes (24.3 MB) — about 40 MB reclaimed on demand.
- Conclusion: proactive per-app reclaim (D3, P5-T04) is available on
  kernel 6.18 with cgroup v2.

### 3. PSI — Not available on the test kernel (spike finding)

- `/proc/pressure/{cpu,memory,io}` does not exist on the guest.
- Booting with `psi=1` does not change this, so the Alpine `linux-virt`
  kernel is built without `CONFIG_PSI` (PSI cannot be a module; the boot
  flag only helps when it is built but default-disabled).
- Action: our P1-T01 kernel fragment must set `CONFIG_PSI=y`; re-run the
  PSI part of this spike once the custom kernel builds (and confirm on
  the spare PC, where a distribution kernel with PSI may be used).
  `lifecycled` design (D3 pressure-driven killer) depends on PSI.

## Notes

- Runs 1 and 2 (2026-11-18, session implementation-2) failed for harness
  reasons, recorded here per the retry rule: run 1 reused a frozen cgroup
  for the reclaim test (kernel correctly returned EAGAIN on
  `memory.reclaim`); run 2's anonymous-memory hog (60 MB `dd | base64`)
  did not finish allocating under TCG emulation before measurement. Run 3
  used a cheap, clean page-cache hog and passed.
- Marker hygiene fix after run 3: markers are now emitted as
  `echo S3_*_"OK"` so the terminal echo of the command line can never be
  mistaken for a result.
- TCG emulation is slow; numbers are functional (pass/fail) evidence
  only, not performance evidence (D6 section 6).
