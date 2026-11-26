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
# Shell surfaces must appear as distinct layer surfaces, not one window:
WAYLAND_DEBUG=1 aether-shell --status
# On the device, the compositor log must contain, for each surface:
for ns in aether-shell-menubar aether-shell-dock aether-shell-launcher \
          aether-shell-notifications aether-shell-control-center; do
  journalctl -u aether-compositor | grep "namespace=$ns" || echo "MISSING $ns"
done
# Rendered (headless) evidence already produced by:
cargo run -p aether-shell --bin aether-shell -- --render /tmp/aether-shell-shots
ls /tmp/aether-shell-shots/shell-{menubar,dock,launcher,notifications}.png
```

Pass when all five namespaces are configured and the four PNGs are non-blank.

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
# Install/remove a pack and confirm persistence + aetherctl agreement:
aether-shell # open Settings -> Features
# Click Install on "Advanced monitoring" (D), then:
cat ~/.config/aether/features/features.toml
aetherctl features list 2>/dev/null || true
```

Pass when the row flips to "Installed", the file contains `D`, and removing it
clears the entry.

## 5. Evidence to record

For each passing criterion, append a line to
`docs/2026/11/16/os-project/features/01-desktop-shell.md` (SHL-AC-*) and
`docs/2026/11/17/os-addendum-1/F8-default-apps.md` (DA-AC-*) with the date
and the command output, and drop screenshots under
`docs/2026/11/26/results/screenshots/`.

## 6. Known blockers

- Audio (DA-AC-06) needs GStreamer in the image; restricted-codec bundle is
  optional (P0-T03).
- PDF (DA-AC-05) needs `pdftotext`/`pdftoppm` in the image.
- Browser media (DA-AC-11/12) needs WebKitGTK and the graphics session (P3).
- Cold-launch/idle timings (DA-AC-09/10/12) need the real device.
