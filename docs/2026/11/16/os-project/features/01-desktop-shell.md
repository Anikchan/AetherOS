# F1 — Desktop Shell

Owner component: `aether-shell` (menu bar, dock, launcher, control center, notifications, lock screen, file manager, overview) on top of `aether-compositor`.

Priority key: MVP (needed for the first daily-driver build), v1 (needed for release), Later (after release).

## 1. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| SHL-01 | Menu bar | MVP | Top bar with the active app name, its menus, system tray, clock, and a status menu. Height 28 px (see D1). |
| SHL-02 | Dock | MVP | Bottom or side dock with pinned and running apps, running indicators, drag to reorder, badge counts, auto-hide. |
| SHL-03 | Launcher and search | MVP | Full-screen or panel launcher. Search apps, files, settings, and simple calculations. Keyboard-first. |
| SHL-04 | Control center | MVP | Quick panel for Wi-Fi, Bluetooth, sound, brightness, Focus, and display settings. |
| SHL-05 | Notification center | MVP | Grouped notifications by app. Focus modes that mute notifications by schedule or by app. |
| SHL-06 | Window management | MVP | Window controls (close, minimize, maximize), drag to snap to halves and quarters, double-click title bar to maximize. |
| SHL-07 | Tiling mode | v1 | Per-workspace tiling layout with keyboard shortcuts. Windows can float or tile per app. |
| SHL-08 | Spaces and overview | v1 | Multiple workspaces. Overview shows all windows and workspaces. Three-finger swipe to switch. |
| SHL-09 | Glass material system | MVP (prototype), v1 (full) | Translucent panels with blur, tint, and edge highlight. Effect tiers in F6 and D1. |
| SHL-10 | Lock screen and login | MVP | Password or PIN unlock. Fast user switching later (v1). |
| SHL-11 | Settings app | MVP | Categories: display, sound, network, power, keyboard, mouse and touchpad, apps, privacy, appearance, about. Each change writes to config (F4). |
| SHL-12 | File manager | MVP | List, column, and icon views. Basic copy, move, rename, trash. Tags and quick preview later. |
| SHL-13 | Quick preview | Later | Press Space on a file to preview it. |
| SHL-14 | Gestures | v1 | Touchpad: three-finger swipe (spaces), four-finger pinch (overview), two-finger scroll. |
| SHL-15 | Multi-monitor and HiDPI | MVP | Different scales per monitor. Arrangement and mirroring. |
| SHL-16 | Appearance | v1 | Light, dark, and automatic (time-based) themes. Accent color. Wallpaper. |
| SHL-17 | Screenshot and recording | MVP | Area, window, and full-screen capture. Screen recording to a file. |
| SHL-18 | Accessibility | MVP (base), v1 (full) | Screen reader via AT-SPI, keyboard navigation for all shell features, high contrast, reduce transparency, reduce motion, text scaling. |

## 2. Window Controls and Behavior

- Window buttons are an original design (see D1). They are placed at the left of the title bar by default. Position is configurable (F4).
- Double-click on a title bar toggles maximize.
- Dragging a window to a screen edge shows a snap preview. Release snaps to the half or quarter.
- Windows of the same app group in the dock and in the overview.

## 3. Glass Material Behavior

- Glass applies to panels, menus, the dock, the control center, and notifications. It does not apply to document content.
- The compositor provides the blurred background. The shell provides the tint and highlight.
- Effect tiers:
  - Full: blur, saturation, tint, edge highlight, subtle edge refraction.
  - Reduced: blur at half resolution, no refraction.
  - Minimal: solid tinted panels with a border (used on low-end GPUs and when "reduce transparency" is on).
- Tier is chosen at startup from a GPU capability check and can be changed in settings (F4).

## 4. Dependencies

- P3 compositor (windows, layer shell, effects pipeline).
- F2 process manager (app state for the dock and overview).
- F4 configuration (all settings).

## 5. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| SHL-AC-01 | Menu bar shows the active app menu and updates within 100 ms of focus change | Implemented, headless-verified (2026-11-23) — menus are property-driven; the 100 ms figure needs compositor focus events (P3-T02) |
| SHL-AC-02 | Dock shows running indicators; a closed app's indicator disappears within 1 s | Implemented, headless-verified — indicator is bound to `App.running`; live timing needs the session |
| SHL-AC-03 | Launcher opens by keyboard shortcut within 200 ms and finds apps by partial name | Implemented, headless-verified — Super+Space opens it, ranking is a pure function (no I/O) and partial names rank above substrings |
| SHL-AC-04 | Control center changes Wi-Fi, Bluetooth, volume, and brightness | Implemented, headless-verified with mock services — nmcli/bluetoothctl/wpctl/brightnessctl adapters exist; on-device confirmation pending |
| SHL-AC-05 | Focus mode hides notifications during the scheduled time and shows them after | Implemented, headless-verified — the daily window comes from `notifications.toml` (`focus-start`/`focus-end`, overnight supported), manual Focus runs two hours, the minute tick expires it and refreshes cards and badge; the settings pane edits the keys |
| SHL-AC-06 | Snap to halves and quarters works with a mouse drag and a keyboard shortcut | Not tested — compositor work (P3-T04) |
| SHL-AC-07 | Glass tiers switch without restart; Minimal tier used when reduce transparency is on | Partially verified — tier selection and the reduce-transparency override are unit-tested; live switching needs the effect pipeline (P3-T05) |
| SHL-AC-08 | Lock screen rejects a wrong password and accepts the right one | Implemented, headless-verified — salted digest, constant-time compare, and the screen is modal: scrim, Escape and shortcuts cannot dismiss it (asserted); PAM/greetd integration is the remaining step |
| SHL-AC-09 | Every settings category opens and every change is written to the config file | Implemented, headless-verified — categories switch the page (clicked in the test), rows are generated from the F4 schema, each shows file+key, and writes are atomic and read back from disk |
| SHL-AC-10 | Two monitors with different scales work without restart | Not tested — compositor work (P3-T02) |
| SHL-AC-11 | Screen reader announces menu bar items and launcher results | Partially verified — every interactive element sets an accessible role/label (asserted by the headless tests); AT-SPI session pending |

Evidence: `docs/2026/11/23/results/p4-shell-and-apps.md` and
`docs/2026/11/23/results/screenshots/`.

Status values: Not tested, Pass (date and evidence), Fail (ticket ID).

## 6. Open Questions

- Dock position default: bottom (macOS-like) or side. Decide with the owner (Q1 relates to the visual reference). **Status: the implementation ships a bottom dock; the config key `shell/dock.toml: position` is accepted by the schema but only `bottom` is rendered so far.**
- Whether to include a start-menu-style layout as an option (Windows-friendly users). Proposed: yes, as a launcher layout option in v1. **Status: the launcher is the macOS-like centred panel; the start-menu layout remains a v1 option.**

## 7. Implementation Notes (2026-11-23)

- The shell is `shell/` (Rust + Slint); the visual language, widgets and icons
  live in the shared `ui/` crate so the default apps match.
- Every surface is rendered headlessly in CI-able form; screenshots are the P4
  evidence.
- Keyboard shortcuts: Super+Space (launcher), Super+A (overview), Super+N
  (notifications), Super+C (control centre), Super+L (lock), Super+F (Focus),
  arrows + Enter in the launcher, Esc closes any overlay. Names match the
  `input/keybindings.toml` keys.
- The on-device gate for this document's acceptance table is the layer-shell
  host from P3-T02 plus greetd/PAM for the lock screen.
