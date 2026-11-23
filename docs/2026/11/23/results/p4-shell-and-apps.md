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

See the app table in `apps/README.md`. Each app has a tested core module and a
window driven from Rust (no logic in `.slint`).

## 2. Verification (performed in this session)

Commands and results:

```text
cargo test -p aether-ui -p aether-shell     # 45 + 5 unit tests, 1 interaction test — PASS
cargo test --workspace                      # see section 3
cargo run -p aether-shell -- --render docs/2026/11/23/results/screenshots
cargo run -p aether-browser -- --render docs/2026/11/23/results/screenshots
```

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
- `screenshots/app-*.png` — one per default app

## 4. Acceptance status

| Task | Status | Note |
| --- | --- | --- |
| P4-T01 menu bar | Implemented, headless-verified | F1 on-device acceptance pending the layer-shell host (P3-T02) |
| P4-T02 dock | Implemented, headless-verified | Running indicators asserted; auto-hide reveal needs pointer enter from the host |
| P4-T03 launcher | Implemented, headless-verified | Ranking + calculation + keyboard model tested |
| P4-T04 control centre | Implemented, headless-verified | Wi-Fi/BT/volume/brightness/Focus/power mode wired to adapters |
| P4-T05 notifications | Implemented, headless-verified | Grouping/Focus rules tested; real D-Bus notifications arrive with the session |
| P4-T06 lock screen | Implemented, headless-verified | greetd/PAM integration is the remaining on-device step |
| P4-T07 settings | Implemented + verified | "Every setting maps to a file key" shown in the UI and asserted by tests |
| P4-T08 files | See app table | Core FS operations tested against temp trees |
| P4-T10..T17 apps | See app table | Each app's core logic tested; UI rendered |
| P4-T18 browser core | Implemented, verified | Tabs/bookmarks/history/downloads/private/session tested; WebKitGTK embedding awaits graphics bring-up |
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
