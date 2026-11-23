# aether-ui — AetherOS design system (P4)

The shared visual language for the shell and every default app: design tokens
from D1, original icon geometry, the widget kit, and a headless renderer used to
produce P4 evidence screenshots.

## Files

```
ui/
  design.slint   tokens (global Tokens), icons (Icons + Icon), widgets, DesignGallery
  src/lib.rs     include_modules + headless renderer (render::render_png)
  src/theme.rs   glass tier + appearance rules (unit-tested)
  build.rs       compiles design.slint with element ids
  examples/gallery.rs  renders the gallery PNG
```

## Tokens (design D1 section 5)

Dark theme defaults, all as `Tokens.*`:

- Colours: `background`, `surface`, `surface-high`, `surface-low`, `text`,
  `text-muted`, `text-faint`, `accent`, `accent-soft`, `accent-strong`,
  `danger`, `success`, `warning`, glass tints/edges, `divider`, `hover`,
  `pressed`, `scrim`.
- Radii: `radius-panel` 18, `radius-menu` 12, `radius-button` 8, `radius-pill`.
- Metrics: `menubar-height` 28, `dock-height` 64, `titlebar-height` 38,
  `controlcenter-width` 340, `notification-width` 360, `launcher-width` 640.
- Type: `font-body` 13, `font-small` 11, `font-title` 15, `font-large` 20,
  `font-hero` 28, `font-family`, `font-family-mono`.
- Motion: `motion-fast` 120 ms, `motion-base` 180 ms.
- Helpers: `Tokens.space(n)` (4 px grid), `Tokens.tier-opacity(tier)`.

Overrides come from `~/.config/aether/theme/tokens.toml` once the config loader
maps them; the schema already lists the keys (`theme.*`) and the settings app
edits them.

## Widgets

| Widget | Notes |
| --- | --- |
| `GlassPanel` | Glass tier panel; degrades from full glass to a solid tinted panel |
| `AetherButton` | Primary/secondary/disabled, optional leading icon |
| `IconButton` | 20–28 px square action with hover/pressed/toggled states |
| `ToggleSwitch` | Switch with `accessible-checked` |
| `AetherSlider` | 0..1 slider, `accessible-value` 0..100 |
| `SegmentedControl` | String segments, current index |
| `SearchField` | Two-way text, edited/accepted callbacks |
| `ListRow` | Icon + title + subtitle + selected state |
| `Badge`, `Pill`, `Divider`, `SectionLabel`, `MenuItem` | Small primitives |
| `Icon` | Any `IconKind` drawn from original 24×24 path data |

Every interactive widget sets `accessible-role` and `accessible-label`
(and checked/value where relevant) — that is both accessibility and the stable
handle used by the headless tests, because Slint element ids repeat per
component definition.

## Glass tiers

`GlassTier.full` → blurred glass (compositor blur), `reduced` → lighter glass,
`minimal` → solid tinted panel. `theme::Tier::choose()` forces minimal when
`reduce-transparency` is on and downgrades `full` on GPUs that cannot blur.
The compositor owns the actual backdrop blur (P3-T02/T05).

## Headless rendering

```rust
aether_ui::render::init()?;                 // install once
let ui = SomeApp::new()?;                   // create the component afterwards
let frame = aether_ui::render::render_png(&ui, 1200, 800, &path)?;
```

`render` installs a small `Platform` that creates a `MinimalSoftwareWindow` per
component and draws one frame with the software renderer. Works without a
display server, which is how every P4 screenshot is produced on the macOS host
and in CI.

```bash
cargo run -p aether-ui --example gallery docs/2026/11/23/results/screenshots/ui-gallery.png
```

## Notes for app authors

- Import with a relative path: `import { Tokens } from "../../../ui/design.slint";`
- Set explicit `x`/`y` in non-layout parents (unset positions are centred).
- Percent values are invalid for `x`, `y` and `border-radius`.
- No per-side borders; draw a 1 px child `Rectangle`.
- Use `Flickable`, not `ScrollView` (std-widgets needs image decoders that are
  deliberately disabled in this workspace).
- Keep logic in Rust; `.slint` is presentation plus callbacks.
