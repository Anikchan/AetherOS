# D6 — apps: launcher launch path + the two weakest default apps

Session: `apps` (overnight parallel run, 2026-11-27).
Territory: `apps/**`, this file. No `shell/**`, `ui/**`, `os-build/**` or
`test.sh` changes.

## 1. Launcher → app launch (new: `apps/launcher-registry`)

The shell built a launcher model from `.desktop` entries but had no apps-side
contract for what a launcher id is and how to start it. This session adds that
seam as a new workspace crate `aether-launcher-registry`:

| API | Purpose |
| --- | --- |
| `entries()` | 11 desktop-entry-like records (`id`, `name`, `exec`, `icon`, `crate_dir`, `categories`, `terminal`) |
| `resolve(id)` | id → record |
| `resolve_exec(exec)` | executable → record |
| `launch_argv(id)` | the argv the shell must spawn: `<exec> --launch <id>` |
| `desktop_file(entry)` | the entry rendered as a `.desktop` file for the image overlay |
| `launch_outcome(id, exec)` | validate the pair, return the marker or a typed error |
| `handle_launch_arg(exec, args)` | one-line `--launch` handler every app calls |

Every app binary (all 11: files, terminal, browser, text, calculator, images,
documents, player, calendar, archive, taskmanager) now starts with:

```rust
if let Some(code) =
    aether_launcher_registry::handle_launch_arg("aether-<app>", std::env::args().skip(1))
{
    std::process::exit(code);
}
```

`<app> --launch <id>` prints `AETHER_LAUNCH <id>` and exits 0 in headless
builds; a window host keeps the same argv and continues into the Slint event
loop. An unknown id, a missing id, or an id owned by another executable prints a
clear error and exits 2.

### What the shell session must change (not edited here)

`shell/src/app.rs`, `spawn_if_enabled` currently spawns `sh -c "exec {exec}"`.
Replace that spawn with the registry argv:

```rust
let argv = aether_launcher_registry::launch_argv(id)
    .unwrap_or_else(|| vec![exec.clone()]);
let _ = std::process::Command::new(&argv[0]).args(&argv[1..]).spawn();
```

and add `aether-launcher-registry = { path = "../apps/launcher-registry" }` to
`shell/Cargo.toml`. Optional follow-ups for the shell owner:
`desktop::default_apps()` can now be generated from `entries()` so ids, names
and icons cannot drift; `desktop_file()` is what the image overlay should ship
under `/usr/share/applications`.

## 2. Weakest two apps, chosen by evidence

Candidate gaps were ranked from the DA-AC table and the D6 notes, not
preference:

| Candidate | Evidence | Verdict |
| --- | --- | --- |
| Terminal | `DA-AC-07` is the **only** app criterion marked "Partial" (`F8-default-apps.md`) — resize/copy-paste were model hooks but exit codes were the raw `waitpid` word | **chosen** |
| Calculator | `DA-02` requires "scientific mode (v1)"; `eval.rs` had only `sqrt`/`abs`/`round` and `pi`/`e`, and the window had no scientific keys, angle unit or factorial | **chosen** |
| Files | SHL-12 copy/move/rename/trash + error paths already implemented and tested (2026-11-23 review fixed overwrite/back-forward) | no work needed |
| Text | DA-AC-01/02 both Pass; atomic save + undo tests present | no work needed |
| Calendar | Week view, alarms, zones and persistence shipped 2026-11-26 | no work needed |
| Images | DA-AC-04 Pass; zoom/next implemented | no work needed |

### 2.1 Terminal — exit codes and resize (DA-AC-07)

| File | Change |
| --- | --- |
| `apps/terminal/src/session.rs` | `ExitStatus { Exited(code), Signaled(sig) }` with `from_raw` (`WIFEXITED`/`WIFSIGNALED` decoding), `code()`, `is_success()`, `label()` (`exit 0`, `SIGTERM`, `signal 7`); `PtySession::wait()` now returns `Option<ExitStatus>` instead of the raw status |
| `apps/terminal/src/lib.rs` | `pump()` reaps the child, draws `[process exit N]` and sets the status note; `resize(cols, rows)` resizes the VT grid and the PTY (`SIGWINCH`); `write_pty()` forwards keystrokes/commands to the PTY; `note()` exposes the status line |
| `apps/terminal/tests/ui.rs` | real PTY: `exit 9` renders `[process exit 9]` and sets the note `Process exit 9`; grid resize asserted through `viewport()` and the window properties |

### 2.2 Calculator — scientific mode (DA-02 v1)

| File | Change |
| --- | --- |
| `apps/calculator/src/eval.rs` | `AngleMode { Radians, Degrees }` (+ `label`/`toggled`); `sin`, `cos`, `tan`, `ln`, `log`, `exp`; postfix factorial (`5!`); trig snapping so `cos(90°)` is `0`, not `6.1e-17`; domain errors for `ln(≤0)`, `log(≤0)`, non-integer/overflow factorials; `evaluate_mode` / `evaluate_display_mode` |
| `apps/calculator/src/lib.rs` | `angle` + `scientific` state, `DEG`/`RAD` key toggles the unit, header `scientific`/`standard` toggle, `angle_mode()`/`scientific()`/`toggle_scientific()`, preview and `=` evaluate in the selected unit |
| `apps/calculator/ui/app.slint` | header mode toggle (`Scientific mode`), scientific keypad (`sin( cos( tan( x!` / `ln( log( eˣ DEG-RAD`), window 420×720 |
| `apps/calculator/tests/ui.rs` | headless clicks: `sin(30)=` → `0.5` in DEG, toggle → `RAD`, `5!=` → `120`, header hides the keypad |

Existing DA-AC-03 acceptance (50 cases) still passes; the one test that asserted
`tan` was unknown now uses `cbrt` as the unknown-function probe, because `tan`
is implemented.

## 3. Verification (run on this host)

| Check | Command | Result |
| --- | --- | --- |
| Registry unit tests (resolution, argv, `.desktop`, drift guard) | `LIBRARY_PATH=/opt/homebrew/lib cargo test -p aether-launcher-registry` | 6 passed |
| All app suites | `cargo test -p aether-files … -p aether-taskmanager -p aether-launcher-registry` (11 apps + registry) | all green, no failures |
| Terminal only | `cargo test -p aether-terminal` | 17 lib + 1 headless UI |
| Calculator only | `cargo test -p aether-calculator` | 12 lib + 2 acceptance + 1 headless UI |
| `--launch` handshake, every app | `cargo run -q -p aether-<app> -- --launch aether-<app>` × 11 | `AETHER_LAUNCH aether-<app>`, exit 0 |
| Rejection paths | `aether-files --launch aether-terminal`; `aether-files --launch` | exit 2 with `'aether-files' cannot start 'aether-terminal'; use 'aether-terminal'` and `--launch needs an app id` |
| Headless render, every app | `cargo run -q -p aether-<app> -- --render /tmp/aether-render-check` × 10 + taskmanager | every `AETHER_*_RENDERED` marker present (`app-calculator.png` 420×720 with the scientific row, activity-manager 9 surfaces) |

## 4. Risks and honest status

- `--launch` exits after the marker until the interactive window host (P3-T02)
  exists. The argv and validation are final, so the shell spawn line does not
  need to change again when the host lands.
- The shell still holds its own `default_apps()` list; until it depends on this
  registry, ids/exec names can drift. The registry's drift guard covers the
  apps side only.
- Settings is not in the registry: it belongs to the shell (`shell/settings`),
  not an app crate.
- Activity Manager is registered but is not in the shell's pinned dock list;
  the shell decides presentation.
- `Cargo.lock` is a shared file: the committed update for the new member also
  carries the shell session's Wayland resolution lines that were already in the
  working tree.
- DA-AC-09/10 (launch time, idle cost) and the on-device clipboard still need
  the device/host; nothing here changes those gates.
