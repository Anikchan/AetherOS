# Results — Spike S5 GUI: Wine engine GUI path (P0-T12 residual)

- Date: 2026-11-20 (S2 autonomous prep; run pending graphical VM/CI)
- D6 metric format: `os-build/metrics/FORMAT.md`
- Tool: `os-build/test/spike-s5-gui.c`, `os-build/test/spike-s5-gui.sh`
- Status: **Prep complete; run pending** (needs Wine + Xvfb on a Linux host;
  CI workflow to be added by S4 or run via `workflow_dispatch`)

## What is proven so far

- Headless console path proven (P0-T12 Done, 2026-11-18, run 44232865848):
  Wine 10.0, mingw-w64 console exe ran, 20 ms startup. See
  `docs/2026/11/18/results/p0-t12-spike-s5-wine.md`.
- GUI test program `spike-s5-gui.c` added: registers a Win32 window class,
  creates a 480x320 top-level window, paints a solid background + text,
  keeps the window alive for ~3 s, prints `S5_GUI_WINDOW_CREATED_MS` and
  `S5_GUI_DONE`.
- GUI spike script `spike-s5-gui.sh` added: builds with
  `x86_64-w64-mingw32-gcc -mwindows`, initializes a Wine prefix, runs under
  a virtual display (`Xvfb :99`) when available, captures window-creation
  latency and markers.

## Pending run

- Execute `os-build/test/spike-s5-gui.sh` on an ubuntu-latest runner (or a
  local Linux host with Wine + mingw-w64 + Xvfb).
- Record Wine version, window-creation latency, and whether the window was
  created (`S5_GUI_RUN_OK`).

## Run table (fill after run)

| Metric | Value |
| --- | --- |
| Wine version | _pending_ |
| Build | _pending_ (S5_GUI_BUILD_OK) |
| Window created | _pending_ (S5_GUI_RUN_OK) |
| Window-creation latency (ms) | _pending_ (S5_GUI_WINDOW_CREATED_MS) |
| Xvfb used | _pending_ |
| Runner | _pending_ |

## Acceptance (P0-T12 residual)

- A Windows GUI program creates and shows a window under Wine on Linux.
- Measured window-creation latency recorded.
- If Wine's Wayland driver (or XWayland) path is needed, note which and
  whether it must ship in the AetherOS image.

## Notes

- GUI path is the remaining part of spike S5; the console baseline (20 ms)
  already supports ADR-013. This residual does not block P0-T12 (already
  Done, headless scope); it informs the P2/P3 graphics + P6 Windows-engine
  packaging decisions.
- Tracker updates are S1's; this doc is S2-owned territory.
