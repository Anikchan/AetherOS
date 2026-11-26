# F4 — Customization (Unix-Style)

Owner components: `aether-confd` (configuration daemon), `aetherctl` (command-line tool), Lua runtime for hooks, settings app (`aether-settings`).

## 1. Principles

1. **Files first.** Every setting is a key in a text file. The settings app is a visual editor for those files.
2. **One format.** All configuration uses TOML. Scripts use Lua.
3. **Layered.** System defaults in `/usr/share/nexa/defaults/`, system overrides in `/etc/nexa/`, user overrides in `~/.config/nexa/`. Later layers win.
4. **Reversible.** Any setting can be reset to the system default.
5. **Validated.** Invalid files are rejected with a clear error. The last valid config stays active.
6. **Live when possible.** Most changes apply without restart.

## 2. Configuration Layout

```
/usr/share/nexa/defaults/      system defaults (read-only)
/etc/nexa/                     system overrides (root)
~/.config/nexa/
  nexa.toml                    global settings (locale, time zone, effects tier)
  shell/
    menubar.toml               menu bar items and layout
    dock.toml                  pinned apps, position, size, auto-hide
    launcher.toml              search sources and layout
    notifications.toml         Focus schedules
  theme/
    tokens.toml                colors, radius, blur, fonts (D1 section 5)
  input/
    keybindings.toml           shortcuts
  process/
    lifecycle.toml             grace periods, hibernation timing (F2)
  apps/
    <app-id>.toml              per-app overrides (CPU weight, memory limit, exceptions)
  scripts/
    *.lua                      hooks (section 5)
```

## 3. Example

```toml
# ~/.config/nexa/shell/dock.toml
position = "bottom"        # "bottom" | "left" | "right"
icon_size = 56             # pixels, base size before magnification
magnification = 1.4        # 1.0 disables it
auto_hide = true
pinned = ["org.mozilla.firefox", "com.nexa.terminal", "org.gnome.TextEditor"]
```

## 4. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| CUS-01 | Layered configuration | MVP | Defaults, system overrides, and user overrides (section 1). |
| CUS-02 | Config validation | MVP | Schema check on save. Errors show file, line, and key. Last valid config stays active. |
| CUS-03 | Theme tokens | MVP | Colors, radii, blur strength, tint, font, and spacing in one file (D1 section 5). |
| CUS-04 | Live reload | v1 | `aetherctl reload` or automatic reload on save. |
| CUS-05 | Keybindings file | MVP | Remap any shell shortcut. Conflicts are reported. |
| CUS-06 | Dock and menu bar layout file | MVP | Position, pinned items, and menu items are set by file. |
| CUS-07 | Shell and terminal choice | MVP | Default shell (bash, zsh, or fish) and default terminal app. |
| CUS-08 | `aetherctl` command-line tool | MVP (basic), v1 (full) | Commands in section 6. |
| CUS-09 | Lua hooks | v1 | Scripts run on events (for example, on login, on focus change, on Wi-Fi connect). Sandboxed with a time limit. |
| CUS-10 | Extension API | Later | Shell extensions can add menu bar items and launcher actions. Signed or local only. |
| CUS-11 | Reset to defaults | MVP | Reset one file, one section, or everything. |
| CUS-12 | Settings-to-file rule | MVP | Every setting in the settings app is a file key. An automated test checks coverage (04). |
| CUS-13 | Dotfile backup and sync | Later | Export and import of `~/.config/nexa/` as a single archive. |

## 5. Lua Hooks (v1)

- Hooks are plain `.lua` files in `~/.config/nexa/scripts/`.
- Available events: `on_login`, `on_focus_change`, `on_window_open`, `on_window_close`, `on_network_change`, `on_power_change`.
- Each hook has a time limit (default 200 ms) and no direct filesystem write access outside `~/.config/nexa/`.
- A failing hook is disabled and reported in the activity log. It does not stop the shell.

## 6. `aetherctl` Commands (MVP)

| Command | Purpose |
| --- | --- |
| `aetherctl config get <key>` | Print a value |
| `aetherctl config set <key> <value>` | Set a value and validate it |
| `aetherctl config reset <key or file>` | Reset to default |
| `aetherctl config check` | Validate all files |
| `aetherctl reload` | Reload configuration |
| `aetherctl apps list` | List running apps and their states (F2) |
| `aetherctl apps suspend <app-id>` | Suspend an app |
| `aetherctl apps quit <app-id>` | Quit an app |
| `aetherctl version` | Show OS and component versions |

## 7. Dependencies

- P7 phase.
- Every feature document refers to its keys here.

## 8. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| CUS-AC-01 | Changing `dock.toml` position moves the dock without restart | Not tested |
| CUS-AC-02 | Invalid TOML shows file, line, and key; previous settings remain active | Not tested |
| CUS-AC-03 | `aetherctl config set` and `get` round-trip a value | Not tested |
| CUS-AC-04 | Reset of one file restores the default values | Not tested |
| CUS-AC-05 | Every settings screen maps to a file key (automated check) | Not tested |
| CUS-AC-06 | A Lua hook that runs past its time limit is stopped and reported | Not tested |
| CUS-AC-07 | Theme token change updates colors across shell components | Not tested |

## 9. Open Questions

- Whether Lua hooks may run at login without a confirmation prompt. Proposed: yes for local user scripts; extensions need an explicit approval.

## 10. Optional Feature Packs — Settings → Features (P7-T07, 2026-11-26)

F15's optional packs are installed and removed from **Settings → Features**
(sidebar index `Category::FEATURES_INDEX` in `shell/src/config.rs`). The page is
presentation only: the rules live in `aether-config`'s `FeatureManager`
(S4-owned, used read-only here — `shell/src/features.rs` wraps it).

- **One row per pack A–G** with name, description, status pill
  (Not installed / Installed / Disabled) and the full cost **before** install:
  disk, idle CPU and memory. The manager reports 0 MB for packs that are not
  installed, so the row reads the catalog values instead (`OPT-AC-02`).
- **Budget bar** shows the limit (`Idle budget 1.0% CPU · 256 MB memory`, F6
  RES-03/04) and current use (packs installed, CPU %, memory and disk).
- **Install** calls `FeatureManager::install`; if the idle budget would be
  exceeded the install is refused, nothing is written, and the reason
  ("idle budget would be exceeded: …") is shown in the danger colour
  (G-OPT1/`OPT-AC-04`).
- **Enabled** packs can be disabled with the row switch (state stays
  installed) and removed with **Remove**, which clears the pack's state so
  nothing is left behind (G-OPT3).
- Every change is written to `~/.config/aether/features/features.toml` — the
  same file `aetherctl features list|install|remove|disable|enable` reads and
  writes, so the CLI and the GUI never diverge.
- Unknown packs and remove-before-install errors are reported in the page, not
  swallowed.

Acceptance:

| ID | Criterion | Result |
| --- | --- | --- |
| OPT-AC-01 | Pack catalog lists every pack with status | Pass (headless, 2026-11-26) — 7 rows, status and pill asserted |
| OPT-AC-02 | Cost (disk, idle CPU, memory) is visible before install | Pass (headless) — unit and surface tests assert the labels |
| OPT-AC-03 | Install and remove work and persist | Pass (headless) — file written and reloaded; `aetherctl` reads the same path |
| OPT-AC-04 | An install that breaks the idle budget is refused with the reason | Pass (headless) — `OverBudget` message shown, pack stays uninstalled |
| OPT-AC-05 | Disable keeps a pack installed; removal clears its state | Pass (headless) |

Evidence: `shell/tests/features.rs`, `shell/src/features.rs` unit tests and the
`shell-features` screenshot from `aether-shell --render`.
