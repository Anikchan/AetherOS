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

`<app> --launch <id>` prints `AETHER_LAUNCH <id>` and continues into the app
(round 2 changed this from an exit-after-marker handshake; §6 has the current
contract). Wrong ids and mismatched executables print a clear error and exit 2.

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
| `--launch` handshake, every app (round 1, now `--launch-check`) | `cargo run -q -p aether-<app> -- --launch-check aether-<app>` × 11 | `AETHER_LAUNCH aether-<app>`, exit 0 |
| Rejection paths | `aether-files --launch-check aether-nope`; `aether-files --launch aether-terminal`; `aether-files --launch` | exit 2 with a message naming `apps/launcher-registry` |
| Headless render, every app | `cargo run -q -p aether-<app> -- --render /tmp/aether-render-check` × 10 + taskmanager | every `AETHER_*_RENDERED` marker present (`app-calculator.png` 420×720 with the scientific row, activity-manager 9 surfaces) |

## 4. Risks and honest status

- `--launch` no longer exits after the marker (round 2); the app keeps running
  into its normal path. A window appears only once the interactive host
  (P3-T02) is attached; until then `--launch <id> --render <dir>` is the
  headless evidence. The argv and validation are final, so the shell spawn line
  does not need to change again when the host lands.
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

## 5. Flaky test fixed — `aether-files` navigation sandbox (2026-11-28)

CI's new guards caught `navigation_tests::back_and_forward_track_history_in_both_directions`
failing about **1 run in 30** with
`Io { path: ".../aether-files-nav-<pid>-<nanos>", kind: NotFound }`.

Root cause: the test sandbox in `apps/files/src/lib.rs` named its directory
`aether-files-nav-<pid>-<now.as_nanos()>`. macOS clocks are microsecond-granular
(values end in `000`), so two tests running in parallel inside the same test
binary can sample the same timestamp, get the same path, and the first test's
trailing `remove_dir_all` deletes the second test's tree mid-run.

| File | Change |
| --- | --- |
| `apps/files/src/lib.rs` | `sandbox()` now appends a process-wide `AtomicUsize` counter to the name and creates the root with `fs::create_dir`, retrying on `AlreadyExists`; a new `cleanup(&Path)` helper ignores an already-removed tree. Assertions are unchanged. |

Verification (this host, 2026-11-28):

| Check | Command | Result |
| --- | --- | --- |
| Old scheme collision probe (64 threads × 5000 `now().as_nanos()` samples, old `pid-nanos` naming) | `rustc -O nav_collide.rs && ./nav_collide` | 190 462 / 320 000 samples produced a duplicate name (≈60% collision rate per sample; only the narrow test-overlap window makes it ≈1/30 in the suite) |
| New sandbox, repeat run | `for i in $(seq 1 30); do LIBRARY_PATH=/opt/homebrew/lib cargo test -q -p aether-files --lib; done` | 30/30 green, 0 failures |
| Whole crate once | `LIBRARY_PATH=/opt/homebrew/lib cargo test -q -p aether-files` | 14 lib + 1 headless UI + 0 doc tests, all green |
| Lint | `cargo clippy -q -p aether-files --all-targets` | no warnings in the touched region (one pre-existing `needless_borrow` in `tests/ui.rs`) |

Before: ≈3.3% failure rate (1/30 per CI observation, mechanism confirmed by the
collision probe). After: 0/30.

## 6. Round 2 — `--launch` is a real launch (2026-11-28)

Defect found while wiring the shell: the round-1 `--launch` printed the marker
and exited, so a dock click would start the binary and the window would never
appear. The contract is now split in two:

| Flag | Behaviour |
| --- | --- |
| `--launch <id>` | validate that `<id>` is this executable's registry id, print `AETHER_LAUNCH <id>`, remove the launch flags and **continue into the normal app path** |
| `--launch-check <id>` | dry handshake: validate, print the marker, exit 0 (tests and packaging checks) |
| wrong / foreign / missing id | stderr message naming `apps/launcher-registry`, exit 2 |

### Changes

| File | Change |
| --- | --- |
| `apps/launcher-registry/src/lib.rs` | `LaunchDecision { Continue { marker }, Exit { code, marker, message } }` (pure, with `marker()`/`exit_code()`), `launch_decision(exec, args)`, `LaunchStartup { exit, rest }`; `handle_launch_arg` now applies the decision, strips `--launch`/`--launch-check` + id from `rest`, and returns instead of exiting on a real launch; rejection messages name the registry |
| all 11 `apps/<crate>/src/main.rs` | call `handle_launch_arg`, stop only when `startup.exit` is set, parse `startup.rest` (taskmanager's `parse_args` now takes the argv) |
| `apps/<crate>/tests/launch.rs` (new × 11) | own id → `Continue` + `AETHER_LAUNCH <id>`, `--launch-check` own → `Exit(0)` + marker, foreign id → `Exit(2)`, missing id → `Exit(2)`, no flag → `Continue`, `launch_argv` = `<exec> --launch <id>` |
| `apps/launcher-registry/src/lib.rs` tests | `launch_decision` table and `handle_launch_arg` stripping/exit behaviour |
| `apps/README.md` | the two-flag contract, app `main` snippet, unchanged shell spawn line |

Rendering is untouched: after the marker the process runs exactly the code it
runs without a launch flag. The image/session spawns `<exec> --launch <id>`;
headless evidence adds `--render <dir>` (`--launch <id> --render <dir>`), which
is verified below.

### Verification (2026-11-28)

| Check | Command | Result |
| --- | --- | --- |
| Registry + all 11 app suites | `cargo test -p aether-files … -p aether-taskmanager -p aether-launcher-registry` | all green (11 new `launch` tests, 1 per app; registry 7) |
| Own id, dry | `cargo run -q -p aether-files -- --launch-check aether-files` | `AETHER_LAUNCH aether-files`, exit 0 |
| Unknown id, dry | `cargo run -q -p aether-files -- --launch-check aether-nope` | `aether-files: unknown app id 'aether-nope' (launcher ids live in apps/launcher-registry/src/lib.rs)`, exit 2 |
| Real launch keeps running | `cargo run -q -p aether-files -- --launch aether-files` | marker, then the normal usage path, exit 0 (no handler exit) |
| Real launch + render | `cargo run -q -p aether-files -- --launch aether-files --render /tmp/aether-launch-r2` | marker, `AETHER_APP_FILES_RENDERED … (1200x800)` |
| Real launch + render, Vec CLI | `aether-calculator --launch aether-calculator --render …`, `aether-taskmanager --launch aether-taskmanager --render …` | marker + `AETHER_APP_CALCULATOR_RENDERED 420x720`, marker + Activity Manager surfaces |
| Every app's dry handshake | `--launch-check <own id>` × 11 | marker, exit 0; `--launch-check aether-nope` × 11 → exit 2 |

**Shell note:** the spawn command stays exactly `<exec> --launch <id>`; no shell
change is needed. The registry path in this report is
`apps/launcher-registry/src/lib.rs`.

