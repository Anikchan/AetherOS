# aether-config — configuration, theme, keybindings, hooks, feature packs (P7)

Files first (F4): every setting is a TOML key registered in `src/schema.rs`.
Layers: `/usr/share/aether/defaults` → `/etc/aether` → `~/.config/aether`
(later wins). Invalid files are reported and skipped, so the last valid
configuration stays active.

## Commands

```bash
aetherctl config get dock.position
aetherctl config set dock.position left
aetherctl config reset dock.position
aetherctl config reset --file shell/dock.toml
aetherctl config reset --all
aetherctl config check
aetherctl reload
aetherctl theme show
aetherctl keybindings list|check
aetherctl hooks list|run on_login
aetherctl features list|preview D|install D|remove D|disable D
aetherctl apps list|suspend APP-ID|quit APP-ID
aetherctl version
```

`--root DIR` points at a test configuration root (`DIR/defaults`, `DIR/system`,
`DIR/user`).

## Files

| File | Keys |
|---|---|
| `aether.toml` | locale, timezone, effects tier |
| `shell/dock.toml` | position, icon size, magnification, auto-hide, pinned apps |
| `shell/menubar.toml` | position, clock, tray |
| `shell/launcher.toml` | search sources |
| `shell/notifications.toml` | Focus mode, enabled |
| `theme/tokens.toml` | accent/background/foreground colors, radius, blur, tint, font, spacing |
| `input/keybindings.toml` | `[bindings]` shortcut = action |
| `process/lifecycle.toml` | grace periods, hibernation delay |
| `scripts/*.lua` | hooks (`on_login`, `on_focus_change`, `on_window_open`, `on_window_close`, `on_network_change`, `on_power_change`) |
| `features/features.toml` | installed/enabled optional packs |

## Theme reload

`ThemeEngine` compares the tokens file timestamp and returns a `ThemeReload`
with `changed`, the new tokens, and an error. A valid change can be applied by
the shell without restart; an invalid change never replaces the active tokens.

## Hooks

Hooks run with the system `lua` interpreter and a 200 ms limit. A timeout or
non-zero exit disables the hook and records it in `hooks-disabled.json`; the
shell keeps running. `aetherctl hooks enable NAME` re-enables it.

## Feature packs (F15)

`A` Android, `B` Windows apps, `C` AI assistant, `D` advanced monitoring,
`E` Screen Time limits, `F` battery reports, `G` storage analyzer. Installing
shows disk and idle cost first and is refused when the idle budget would be
exceeded. Removal clears the pack's state; payload removal belongs to the
package manager.

## Honest limits

- The settings UI (S3) consumes these keys; the daemon/binding is not in this
  crate.
- Hooks use an external Lua interpreter, not an embedded VM.
- Pack payloads (Android container, Wine engine, AI model) are not installed by
  this crate; it manages state and budgets only.
