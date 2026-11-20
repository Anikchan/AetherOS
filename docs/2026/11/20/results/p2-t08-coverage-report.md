# Results — P2-T08: Hardware coverage report (tool)

- Date: 2026-11-20 (S2 autonomous; tool verified on runner 2026-11-18)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Tool: `os-build/test/coverage-report.sh`
- Status: **Tool ready and verified; real-machine run pending** (spare Dell)

## Tool verification (done)

- Script runs on the GitHub ubuntu-latest runner (`coverage-sample.yml`,
  2026-11-18): produced populated PCI rows (device + driver + Working) after
  two portability fixes (mawk interval regex; full PCI domain
  `0000:00:xx.x` slot format).
- 2026-11-20 S2 refinement: auto-classify "Needs firmware" by checking
  whether the bound driver module appears in the missing-firmware dmesg
  scan (`MISSING_DRV`); classification note updated. `bash -n` clean;
  graceful on hosts without lspci/lsusb (macOS smoke run produced empty
  sections, no crash).

## What the tool reports

- CPU / memory summary.
- PCI devices: device, bound driver, coverage (Working / Needs firmware /
  No driver).
- USB devices (`lsusb`).
- Network interfaces, ALSA audio cards.
- Battery (sysfs `BAT*`).
- Missing-firmware lines from dmesg (last 10).

## Pending (needs spare Dell)

- Run on the spare Dell at first AetherOS boot:
  `os-build/test/coverage-report.sh > coverage-$(hostname).md`
- Populate `docs/.../04` section 6.2 hardware matrix (P2-T07).
- Feed firmware list template (`docs/2026/11/18/results/p2-t01-firmware-list-template.md`).

## Acceptance (P2-T08)

- Tool runs on real hardware and produces a populated device table.
- Classification matches the 04 section 6 checklists (final verdicts
  confirmed manually).

## Notes

- Blocked on spare Dell availability (owner action). Tooling itself is
  complete; no further code changes required unless the first real run
  exposes a portability issue.
