# F4 — Customization (Unix-Style)

Owner components: `nexa-confd` (configuration daemon), `nexactl` (command-line tool), Lua runtime for hooks, settings app (`nexa-settings`).

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
| CUS-04 | Live reload | v1 | `nexactl reload` or automatic reload on save. |
| CUS-05 | Keybindings file | MVP | Remap any shell shortcut. Conflicts are reported. |
| CUS-06 | Dock and menu bar layout file | MVP | Position, pinned items, and menu items are set by file. |
| CUS-07 | Shell and terminal choice | MVP | Default shell (bash, zsh, or fish) and default terminal app. |
| CUS-08 | `nexactl` command-line tool | MVP (basic), v1 (full) | Commands in section 6. |
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

## 6. `nexactl` Commands (MVP)

| Command | Purpose |
| --- | --- |
| `nexactl config get <key>` | Print a value |
| `nexactl config set <key> <value>` | Set a value and validate it |
| `nexactl config reset <key or file>` | Reset to default |
| `nexactl config check` | Validate all files |
| `nexactl reload` | Reload configuration |
| `nexactl apps list` | List running apps and their states (F2) |
| `nexactl apps suspend <app-id>` | Suspend an app |
| `nexactl apps quit <app-id>` | Quit an app |
| `nexactl version` | Show OS and component versions |

## 7. Dependencies

- P7 phase.
- Every feature document refers to its keys here.

## 8. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| CUS-AC-01 | Changing `dock.toml` position moves the dock without restart | Not tested |
| CUS-AC-02 | Invalid TOML shows file, line, and key; previous settings remain active | Not tested |
| CUS-AC-03 | `nexactl config set` and `get` round-trip a value | Not tested |
| CUS-AC-04 | Reset of one file restores the default values | Not tested |
| CUS-AC-05 | Every settings screen maps to a file key (automated check) | Not tested |
| CUS-AC-06 | A Lua hook that runs past its time limit is stopped and reported | Not tested |
| CUS-AC-07 | Theme token change updates colors across shell components | Not tested |

## 9. Open Questions

- Whether Lua hooks may run at login without a confirmation prompt. Proposed: yes for local user scripts; extensions need an explicit approval.
