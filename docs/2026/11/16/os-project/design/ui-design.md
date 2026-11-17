# D1 — UI Design

Scope: visual language, layout, glass material, tokens, components, motion, accessibility. Feature behavior is in F1. Values marked "proposed" are starting points to be refined in P3 and P4 with real screenshots.

## 1. Design Principles

1. **Calm first.** The desktop stays quiet. Content is the focus; chrome is thin and translucent.
2. **Depth through glass.** Layers are separated by blur, tint, and light edges, not heavy shadows.
3. **Predictable.** Every control works the same way in every panel.
4. **Keyboard and mouse equal.** Every action has a shortcut and a visible control.
5. **Original.** Our own icons, fonts (open license), wallpapers, and shapes. No copied Apple assets.

## 2. Visual Language ("Glass")

- Panels use a translucent material: blurred background, a light tint, and a bright 1 px edge.
- Corners are continuous (squircle-like). Use rounded rectangles with a smooth curve as an approximation.
- Motion is spring-based. Items appear with a short scale and fade.
- Colors adapt to the wallpaper (tint sampling is a v1 feature).
- Visual reference: macOS 27, as stated by the owner (Q1). We take design cues (glass depth, layout, motion) and build original icons, fonts (open license), and wallpapers.

## 3. Layout and Grid

- Base unit: 4 px. All spacing is a multiple of 4.
- Menu bar: full width, 28 px high, top of the screen.
- Dock: bottom center, 64 px tall panel (icon size 48 px base, magnified up to 1.4× by default).
- Windows: title bar 38 px. Content area has 12 px inner padding for panels.
- Control center: 340 px wide, anchored to the menu bar tray.
- Notifications: 360 px wide, top right, stacked.
- Launcher: centered panel, 640 px wide, search field 44 px high.

```
+----------------------------------------------------------------+
| [Logo] App name  File Edit View ...        [Tray] [Clock]      | <- menu bar 28 px
+----------------------------------------------------------------+
|                                                                |
|     +--------------------------+        +------------------+   |
|     |  Window title bar  38px  |        | Notification     |   |
|     |--------------------------|        +------------------+   |
|     |                          |                              |
|     |     App content          |                              |
|     |                          |                              |
|     +--------------------------+                              |
|                                                                |
|          [ dock: icons with running indicators ]               |
+----------------------------------------------------------------+
```

## 4. Glass Material Specification

Each glass surface is composed of these layers, from back to front.

| Layer | Full tier | Reduced tier | Minimal tier | Notes |
| --- | --- | --- | --- | --- |
| Backdrop blur | Dual Kawase blur, radius 24 px (proposed) | Dual Kawase blur at half resolution, radius 16 px | None | Blur is computed by the compositor |
| Saturation boost | 1.2× | 1.1× | 1.0× | Keeps colors lively |
| Tint | 18% white (light) or 18% black (dark) | Same | Solid 92% opacity | Opacity values are proposed |
| Edge highlight | 1 px, 40% white, brighter at top edge | 1 px, 30% white | 1 px border, 20% white | Gives the glass edge |
| Edge refraction | Subtle displacement of backdrop at edges (shader) | None | None | Costliest effect; disabled in Reduced |
| Shadow | Soft, 12 px blur, 12% black | 8 px blur, 10% black | 4 px blur, 10% black | Keeps panels separated |
| Corner radius | 18 px (panels), 12 px (menus), 8 px (buttons) | Same | Same | |

Rules:
- Glass is never placed behind text-heavy document content.
- Glass panels over dark wallpaper must keep text contrast ≥ 4.5:1 (see section 9).
- The compositor applies blur only to visible regions (damage-based) to save GPU time.

## 5. Design Tokens

All tokens live in `~/.config/nexa/theme/tokens.toml` (F4 CUS-03). Defaults ship in `/usr/share/nexa/defaults/theme/`.

```toml
# tokens.toml (proposed defaults, dark theme shown)
[color]
background = "#0E1116"
surface    = "#1A1F27"
text       = "#F2F4F7"
text_muted = "#9AA4B2"
accent     = "#4C8DFF"
danger     = "#FF5C5C"
success    = "#3DD68C"

[glass]
blur_radius = 24
saturation = 1.2
tint_opacity = 0.18
edge_opacity = 0.40

[radius]
panel = 18
menu = 12
button = 8

[font]
family = "Inter"
size_body = 13
size_title = 15
```

Light theme uses the same token names with light values.

## 6. Typography

- Font: Inter (SIL OFL). Do not use Apple's SF family.
- Sizes: 11 (caption), 13 (body), 15 (title), 20 (heading), 28 (large title).
- Weight: 400 body, 500 labels, 600 titles.
- Monospace for terminal: a free font such as JetBrains Mono (OFL) or Iosevka (OFL). Confirm license in P3.

## 7. Components

| Component | Size | Behavior |
| --- | --- | --- |
| Menu bar | 28 px high, full width | Left: app name and menus. Right: tray, clock. Hover highlights items. |
| Dock icon | 48 px base, up to 1.4× magnified | Running dot below icon. Badge in top-right. Bounce once on launch request. |
| Window controls | 12 px circles (original shape) | Close, minimize, maximize. Hover shows symbol. Position configurable. |
| Launcher search | 44 px high field | Results list with keyboard selection. |
| Control center tile | 80 px square | Toggle state with accent color. |
| Slider | 4 px track, 16 px knob | Keyboard arrows change value by 5%. |
| Notification card | 360 px wide, auto height | Dismiss, action buttons (max 2). |
| Button | 28 px high (default) | Primary (accent fill), secondary (glass), destructive (danger). |
| Menu | Glass, 12 px radius | Keyboard navigation, submenus, shortcuts shown on the right. |

## 8. Motion

| Token | Duration | Use |
| --- | --- | --- |
| motion.fast | 120 ms | Hover, toggles |
| motion.base | 200 ms | Menus, panels appearing |
| motion.slow | 350 ms | Overview, workspace switch |

- Easing: spring with critical damping (proposed stiffness 300, damping 30).
- Reduce motion (SHL-18) replaces springs and scaling with a 120 ms fade.

## 9. Accessibility

| Requirement | Target |
| --- | --- |
| Text contrast | ≥ 4.5:1 for body text, ≥ 3:1 for large text |
| Keyboard navigation | Every shell feature reachable without a mouse |
| Screen reader | AT-SPI tree for menu bar, dock, launcher, control center, notifications |
| Reduce transparency | Switches to Minimal tier (F6) |
| Reduce motion | Fades only (section 8) |
| Text scaling | 100% to 200% for shell text |
| Focus indicator | 2 px accent outline, always visible |

## 10. Iconography

- Own icon set on a 24 px grid with 2 px strokes (proposed).
- Base icons may start from a permissively licensed set such as Lucide (ISC), with changes recorded in the license file.
- App icons follow the freedesktop icon specification so Linux apps show correctly.
- Do not use Apple SF Symbols or any other proprietary icon font.

## 11. Screens

| Screen | Owner feature | Notes |
| --- | --- | --- |
| Desktop with wallpaper | SHL-09, SHL-16 | Wallpaper stored in `~/Pictures/Wallpapers` |
| Lock screen | SHL-10 | Clock, user picture, password field |
| Launcher | SHL-03 | Search-first; grid of apps below |
| Control center | SHL-04 | Tiles for network, Bluetooth, Focus, display; sliders for sound and brightness |
| Notification center | SHL-05 | Grouped cards, Focus schedule at top |
| Overview | SHL-08 | Workspace strip on top, window grid below |
| Settings | SHL-11 | Sidebar categories, content on the right |
| Activity monitor | PRM-10 | Table with state, memory, CPU; color codes for states |
| Installer | F5 | Step-by-step wizard, plain language, dry-run preview |

## 12. Design Process

1. Low-fidelity layout sketches for each screen.
2. Token-based prototype in the shell (P4).
3. Test on reference machines at all three effect tiers.
4. Accessibility review against section 9 before each milestone.
5. Visual review by the owner against the macOS 27 visual reference (Q1, answered).

Initial original SVG concepts: [desktop](mockups/desktop.svg), [launcher](mockups/launcher.svg), [control center](mockups/control-center.svg), [notifications](mockups/notifications.svg), and [installer](mockups/installer.svg). They are visual direction, not implementation specifications; dimensions and component behavior remain governed by this document and F1.
