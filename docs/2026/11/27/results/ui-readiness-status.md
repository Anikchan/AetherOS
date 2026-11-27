# UI readiness status — what exists vs. what the image boots

- Date: 2026-11-27
- Session: S1 (`session - 1 (Nov27)`) — status answer for the owner
- Question: "Do we have some UI ready at least, or is it only CLI yet?"

## Short answer

The UI is **built and running** as software: a Slint desktop shell (menu bar,
dock, launcher, control centre, notifications, overview, settings, lock,
features) plus eleven Slint applications. It renders today with
`aether-shell --render <dir>` (10 surfaces, 1440×900) and
`aether-taskmanager --render <dir>` (9 Activity Manager tabs).

The **bootable image does not contain any of it yet**. `os-build/` is a plain
Buildroot defconfig with a rootfs overlay whose only added file is
`overlay/root/test.sh`; there is no Buildroot package for the Rust workspace,
so `aether-compositor`, `aether-shell`, `aether-greeter` and the apps never
reach `/usr/bin` on the device. Booting the current image gives a console
login on tty1 plus SSH — no graphical session.

So: **CLI on the device, full UI in the repository.**

## Evidence collected in this session

| Check | Command | Result |
| --- | --- | --- |
| Shell surfaces render | `cargo run -p aether-shell --bin aether-shell -- --render /tmp/aether-shell-render` | 10 PNGs, `AETHER_SHELL_RENDER_DONE` |
| Accessibility evidence | `--render-a11y` (S3, `661dab4`) | 4 PNGs (focus ring, 150 %/200 % text, Settings 200 %) |
| Activity Manager tabs | `cargo test -p aether-taskmanager` + `--render` (S4, `4833fd8`) | 9 tabs, headers pinned by `tests/tables.rs` |
| Shell + UI tests | `cargo test -p aether-shell -p aether-ui` | 163 + 5 passed |
| Image contents | `grep -rn aether- os-build/buildroot/aetheros_defconfig`, `ls os-build/board/aetheros/overlay/root` | only `test.sh`; no binary package |

Rendered surfaces copied to `docs/2026/11/27/results/screenshots/`
(`shell-desktop`, `-launcher`, `-control-center`, `-notifications`,
`-overview`, `-settings`, `-lock`, `-features`, `-menubar`, `-dock`) next to the
existing `shell-a11y-*` and `activity-manager-*` frames.

## Applications with a Slint UI in the workspace

`archive`, `browser`, `calculator`, `calendar`, `documents`, `files`, `images`,
`player`, `taskmanager`, `terminal`, `text` — each with `ui/app.slint` and a
headless render path; `store`/`pkg`/`runner`/`sandbox` are services without a
window.

## What is missing for "UI on the screen"

1. **Buildroot package for the workspace binaries** (the real gap). Needs the
   Rust host toolchain in Buildroot (`BR2_PACKAGE_HOST_RUSTC`), a
   `cargo-package`-style recipe that builds
   `aether-compositor`, `aether-shell`, `aether-greeter` and the apps for
   `x86_64-unknown-linux-musl`, plus the runtime libraries already in the
   defconfig (mesa/gbm/EGL, libdrm, libinput, xkbcommon, wayland,
   fontconfig/freetype for Slint).
2. **Session wiring**: a `aether-session` unit that starts the compositor on
   tty1, with the console getty moved to tty2 so a failed graphical start still
   leaves a login and `test.sh` reachable over SSH.
3. **Compositor ↔ shell handshake** on the device (the layer-shell host
   contract is implemented and tested headless; it has never been exercised
   against the real DRM backend).

## Honest hardware status

`P2` graphics, audio, sleep, battery, temperature and webcam were validated on
the Dell with the earlier image; the graphical *stack* was exercised through
the KMS checklist, not through the Slint shell, because the shell is not in the
image.

## Next step

Package the workspace into the image (item 1 + 2 above), rebuild, and boot it.
That is the task that turns "UI in the repository" into "UI on the screen".
