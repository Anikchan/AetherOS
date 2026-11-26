# SHL-18 — Accessibility: reduce motion, reduce transparency, text scaling, keyboard navigation

- Date: 2026-11-27
- Session: S3 (`session - 3 (Nov27)`)
- Tasks: F1 / SHL-18 base + v1, design D1 sections 8–9
- Status: implemented and headless-verified; on-device visual/AT-SPI steps pending
  (hardware paused per `docs/2026/11/26/session-kickoff.md`)

## What shipped

| Area | Code | Contract |
| --- | --- | --- |
| Resolved settings | `shell/src/a11y.rs` | `theme.reduce-motion`, `theme.reduce-transparency`, `theme.text-scale` (100–200 %, clamped) → [`A11ySettings`] |
| Design tokens | `ui/design.slint` | `Tokens.text-scale`/`Tokens.reduce-motion` drive `font-*`, `menubar-height`, `motion-base`, `motion-slide`, `motion-fade`, `focus-ring` |
| Surfaces | `shell/ui/surfaces.slint` | one `Tokens` binding reaches every surface; focus props per surface; 2 px accent ring on the focused item; scaled panel/row heights |
| Keyboard | `shell/src/kbdnav.rs`, `ShellController::handle_key` | Tab/Shift+Tab across five surfaces, arrows wrap, Enter/Space activate, Escape closes |
| Settings app | `shell/src/config.rs` (new `theme.text-scale` row) | Appearance → Text scale cycles 100 % → 200 %; Reduce motion toggles |
| CLI / harness | `shell/src/main.rs` | `--a11y-status`, `--a11y-env`, `--status` accessibility lines |

## Motion rule (D1 §8)

`reduce-motion = false` → slides use `motion-slide = 180 ms`.
`reduce-motion = true` → `motion-slide = 0 ms` (movement removed) and every
remaining transition is a `motion-fade = 120 ms` fade; the dock's opacity
animation is bound to the fade so the reduced path still animates.

## Transparency rule (D1 §9)

`reduce-transparency = true` resolves to the Minimal tier regardless of
`theme.effects-tier`, the same value the effect-tier control already owned
(SHL-AC-07). The accessibility summary reads the same key, so the two modules
can never disagree.

## Text scaling (D1 §9)

`theme.text-scale` is stored as a percentage string (`"150%"`) so the settings
row can show it; the parser also accepts `"1.5"` and TOML floats for hand-edited
files. Values are clamped to 100–200 %. Scaling applies to `font-body`,
`font-small`, `font-title`, `font-large`, `font-hero`, the menu bar height, and
the rows the shell owns (settings rows, menu items, list rows, launcher rows,
notification cards, quick tiles) so text does not clip at the 2× step.

## Keyboard coverage

| Surface | Focusables |
| --- | --- |
| Menu bar | AetherOS menu, 5 menu titles, Wi-Fi, Bluetooth, Sound, Battery, Notifications, Clock |
| Dock | pinned then running apps |
| Launcher | result rows (arrows move the search selection) |
| Control centre | Wi-Fi, Bluetooth, Focus, Night light, volume, brightness, power mode, effects, Settings, Lock |
| Notifications | Clear all, one row per card, Focus settings |

Empty surfaces are skipped by Tab, so focus never lands on nothing. The
launcher is skipped until it has results; the dock is skipped when empty.

## Verification (commands and results)

```sh
cargo test -p aether-shell --lib a11y     # 6 passed
cargo test -p aether-shell --lib kbdnav   # 5 passed
cargo test -p aether-shell --test a11y    # 2 passed (status + env through the binary)
cargo test -p aether-shell --test a11y_ui # 1 passed (settings, tokens, keys, focus ring)
cargo test -p aether-shell -p aether-ui   # 129 + 0 + 0 lib tests, every integration test green
```

Measured on the development host (macOS, Slint testing backend):

| Check | Result |
| --- | --- |
| Fresh profile `A11Y_TEXT_SCALE` | `100% scale=1.00` |
| Text scale 150 % → `Tokens.font-body` | `19.5 px` (13 × 1.5) |
| Text scale 150 % → `Tokens.menubar-height` | `42 px` (28 × 1.5) |
| Reduce motion → slide / fade / motion | `0 / 120 / 120 ms` |
| Reduce transparency → effective tier | `minimal` |
| Focus ring | `2 px`, `#4C8DFF` (`Tokens.focus-ring`) |
| Status marker order | settings → keyboard plan → focus → `AETHER_SHELL_A11Y_OK` (last) |

Visual evidence (software renderer, development host):

- `docs/2026/11/27/results/screenshots/shell-a11y-focus-dock.png` — 150 % text with
  the 2 px accent ring on the focused dock item.
- `docs/2026/11/27/results/screenshots/shell-a11y-text-150.png` — control centre
  at 150 %.
- `docs/2026/11/27/results/screenshots/shell-a11y-text-200.png` — control centre
  at 200 % (tiles widened with the panel; no clipped labels).
- `docs/2026/11/27/results/screenshots/shell-a11y-settings-200.png` — Settings →
  Appearance at 200 %, showing the new "Text scale" row at 200 %.

Reproduce with:

```sh
cargo run -p aether-shell -- --render-a11y docs/2026/11/27/results/screenshots
```

The `a11y_ui` test drives the real widget tree: it clicks the Appearance rows,
asserts the persisted TOML, asserts the `Tokens` globals, injects Tab/arrow/
Enter keys, checks that the focused dock tile exposes `Focused` in its
accessible description, and that focus visits all five surfaces in order.

## Hardware-gated (not claimed)

1. Visual confirmation that the 2 px ring is legible on the Dell panel at 1× and
   1.5×, and that no surface clips at 200 % text.
2. AT-SPI announcement of the focused item (Orca) — needs the image's AT-SPI
   session; the shell sets `accessible-label`/`accessible-description` today.
3. The compositor-side keyboard grab that feeds XKB keysyms into
   `ShellController::handle_key` on the real layer-surface host; the headless
   tests inject the same key strings.

Harness steps: `docs/2026/11/26/results/on-device-acceptance-harness.md`
§1d / SHL-AC-12.

## Files changed

- `shell/src/a11y.rs` (new), `shell/src/kbdnav.rs` (new)
- `shell/src/app.rs`, `shell/src/config.rs`, `shell/src/model.rs`,
  `shell/src/lib.rs`, `shell/src/main.rs`
- `shell/ui/surfaces.slint`, `ui/design.slint`
- `shell/tests/a11y.rs` (new), `shell/tests/a11y_ui.rs` (new)
- `docs/2026/11/16/os-project/features/01-desktop-shell.md` (§13, SHL-AC-12)
- `docs/2026/11/26/results/on-device-acceptance-harness.md` (§1d, SHL-AC-12)
