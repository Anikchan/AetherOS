# D6 — Shell launcher wired to the apps registry (P4 launch path)

- Date: 2026-11-28 (IST, overnight run, second shell round)
- Session: `shell` (territory `shell/**`, this file)
- Depends on: `apps` session commit `68d2e87` — crate `apps/launcher-registry`
  (11 entries), `docs/2026/11/27/results/apps-launcher-and-gaps.md`
- Status: **Done.** Launcher ids, names, icons and argv now come from the
  registry; the shell spawns `<exec> --launch <id>` through an injectable seam;
  `aether-shell --launch <id>` is the documented hand-off CLI.

## 1. What changed

| File | Change |
| --- | --- |
| `shell/src/spawn.rs` *(new)* | The process seam. `launch_argv(id, exec)` returns the registry argv (`<exec> --launch <id>`) or, for ids the registry does not own, the app's `Exec` line run through `sh -c "exec …"`. `Spawner` is the injected seam; `ProcessSpawner` is the real one (`spawn`, never `wait`); `launch()` returns the argv, its `Source` and the outcome so the host log can name both. |
| `shell/src/app.rs` | `spawn_if_enabled` starts apps through the seam instead of `sh -c "exec {exec}"`; the log line is now `spawned <id> source=registry argv=<exe> --launch <id>`. New `set_spawner()` (installs the seam and enables spawning) and `launch_argv(id)`. Shell-owned ids (`Settings`) return before spawning — they are overlays, not processes. |
| `shell/src/desktop.rs` | The launcher model is generated from `aether_launcher_registry::entries()`: ids, names, `Exec` and icons (via the existing `icon_for` over the registry icon name and categories). `default_apps()` = registry apps + the shell-owned `aether-settings`; `PINNED_IDS`, `RUNNING_IDS` and `SHELL_OWNED_IDS` keep the dock policy explicitly shell-side. `merged_apps()` still lets discovered `.desktop` entries override metadata and still appends ids the registry does not know. |
| `shell/src/main.rs` | `aether-shell --launch <id>`: registry ids spawn `<exec> --launch <id>` and print `AETHER_SHELL_LAUNCH: id=… source=registry exec=… argv=…`; shell-owned ids print `… handled=shell`; an unknown id prints a clear message and exits **2**; a spawn failure exits 1. |
| `shell/Cargo.toml` | `aether-launcher-registry = { path = "../apps/launcher-registry" }`. |
| `Cargo.lock` | One added dependency line for `aether-shell` (now committed; the apps session's entries are already in `main`). |
| `shell/tests/launcher.rs` *(new, 4 tests)* | Registry ↔ shell model equality; the spawn seam; the controller launch path; the `--launch` CLI end to end. |

### Why the model is "registry + shell-owned", not "registry only"

The registry is the apps-side truth and owns 11 ids. `Settings` is a shell
surface (`aether-settings` has no binary and is not in the registry), so the
launcher model is the registry list in registry order plus
`desktop::SHELL_OWNED_IDS`. The new test asserts both directions — every model
id resolves through the registry or is explicitly shell-owned, and every
registry id is in the model — so the two can never drift silently. A practical
consequence: **`aether-taskmanager` ("Activity Manager") is now in the launcher**
(it was a P4 app the shell's hand-written list had missed).

Icons are generated, not copied: `icon_for("system-monitor", "Activity Manager",
…)` → `Activity`, `"text-editor"` → `Document`, and so on for all 11; the
registry's icon names were chosen to match the existing heuristic, so the dock
looks unchanged.

## 2. Transcript: `--launch`

```sh
$ aether-shell --launch aether-nope
aether-shell: unknown app id 'aether-nope'; it is not in the launcher registry (see apps/launcher-registry)
$ echo $?
2

$ aether-shell --launch            # no id
--launch needs an app id (for example aether-files)
$ echo $?
2

$ aether-shell --launch aether-settings
AETHER_SHELL_LAUNCH: id=aether-settings handled=shell
$ echo $?
0

$ aether-shell --launch aether-taskmanager     # dev host: app not installed
aether-shell: cannot start aether-taskmanager: No such file or directory (os error 2)
$ echo $?
1
```

With a stub `aether-files` on `PATH` (what the integration test does), the
success path is:

```
AETHER_SHELL_LAUNCH: id=aether-files source=registry exec=aether-files argv=--launch aether-files
# stub recorded: --launch aether-files
```

## 3. Transcript: the model and the seam

`aether-shell --status` on a dev profile (no `.desktop` files) still reports the
same dock (4 pinned + the demo running Settings = 5), now built from the
registry; the launcher lists the 11 registry apps plus Settings.

From `shell/tests/launcher.rs`:

- `the_shell_app_model_matches_the_registry`: for every app in
  `desktop::default_apps()`, `name`/`exec` equal the registry record, the icon
  equals `icon_for(registry icon name)`, and `spawn::launch_argv(id)` is exactly
  `registry::launch_argv(id)` with `Source::Registry`; the reverse direction
  (every registry id present) and the pinned dock list are asserted too.
- `a_launch_starts_the_registry_argv_exactly_once`: the injected recording
  spawner sees exactly `["aether-files", "--launch", "aether-files"]` once; an
  unowned id yields `["sh", "-c", "exec example --new"]`; the real
  `ProcessSpawner` starts `true --launch x` (`/bin/true`, `/usr/bin/true` on the
  macOS host) — no GUI and no AetherOS binary involved.
- `launching_from_the_shell_uses_the_registry_argv`: `ShellController::launch`
  → seam, one call, log line `spawned aether-calculator source=registry
  argv=aether-calculator --launch aether-calculator`; `aether-settings` spawns
  nothing; `launch_argv()` returns `None` for an unknown id.
- `the_launch_cli_hands_off_to_the_registry_argv`: the shipped binary starts a
  stub app from `PATH` that records its argv, plus the unknown/missing/shell-owned
  cases above.

## 4. Verification

```sh
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell -p aether-ui
# 179 passed, 0 failed   (170 before; +4 launcher integration, +4 spawn unit,
#                         +1 desktop registry-equality)
LIBRARY_PATH=/opt/homebrew/lib cargo check --workspace
# Finished `dev` profile ... (no errors, no new warnings in the shell)
LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-shell --test launcher
# test result: ok. 4 passed
```

## 5. What the image needs for the first graphical boot (HANDOFF)

1. **Binaries on `PATH`.** The shell starts `<exec> --launch <id>` for all 11
   registry ids: `aether-files`, `aether-terminal`, `aether-browser`,
   `aether-text`, `aether-calculator`, `aether-images`, `aether-documents`,
   `aether-player`, `aether-calendar`, `aether-archive`, `aether-taskmanager`.
   On the dev host `--launch aether-taskmanager` exits 1 with "cannot start" —
   that is the exact failure the image must not have.
2. **`.desktop` files (recommended).** `registry::desktop_file(entry)` renders
   each record with `X-Aether-Id`; shipping them under
   `/usr/share/applications/aether-*.desktop` makes other launchers and the
   shell's own discovery agree. The shell does not need them (the registry is
   compiled in) — without them it simply uses the built-in model.
3. **No registry JSON at runtime.** The registry is a Rust crate compiled into
   the shell, so there is no config file to install and nothing to keep in sync
   at runtime; adding an app is a code change in `apps/launcher-registry`.
4. **Apps must open a window when started with `--launch`.** In headless builds
   `handle_launch_arg` prints `AETHER_LAUNCH <id>` and exits 0 (that is what the
   shell's spawn sees on the dev host). For the graphical boot the app must keep
   the same argv and continue into its Slint window, otherwise a launcher click
   starts a process that exits immediately. This is the apps' xdg-shell/window
   host work, not a shell change.
5. **Session wiring.** The interactive shell host (`aether-shell --interactive`)
   already enables spawning, so dock/launcher activation on the image will use
   the registry argv with no further change; `aether-shell --launch <id>` is the
   entry point to document for file managers and scripts.

## 6. Remaining shell-side follow-ups

1. `launch_if_enabled` currently logs the spawn and moves on; it does not track
   the child (PID, exit, "app closed" → dock dot). The seam returns only
   `Result<(), String>`, so supervision needs a second seam method later.
2. Discovered `.desktop` apps that are not in the registry still spawn through
   `sh -c "exec …"` (unchanged behaviour, quoted arguments preserved). If the
   P6 package platform lands a dynamic registry, `spawn::launch_argv` is the one
   place to extend.
3. The Settings dock item is shell-owned and never spawned; if P4 later gives
   Settings its own binary, move it out of `SHELL_OWNED_IDS` and into the
   registry.
