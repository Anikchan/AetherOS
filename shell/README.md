# aether-shell — desktop shell (P4)

The AetherOS desktop shell: menu bar, dock, launcher, control centre,
notification centre, lock screen, overview and the settings app. Rust owns the
behaviour; Slint owns the presentation.

## Layout

```
shell/
  ui/surfaces.slint     every shell surface + the composed ShellRoot
  src/lib.rs            crate surface (include_modules + modules)
  src/model.rs          ShellState and every rule (dock order, Focus, badges,
                        overlays, credentials, power modes, clock formatting)
  src/search.rs         launcher ranking + inline calculator
  src/config.rs         F4 settings schema + layered TOML store
  src/desktop.rs        .desktop discovery for dock/launcher
  src/services.rs       nmcli / bluetoothctl / wpctl / brightnessctl / sysfs
  src/app.rs            ShellController: model <-> Slint, settings, services
  src/render.rs         headless screenshots of every surface
  src/main.rs           `aether-shell --render <dir>` / `--status`
  tests/interaction.rs  headless widget-driving test for all surfaces
```

## Running

```bash
cargo run -p aether-shell -- --render docs/2026/11/23/results/screenshots
cargo run -p aether-shell -- --status
cargo test -p aether-shell
```

`--render` uses a small Slint `Platform` over the software renderer, so it works
on the macOS dev host and on a headless Linux runner. Each surface writes a PNG
(`shell-desktop.png`, `shell-launcher.png`, …).

## Surface host

Today `ShellRoot` composes all surfaces in one window: good for review,
screenshots and tests. On the device each surface becomes its own layer-shell
surface (menu bar top, dock bottom, panels overlay) through the custom Slint
platform proven by spike S1. That host lands with the compositor's layer-shell
work (P3-T02, session S2); nothing in this crate changes for it — the same
`.slint` components are mounted with `layer: top/bottom/overlay`.

## Settings (F4)

Every settings row shows the file and key it writes. The schema is
`src/config.rs::SETTINGS`; adding an entry is the only change needed to make a
setting visible in the app and in launcher search. Writes are atomic and a file
that fails to parse is never overwritten. Defaults ship in the schema; user
overrides live under `~/.config/aether/`.

## Icons and design

Tokens come from design D1 (`ui/design.slint`): 4 px grid, 28 px menu bar,
64 px dock, 340 px control centre, 360 px notifications, 640 px launcher, glass
tiers that degrade to tint + edge when the compositor cannot blur.

All icon geometry is original (line icons on a 24×24 grid) — no copied assets.

## Accessibility

Interactive widgets set `accessible-role`, `accessible-label` (and
checked/value where relevant), which also gives the headless tests stable
handles. Keyboard navigation and AT-SPI come with the compositor session
(P3/P4 follow-up); `reduce-transparency` forces the minimal glass tier.

## Known gaps

- Layer-shell host + real input (P3-T02, S2).
- Battery polling loop and power policy live in `powerd` (P5, S4); the shell
  reads one sample at startup and shows it.
- Settings panes currently expose toggles and cycling select rows; free-text
  and numeric editors land with the full settings work.
- App launch spawns the `.desktop` exec when enabled; window management and
  focus hand-off belong to the compositor.
