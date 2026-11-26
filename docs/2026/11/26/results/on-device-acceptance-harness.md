# On-device acceptance harness — F1 (Desktop Shell) and DA-* (Default Apps)

- Date: 2026-11-26
- Session: S3
- Scope: headless code is green; this document is the runbook for confirming
  the remaining on-device criteria against a running AetherOS image. It is
  preparation only — no hardware run is claimed here.

## 0. Prerequisites

```sh
# Boot the AetherOS image (BIOS or UEFI), log in as `aether`.
# Confirm the compositor and shell are running:
ps -e | grep -E 'aether-compositor|aether-shell'
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
# Compositor must print its layer-shell markers:
journalctl -u aether-compositor 2>/dev/null | grep -E 'LAYER_SURFACE|OUTPUT_COUNT'
```

Expected markers (from P3-T02): `AETHER_COMPOSITOR_OUTPUT_COUNT: 2`,
`AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-*`,
`AETHER_COMPOSITOR_LAYER_CONFIGURE: namespace=aether-shell-menubar size=...x28`.

## 1. Layer-shell binding check (Task 1)

```sh
# The shell owns the contract: the request plan the host sends, and the
# verification of the compositor's configure markers.
aether-shell --host-plan
journalctl -u aether-compositor --no-pager > /tmp/compositor.log
aether-shell --host-check /tmp/compositor.log     # exit 0 and AETHER_SHELL_LAYER_HOST_OK
# Equivalent one-liner without the journal:
journalctl -u aether-compositor --no-pager | aether-shell --host-check -
```

`--host-check` prints one `LAYER_SURFACE` line per surface
(`bound=`, `configured=WxH`, `expected=WxH`, `ok=`) and then
`AETHER_SHELL_LAYER_HOST_OK`, or `AETHER_SHELL_LAYER_HOST_PROBLEM: …` lines and
`AETHER_SHELL_LAYER_HOST_FAILED` with exit code 1. It fails when a surface never
bound, bound but was never configured, is on the wrong layer, or was configured
at a size that differs from the plan (size drift between the shell and
compositor).

Expected per-surface sizes on a 1920×1080 logical output: menubar 1920×28
(exclusive zone 28), dock 720×64, launcher 640×480, notifications 360×1080,
control centre 340×1080. On a HiDPI output the panel stretches to the **logical**
width (1920×28 for a 2880×1800 panel at 1.5×).

Headless evidence (no device needed, runs on the development host):

```sh
cargo test -p aether-shell --test layer_host      # 5 tests, incl. the binary
cargo run -p aether-shell -- --render /tmp/aether-shell-shots
ls /tmp/aether-shell-shots/shell-{menubar,dock,launcher,notifications}.png
```

Pass when `--host-check` exits 0 and the four PNGs are non-blank.

> Socket-level note: the Wayland client host that creates the surfaces needs
> `libwayland-client`; it runs on the image, not on the macOS development host.
> The compositor's own `p3_t02_headless` test covers the server half, and
> `--host-check` covers the client half at marker level
> (`docs/2026/11/26/results/p3-t02-layer-host-wiring.md`).

## 2. F1 (Desktop Shell) checklist

| Criterion | Command / manual step | Pass signal |
| --- | --- | --- |
| SHL-AC-01 Menu bar updates on focus change | Click between two app windows; watch the app name in the top bar | name changes within 100 ms |
| SHL-AC-02 Dock running indicator | Launch then quit an app | indicator disappears within 1 s |
| SHL-AC-03 Launcher by keyboard | Press Super+Space, type `ca`, press Enter | launcher opens < 200 ms, Calculator launches |
| SHL-AC-04 Control centre | Toggle Wi-Fi/BT, move volume/brightness sliders | service state follows (nmcli/bluetoothctl/wpctl/brightnessctl) |
| SHL-AC-05 Focus mode | Enable Focus in control centre; send a notification | notification hidden until the schedule ends |
| SHL-AC-06 Snap halves/quarters | Drag a window to a screen edge | snap preview and snap |
| SHL-AC-07 Glass tiers | Toggle "reduce transparency" in Settings | panels become solid without restart |
| SHL-AC-08 Lock screen | Press Super+L, enter wrong then right password | wrong rejected with counter, right unlocks |
| SHL-AC-09 Settings writes config | Change a toggle in every category | `~/.config/aether/*.toml` updated and re-read |
| SHL-AC-10 Two monitors / HiDPI | Connect a second monitor at a different scale | shell surfaces scale per output, no restart |
| SHL-AC-11 Screen reader | Run `orca`, focus menu bar items | items announced |

## 3. DA-* (Default Apps) checklist

| Criterion | Command / manual step | Pass signal |
| --- | --- | --- |
| DA-AC-01 Text opens UTF-16 / Windows-1252 | `aether-text /tmp/utf16.txt` and a CP-1252 file; save and diff | no character loss |
| DA-AC-02 Text find/replace on 10 MB | Open a 10 MB file, Find/Replace all | completes < 1 s, no UI freeze |
| DA-AC-03 Calculator 50 cases | `cargo test -p aether-calculator` on image; manual spot checks | 50/50 correct |
| DA-AC-04 Images formats + rotation | Open PNG/JPEG/WebP/GIF/BMP/TIFF; rotate 90° | all decode; rotation lossless |
| DA-AC-05 Documents 200-page PDF search | `aether-documents sample-200p.pdf`; search a word | opens and finds it (needs `pdftotext`/`pdftoppm` in image) |
| DA-AC-06 Player test files | `aether-player --status`; play FLAC/Vorbis/Opus/MP3/WAV; try H.264/AAC file | allowed play; restricted shows codec-bundle message |
| DA-AC-07 Terminal | `aether-terminal`; type, copy, paste, resize | shell runs, clipboard and resize work |
| DA-AC-08 Archive ZIP/TAR | Extract the supplied ZIP and TAR fixtures | permissions preserved; `../` rejected |
| DA-AC-09 Cold launch ≤ 500 ms | `hyperfine 'aether-<app>'` for each default app | ≤ 500 ms |
| DA-AC-10 Idle cost 0 MB / 0% CPU | Close all apps; `ps`/`pidstat` | no app process remains |
| DA-AC-11 Browser media + streaming | Open a local video and one streaming site | audio + video play |
| DA-AC-12 Browser cold launch ≤ 500 ms | `hyperfine 'aether-browser'` | ≤ 500 ms |

## 4. Features page (P7-T07) on device

```sh
# Headless proof that the page is a faithful projection of aether-config's
# FeatureManager (catalog, install/remove/enable, budget refusal, persistence):
cargo test -p aether-shell --lib features        # 9 tests
cargo test -p aether-shell --test features       # drives the real surface
cargo test -p aether-config --lib                # the manager itself

# On the device: install/remove a pack and confirm persistence + aetherctl
# agreement:
aether-shell # open Settings -> Features
# Click Install on "Advanced monitoring" (D), then:
cat ~/.config/aether/features/features.toml
aetherctl features list 2>/dev/null || true
```

Pass when the row flips to "Installed", the file contains `D`, and removing it
clears the entry. The page and `aetherctl` share
`~/.config/aether/features/features.toml`; a fresh `FeaturesPage::load` of that
file must show the same rows (covered by `a_fresh_page_sees_the_persisted_install_state`).

## 5. Evidence to record

For each passing criterion, append a line to
`docs/2026/11/16/os-project/features/01-desktop-shell.md` (SHL-AC-*) and
`docs/2026/11/17/os-addendum-1/F8-default-apps.md` (DA-AC-*) with the date
and the command output, and drop screenshots under
`docs/2026/11/26/results/screenshots/`.

Automated evidence already recorded (development host, 2026-11-26):

- Layer host: `cargo test -p aether-shell --test layer_host` (5 tests) and
  `aether-shell --host-check` against complete/broken/HiDPI logs —
  `docs/2026/11/26/results/p3-t02-layer-host-wiring.md`.
- Player transport: `cargo test -p aether-player` (19 lib, 1 `codec`, 1
  `transport_edges`, 1 `ui`) — pause/resume position, seek-while-paused,
  duration clamping, empty-URI rejection —
  `docs/2026/11/26/results/p4-t14-player.md`.
- Features page: `cargo test -p aether-shell --lib features` (9 tests) and
  `--test features` —
  `docs/2026/11/26/results/p7-t07-features-ui.md`.

## 6. Known blockers

- Audio (DA-AC-06) needs GStreamer in the image; restricted-codec bundle is
  optional (P0-T03).
- PDF (DA-AC-05) needs `pdftotext`/`pdftoppm` in the image.
- Browser media (DA-AC-11/12) needs WebKitGTK and the graphics session (P3).
- Cold-launch/idle timings (DA-AC-09/10/12) need the real device.
- The interactive layer-shell host (§1) needs `libwayland-client` on the image;
  the development host can neither link nor run the compositor.
