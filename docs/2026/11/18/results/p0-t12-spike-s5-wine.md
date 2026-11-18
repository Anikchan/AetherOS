# Results — Spike S5: Wine engine (P0-T12)

- Date: 2026-11-18 (implementation-3, CI run 44232865848, `wine-spike`
  workflow, ubuntu-latest Linux x86-64 VM)
- D6 metric format: `os-build/metrics/FORMAT.md`

## Result: PASS (headless scope)

| Metric | Value |
| --- | --- |
| Wine version | wine-10.0 (Ubuntu 10.0~repack-12ubuntu1) |
| Test program | `os-build/test/spike-s5-hello.c` (Win32 console, built with x86_64-w64-mingw32-gcc, -O2) |
| Program ran | Yes — `S5_WINE_RUN_OK` |
| Startup (process start to exit) | **20 ms** (`S5_STARTUP_MS: 20`) |
| Runner | GitHub ubuntu-latest (Linux x86-64 VM, no display) |

## Scope and findings

- Proven: a mingw-w64-built Windows console program runs under Wine on a
  plain Linux VM with no configuration beyond the distro package; startup
  measured at 20 ms — far inside any interactive-launch budget.
- Headless scope: console app only. The GUI-app path (a Windows program
  rendering through our Wayland compositor) is the remaining part of this
  spike and runs in the graphical VM with the P2 graphics stack — tracked as
  a P0-T12 residual, not a failure.
- ADR implications: supports ADR-013 (Wine engine for Windows compatibility);
  no fallback trigger.

## Commands

```
x86_64-w64-mingw32-gcc -O2 -o spike-s5-hello.exe spike-s5-hello.c
wine64 wineboot -i   # prefix init, not timed
time wine64 spike-s5-hello.exe
```
