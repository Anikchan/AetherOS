# apps — AetherOS default apps (P4)

One crate per default app. Each crate follows the same shape:

```
apps/<name>/
  Cargo.toml            package `aether-<name>`; slint + aether-ui + build script
  build.rs              compiles ui/app.slint with debug info (element ids)
  ui/app.slint          the app window (Window component, `<Name>App`)
  src/lib.rs            include_modules! + core modules + controller glue
  src/main.rs           `--render <dir>` writes app-<name>.png; `--help`
  tests/ui.rs           one headless interaction test (testing backend)
```

Core logic lives in plain Rust modules with unit tests; the `.slint` file is
presentation and callbacks only. That keeps every rule testable without a
display server and makes the app port trivial when the compositor session
arrives.

| Task | Crate | App |
| --- | --- | --- |
| P4-T08 | `apps/files` | Files |
| P4-T10 | `apps/text` | AetherPad |
| P4-T11 | `apps/calculator` | Calculator |
| P4-T12 | `apps/images` | Images |
| P4-T13 | `apps/documents` | Documents / PDF |
| P4-T14 | `apps/player` | Player |
| P4-T15 | `apps/terminal` | Terminal |
| P4-T16 | `apps/calendar` | Calendar & Clock |
| P4-T17 | `apps/archive` | Archive |
| P4-T18 | `apps/browser` | Browser |

Platform apps for the P6 app platform (`apps/pkg`, `apps/store`, `apps/sandbox`,
`apps/runner`) belong to session S4 and are separate crates.

## Launcher registry and `--launch`

`apps/launcher-registry` (`aether-launcher-registry`) is the apps-side seam for
the shell launcher. It lists every default app as a desktop-entry-like record:

```rust
pub struct Entry { id, name, exec, icon, crate_dir, categories, terminal }
```

| API | Purpose |
| --- | --- |
| `entries()` | all default apps, in launcher order |
| `resolve(id)` | id → record (`aether-files`, `aether-text`, …) |
| `launch_argv(id)` | the argv the shell must spawn: `<exec> --launch <id>` |
| `desktop_file(entry)` | the entry rendered as a `.desktop` file |
| `handle_launch_arg(exec, args)` | the one-line entry point every app calls |

Every app binary accepts `--launch <id>` before its own CLI. The app validates
that the id belongs to it and prints `AETHER_LAUNCH <id>`; headless builds exit
0 after the marker, and the interactive window host (P3-T02) continues into the
Slint event loop with the same argv. Wrong ids and mismatched executables print
a clear error and exit 2.

```bash
cargo run -q -p aether-files -- --launch aether-files   # AETHER_LAUNCH aether-files
```

The shell spawns that argv instead of guessing an `Exec` string. The integration
line (in `shell/src/app.rs`, inside `spawn_if_enabled`) is:

```rust
let argv = aether_launcher_registry::launch_argv(id)
    .unwrap_or_else(|| vec![exec.clone()]);
let _ = std::process::Command::new(&argv[0]).args(&argv[1..]).spawn();
```

`apps/launcher-registry/src/lib.rs` has a drift guard: its tests read every
`apps/<crate>/Cargo.toml` and `src/main.rs` and fail if a registered app loses
the `--launch` handshake or renames its binary.

## Conventions

- Import the design system with a relative path:
  `import { Tokens, AetherButton } from "../../../ui/design.slint";`
- Dark theme only; use `Tokens` colours and `Tokens.space(n)` (4 px grid). No
  hard-coded hex values except alpha tints of tokens.
- Interactive elements set `accessible-label`; tests query by label because
  element ids repeat per component definition.
- Use `Flickable` rather than `ScrollView` (std-widgets needs image decoders
  that the workspace does not enable).
- In a non-layout parent an element without explicit `x`/`y` is centred —
  always set the position or use a layout.
- Percent values are invalid for `x`/`y`/`border-radius`; use
  `parent.width * 0.4` and single-value radii.
- No per-side borders on `Rectangle`; draw a 1 px child rectangle.

## Render an app

```bash
cargo run -p aether-text -- --render docs/2026/11/23/results/screenshots
```

Screenshots land in `docs/2026/11/23/results/screenshots/app-<name>.png` and are
the P4 visual evidence.
