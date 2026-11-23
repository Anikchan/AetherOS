# P4 — Desktop shell and default apps (D6 results)

- Date: 2026-11-23
- Session: S3 / session-1 (autonomous run)
- Scope: P4-T01..T09 (desktop shell), P4-T10..T18 (default apps, browser core)
- Host: macOS (Apple silicon) for headless verification; Linux verification via
  `x86_64-unknown-linux-gnu` type-check and CI (`cargo test --workspace`)

## 1. What was built

The P4 block is implemented as Rust logic + Slint presentation, with a shared
design system so the shell and every app look like one product.

| Layer | Crate | Output |
| --- | --- | --- |
| Design system | `ui/` (`aether-ui`) | D1 tokens, original 24×24 line icons, widget kit, glass tiers, headless PNG renderer |
| Shell | `shell/` (`aether-shell`) | Menu bar, dock, launcher, control centre, notification centre, lock screen, overview, settings app, composed `ShellRoot` |
| Browser | `apps/browser` | Tabs, address bar, bookmarks, history, downloads, private tabs, session restore, engine seam |
| Default apps | `apps/{text,calculator,files,images,documents,player,terminal,calendar,archive}` | Per-app crates with tested core modules and app windows |

### Shell surfaces (P4-T01..T07, T09)

- **Menu bar (T01)** — app menus, tray items (Wi-Fi, Bluetooth, sound, battery,
  notifications), clock, unread badge, drop-down menus, Focus pill.
- **Dock (T02)** — pinned then running apps without duplicates, running dots,
  pinned hairlines, badges, auto-hide state.
- **Launcher (T03)** — 640 px panel, ranked search over apps + settings +
  inline calculator, keyboard selection model, result kinds.
- **Control centre (T04)** — 2×2 quick tiles (Wi-Fi, Bluetooth, Focus, night
  light), volume/brightness sliders, battery state, power-profile
  segmentation, settings/lock actions.
- **Notification centre (T05)** — grouped cards, per-item dismiss, clear all,
  Focus suppression banner.
- **Lock screen (T06)** — clock/date, user, masked password field, wrong-password
  rejection with error text, constant-time verification, success unlock.
- **Settings (T07)** — categories generated from the F4 schema; every row shows
  the file and key it writes; toggles write TOML atomically, select rows cycle;
  invalid files are never overwritten.
- **Overview (T09)** — window cards, workspace strip, activation.

### Default apps (P4-T08, T10..T18)

Each app has a Slint-free core module with unit tests, a controller that owns
the window, and a `--render` entry point for evidence screenshots.

| Task | Crate | Core (tested) | Unit tests | UI test | Screenshot |
| --- | --- | --- | --- | --- | --- |
| P4-T08 | `apps/files` | Sandboxed listing, navigation, sorting, search, multi-select, copy/cut/paste, rename, trash, traversal + symlink escape guards | 11 | yes | `app-files.png` |
| P4-T10 | `apps/text` | Char-indexed buffer, bounded undo/redo, unicode counts, single-pass find/replace (10 MB in <100 ms), encoding detection (UTF-8/BOM/UTF-16/CP-1252) with encoding-preserving saves, autosave, recent files | 18 | yes | `app-text.png` |
| P4-T11 | `apps/calculator` | Recursive-descent evaluator (precedence, right-assoc `^`, functions, typed errors), history, memory | 11 | yes | `app-calculator.png` |
| P4-T12 | `apps/images` | Decode png/jpeg/gif/bmp, metadata, fit/zoom clamp, lossless rotate/flip, folder order, thumbnail cache | 9 | yes | `app-images.png` |
| P4-T13 | `apps/documents` | Markdown block+inline parser, line-budget pagination, zoom, search with page mapping, poppler adapter (detect/commands, NotAvailable when absent) | 15 | yes | `app-documents.png`, `app-documents-page2.png` |
| P4-T14 | `apps/player` | Queue, seeded shuffle, repeat modes, transport with auto-advance, seek/volume, GStreamer adapter + codec policy | 11 | yes | `app-player.png` |
| P4-T15 | `apps/terminal` | VT100/ANSI parser (cursor, erase, SGR, scrollback, UTF-8 across reads), forkpty session (unix) | 13 | yes | `app-terminal.png` |
| P4-T16 | `apps/calendar` | Month grid (Monday-first, leap years, year rollover), event store (atomic JSON), clock/analog angles, stopwatch, timer | 14 | yes | `app-calendar.png`, `app-calendar-clock.png` |
| P4-T17 | `apps/archive` | Format detection, entry listing, extract/create with zip-slip and tar-slip guards, rebuild delete, permissions | 9 | yes | `app-archive.png` |
| P4-T18 | `apps/browser` | URL/search parsing, tab stacks, bookmarks, history de-duplication, downloads, private isolation, session restore | 38 | yes | `app-browser.png` |

## 2. Verification (performed in this session)

Commands and results:

```text
cargo test --workspace --exclude aether-compositor   # 66 test binaries, 513 cases, 0 failures
cargo test -p aether-ui -p aether-shell              # 5 + 55 unit, 1 interaction
cargo test -p aether-text -p aether-calculator -p aether-terminal
cargo test -p aether-files -p aether-archive -p aether-images
cargo test -p aether-calendar -p aether-documents -p aether-player
cargo test -p aether-browser
cargo run -p <crate> -- --render docs/2026/11/23/results/screenshots
```

The 66 binaries include 23 empty doc-test runs; P4 contributes 35 test binaries
and **235 cases**. The exclusion is only for the macOS dev host (Smithay links
`libxkbcommon`); CI installs it and runs the full workspace.

P4 totals: **224 unit/acceptance tests + 11 headless interaction tests (235
cases)**, all green. The `aether-compositor` crate is excluded on the macOS host
because Smithay links `libxkbcommon`, which only the Linux CI image provides.

CI status: the workflow runs `cargo test --workspace`, but the most recent runs
on `main` are red on `lifecycled`'s `unresponsive_client_times_out`
(`Socket(ConnectionReset)`), which passes locally and belongs to the P5 session.
No workflow run exists yet for the P4 commits, so P4's CI status is **pending an
owner-confirmed green run**, not claimed green here.

Note from the P4 T05 work: Focus now supports a daily schedule
(`FocusSchedule`, overnight windows included) in addition to the manual toggle,
so SHL-AC-05's "scheduled time" behaviour is implemented and unit-tested.

The interaction test is not a smoke test: it clicks real widgets through the
Slint testing backend and asserts model, UI and file effects —

| Check | Assertion |
| --- | --- |
| Tray Wi-Fi | state flips, `nmcli` mock called, `network.wifi=false` written to TOML |
| Dock | clicking AetherPad focuses/launches it and updates the menu-bar title |
| Launcher | "cal" ranks Calculator first; clicking it launches and closes the overlay |
| Control centre | Bluetooth tile toggles; "Performance" sets the power profile (mock called) |
| Settings | "Alert sounds" writes `sound.toml`; "Effects tier" cycles `full → reduced` |
| Notifications | dismiss removes one card; Focus hides all and clears the badge; nothing is deleted |
| Lock screen | wrong password shows the error and keeps the lock; correct password unlocks |
| Overview | clicking a window card activates it and closes the overlay |

### P4-T18a media decision record

- **Engine**: WebKitGTK 6.0 embedded through the platform surface; the chrome
  and data model are engine-agnostic (`apps/browser/src/engine.rs`).
- **Codecs**: the public image ships royalty-free codecs only (VP9, Opus, AV1).
  H.264/AAC are excluded from the shipped image per the P0-T03 legal note, so a
  page that requires them shows an explicit "codec unavailable" state (reduced
  quality path) rather than failing silently.
- **DRM/EME**: off by default; enabling it is a per-user choice (owner decision
  recorded in Q24: lighter browser, HD-or-reduced playback).
- **State today**: hardware-accelerated playback and streaming sites are
  **blocked on the graphics session** (GPU + compositor surfaces). The transport
  and policy above are what T18a will implement; no playback path is claimed as
  tested.

## 3. Evidence screenshots

All rendered headlessly with the software renderer (`aether-ui::render`):

- `screenshots/ui-gallery.png` — design system (tokens, controls, icons, rows)
- `screenshots/shell-desktop.png` — composed desktop (menu bar, windows, dock)
- `screenshots/shell-launcher.png` — launcher with calculator + settings results
- `screenshots/shell-control-center.png` — control centre
- `screenshots/shell-notifications.png` — notification centre
- `screenshots/shell-settings.png` — settings app with file/key mapping
- `screenshots/shell-lock.png` — lock screen
- `screenshots/shell-overview.png` — overview with workspaces
- `screenshots/shell-menubar.png` — menu bar strip (1440×96)
- `screenshots/shell-dock.png` — dock strip (720×140)
- `screenshots/app-browser.png`, `app-text.png`, `app-calculator.png`,
  `app-terminal.png`, `app-files.png`, `app-archive.png`, `app-images.png`,
  `app-calendar.png`, `app-calendar-clock.png`, `app-documents.png`,
  `app-documents-page2.png`, `app-player.png`

## 4. Acceptance status

| Task | Status | Note |
| --- | --- | --- |
| P4-T01 menu bar | Implemented, headless-verified | F1 on-device acceptance pending the layer-shell host (P3-T02) |
| P4-T02 dock | Implemented, headless-verified | Running indicators asserted on launch and close (the File menu's "Close window" clears the app and moves focus); auto-hide reveal needs pointer enter from the host |
| P4-T03 launcher | Implemented, headless-verified | Ranking + calculation + keyboard model tested |
| P4-T04 control centre | Implemented, headless-verified | Wi-Fi/BT/volume/brightness/Focus/power mode wired to adapters |
| P4-T05 notifications | Implemented, headless-verified | Grouping/Hide rules tested; Focus is a daily window that expires on the minute tick (manual Focus runs two hours), and the badge/cards refresh when it flips; real D-Bus notifications arrive with the session |
| P4-T06 lock screen | Implemented, headless-verified | Modal: scrim, Escape and every shortcut are ignored while locked (tested); only the password leaves it. greetd/PAM is the remaining on-device step |
| P4-T07 settings | Implemented + verified | Sidebar categories switch pages (callback-wired and clicked in the test); every row shows its file+key; writes are atomic and asserted from disk |
| P4-T08 files | Implemented, verified | Sandbox escape + symlink tests; Back/Forward, toolbar Open (double-click also wired), rename that refuses to overwrite; copy/move/trash on temp trees |
| P4-T10 AetherPad | Implemented, verified | **DA-AC-01 Pass**: UTF-16 LE/BE + Windows-1252 detection and round trips preserve characters and bytes. **DA-AC-02 Pass**: 10 MB count 77 ms, replace-all 67 ms, load/save 3 ms (release) |
| P4-T11 Calculator | Implemented, verified | **DA-AC-03 Pass**: 50-case acceptance set; precedence, `^` associativity, domain errors, history cap, memory |
| P4-T12 Images | Implemented, verified | All six DA-03 formats decode (PNG/JPEG/GIF/BMP/WebP/TIFF on generated fixtures), metadata, lossless rotate/flip, clamped zoom |
| P4-T13 Documents | Implemented, verified (PDF env-gated) | Markdown + pagination + search tested; poppler adapter reports NotAvailable when absent |
| P4-T14 Player | Implemented, verified (audio env-gated) | Playlist/shuffle/repeat/transport tested; GStreamer shell-out is the adapter, mock in tests |
| P4-T15 Terminal | Implemented, verified (host-gated PTY) | VT parser, forkpty session and copy/paste model hooks tested; Slint 1.19 exposes clipboard hooks on `Platform`, so the layer-shell platform will provide the real clipboard |
| P4-T16 Calendar/Clock | Implemented, verified | Leap year, month navigation, event CRUD + JSON round trip, hand angles, stopwatch/timer |
| P4-T17 Archive | Implemented, verified | Malicious tar/zip member rejection, extract/create/delete round trip |
| P4-T18 browser core | Implemented, verified | Tabs/bookmarks/history/downloads tested; a private tab never writes history, session, bookmarks or downloads to the profile; WebKitGTK embedding awaits graphics bring-up |
| P4-T18a browser media | Decision recorded | Royalty-free codecs only; H.264/AAC path documented, DRM off by default |
| P4-T18b browser data | Implemented, verified | History de-duplication, cap, downloads, session restore |

## 5. Gaps and next steps

1. **Layer-shell host (P3-T02, S2)** — mount `Menubar`/`Dock` as `zwlr_layer_shell`
   surfaces and the panels as overlays through the custom Slint platform from
   spike S1. No shell `.slint` change is needed; this is the on-device gate.
2. **Input and focus** — keyboard shortcuts (Super+Space launcher, overview,
   lock), pointer enter/leave for dock auto-hide, and compositor focus hand-off.
3. **D-Bus services** — real notification server, UPower polling loop (P5/S4),
   NetworkManager/BlueZ state instead of optimistic toggles.
4. **Engine embedding (T18/T18a)** — WebKitGTK surface once the GPU/compositor
   session exists; codec matrix per P0-T03.
5. **CI** — `cargo test --workspace` now includes the Slint tests. They are
   headless and need no display; install `fonts-dejavu-core` on the runner only
   if a future screenshot job renders text (the current tests do not compare
   pixels).

## 6. Review pass (2026-11-23, after independent verification)

A read-only review and an independent tester re-ran the suites and probed the
APIs. Blocking findings were fixed in the same session, each with a regression
test:

| Finding | Fix |
| --- | --- |
| Lock screen could be dismissed by the scrim, Escape or any shortcut | Lock is modal in the model: only a successful unlock changes the overlay; the scrim is skipped for overlay 6; three assertions in the interaction test |
| Settings sidebar categories did nothing (child wrote a parent-bound property) | `page-changed` callback wired to `open_settings_category`; test clicks Display and asserts the page and its `display.toml` rows |
| Focus never ended and the badge never refreshed | `FocusSchedule` is the single source of truth, manual Focus is a two-hour window, `tick` reports a flip and the shell resyncs; overnight windows tested |
| Files Back/Forward broken (navigation always recorded history) | `navigate(dir, record)` split from history; Back/Forward manage the stacks and a test asserts both directions |
| Files rename silently overwrote an existing file | Rename refuses with `AlreadyExists`; test proves the existing file is untouched |
| Files could not open a folder | Toolbar "Open" (and double-click) opens the selected directory; test navigates in and out |
| AetherPad save could truncate on a crash | Atomic temp-file + rename save |
| Player skipped the current track when an earlier one was removed | Cursor follows the play order; test keeps "Neon Rain" playing |
| Private-tab bookmarks/downloads leaked into the profile | Refused in a private tab; test reloads `profile.json` and finds it empty |
| Archive extraction traversed a symlink already in the destination | `prepare_target` walks each component without following links; tar + zip regression tests |
| Images missed WebP/TIFF | Both decoder features enabled and tested, so DA-AC-04 covers all six formats |

Non-blocking items also fixed:

- **F4 layering** (CUS-01): `SettingsStore` now reads the user layer, then
  `/etc/aether`, then `/usr/share/aether/defaults`; saves always go to the user
  layer, so a shipped default can never be modified (tested).
- Settings toggles update live state and services (dotted keys mapped to short
  ids); the password field is no longer cleared by unrelated syncs.
- Focus banner shows the real window (`09:00–17:00`) instead of a hardcoded
  "18:00"; the dock/notification labels come from the schedule.
- Files caches the `df` label per folder instead of spawning it on every
  selection change.
- Archive: selecting a folder extracts its children and deletes the whole
  subtree (tested), not only the folder entry.
- Services skip power supplies without a `type` file instead of aborting the
  battery scan; the terminal's `wait()` marks the session closed and `Drop`
  reaps without blocking forever; app launch goes through `sh -c` so quoted
  `.desktop` Exec arguments survive.
- Player demo paths are generic; a misnamed test was corrected.
