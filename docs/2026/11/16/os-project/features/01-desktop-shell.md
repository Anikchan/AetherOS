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
| SHL-AC-01 | Menu bar shows the active app menu and updates within 100 ms of focus change | Not tested |
| SHL-AC-02 | Dock shows running indicators; a closed app's indicator disappears within 1 s | Not tested |
| SHL-AC-03 | Launcher opens by keyboard shortcut within 200 ms and finds apps by partial name | Not tested |
| SHL-AC-04 | Control center changes Wi-Fi, Bluetooth, volume, and brightness | Not tested |
| SHL-AC-05 | Focus mode hides notifications during the scheduled time and shows them after | Not tested |
| SHL-AC-06 | Snap to halves and quarters works with a mouse drag and a keyboard shortcut | Not tested |
| SHL-AC-07 | Glass tiers switch without restart; Minimal tier used when reduce transparency is on | Not tested |
| SHL-AC-08 | Lock screen rejects a wrong password and accepts the right one | Not tested |
| SHL-AC-09 | Every settings category opens and every change is written to the config file | Not tested |
| SHL-AC-10 | Two monitors with different scales work without restart | Not tested |
| SHL-AC-11 | Screen reader announces menu bar items and launcher results | Not tested |

Status values: Not tested, Pass (date and evidence), Fail (ticket ID).

## 6. Open Questions

- Dock position default: bottom (macOS-like) or side. Decide with the owner (Q1 relates to the visual reference).
- Whether to include a start-menu-style layout as an option (Windows-friendly users). Proposed: yes, as a launcher layout option in v1.
