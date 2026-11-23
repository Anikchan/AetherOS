//! Sample Markdown used by the screenshot renderer and the UI test fixtures.
//!
//! The documents are plain text constants so the demo never depends on the
//! working directory. `DocumentsController::with_demo_content` writes them to a
//! temporary directory first, which keeps the recent-file list honest: clicking
//! an entry opens a real file.

pub const FIELD_NOTES: &str = r#"# Field notes — AetherOS shell

These notes track the interface work for the 0.1.0 image. They are written in
Markdown so the Documents app can render them, and every page is measured in
rendered lines rather than pixels, which keeps pagination stable across zoom
levels.

## Why glass, and why not more of it

Full glass is only used for floating panels. Surfaces that stay on screen — the
menu bar, the dock, long lists — use the reduced tier, because blur under text
tires the eye after a few minutes of reading.

> Design is finished when there is nothing left to remove, not when there is
> nothing left to add.

The compositor owns the blur; the shell only supplies the tint and the edge
highlight. When the blur extension is absent the panel degrades to a solid
surface with the same geometry, so no layout shifts.

### Tier rules

- **Full** — floating panels over the wallpaper: launcher, control centre.
- **Reduced** — persistent chrome: menu bar, dock, notification centre.
- **Minimal** — anything that must stay legible while the desktop is busy.

## Type scale

The scale is one family, six sizes and two weights. Monospace is reserved for
code, logs and paths.

1. Hero at 28 px for empty-state titles.
2. Large at 20 px for window and page titles.
3. Title at 15 px for section headers.
4. Body at 13 px for everything readable.
5. Small at 11 px for metadata only.

---

## Keyboard first

Every surface is reachable without a pointer. The launcher is the fastest path:
`Super+Space`, type three characters, `Enter`. The same pattern works in the
Documents app for search and page jumps.

```rust
// The shell keeps one controller per surface; callbacks never own state.
let controller = ShellController::with_demo_content(ui, services, store);
controller.open(Overlay::Launcher);
controller.search("cal");
```

## Glass and contrast

Contrast is checked against the darkest wallpaper first; the lightest wallpaper
gets a scrim behind text. The glass tint is 72 percent opaque at the full tier,
86 at reduced and 96 at minimal, which is enough to clear a 4.5:1 ratio for body
copy.

- body copy on glass: 12.4:1
- muted metadata on glass: 6.1:1
- accent on glass: 4.9:1
- danger on glass: 4.6:1

## Motion

Motion is short and linear. Anything longer than 240 ms feels slow when the
launcher opens twenty times an hour. Reduce-motion turns the transitions into
cross-fades of the same duration.

## Open questions

1. Should the dock auto-hide on small displays?
2. Do we need a compact density for the settings app?
3. Can the notification centre reuse the control centre geometry?

> Ship the boring version first. Polish is a second pass with real content.

## Release checklist

- [x] Design tokens frozen for 0.1.0
- [x] Every surface renders headlessly in CI
- [ ] On-device blur verified on Intel and AMD
- [ ] Accessibility pass with Orca
- [ ] Screenshots refreshed in the results folder

The last item is deliberately manual: screenshots are evidence, and evidence
must be inspected by a person before it is published.
"#;

pub const DESIGN_REVIEW: &str = r#"# Design review — 23 November

Attendees: design, shell, compositor.

## Decisions

- Keep the 4 px grid everywhere, including icon internals.
- The dock stays 64 px; the extra height is for the running indicator.
- Window shadows are drawn by the compositor, not by the app.

## Follow-ups

1. Contrast table for the light wallpaper.
2. Motion audit of the launcher.
3. Glass fallback screenshots for the minimal tier.

> One source of truth per surface: the token file wins over the mockup.
"#;

pub const RFC_PAGINATION: &str = r#"# RFC 014 — Line-budget pagination

## Summary

The documents viewer paginates by estimated rendered lines instead of measuring
text. Each block declares a line cost and a page holds a fixed budget, so page
counts are stable across fonts and platforms.

## Motivation

Measuring text requires a font stack per platform, which the base image does not
guarantee. A line budget is deterministic, cheap and testable.

## Specification

- A page holds 44 rendered lines by default.
- Headings cost three lines, paragraphs cost their wrapped length plus one.
- Code blocks cost one line per source line plus two for padding.
- A block never splits across pages.

## Alternatives

A pixel-perfect engine with harfbuzz shaping was rejected for the prototype: it
adds a dependency, a font cache and a cache invalidation problem, for a viewer
that mostly shows notes.
"#;
