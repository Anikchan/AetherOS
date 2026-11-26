# On-device acceptance harness — F1 (Desktop Shell), DA-* (Default Apps), media

- Date: 2026-11-26 (rev 2)
- Session: S3
- Scope: the headless suites are green; this document is the executable runbook
  for confirming the remaining criteria against a running AetherOS image. It
  lists **one command block and one expected output per acceptance row**, so a
  run either produces the quoted markers or it fails. No hardware run is claimed
  here.

Conventions used below:

- `# do:` — the action to perform (keyboard/pointer).
- `# verify:` — the command that produces evidence and the output to look for.
- `EXPECT` — the exact line(s) or pattern the run must produce.
- Anything in `<angle brackets>` is environment-specific and recorded verbatim
  in the evidence log.

## 0. Prerequisites and evidence capture

```sh
# Boot the AetherOS image (BIOS or UEFI), log in as `aether`, open a terminal.
export WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}

mkdir -p "$HOME/aether-shots"
LOG="$HOME/aether-acceptance-$(date +%Y%m%d).log"
exec > >(tee -a "$LOG") 2>&1
say() { printf '\n=== %s ===\n' "$*"; }
shot() { grim "$HOME/aether-shots/$1.png" && echo "SCREENSHOT $1"; }
```

Preflight (all three must pass before any row is scored):

```sh
say preflight
pgrep -a aether-compositor
pgrep -a aether-shell
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
aether-shell --status | tail -n 12
journalctl -b -u aether-compositor --no-pager | grep -E 'LAYER_SURFACE|OUTPUT_COUNT'
```

EXPECT:

```
<pid> aether-compositor
<pid> aether-shell
WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/run/user/1000
active-app: <name>
clock: <HH:MM>
battery: <percent or "AC">
power-mode: balanced
wifi: <ssid|off>
bluetooth: <state>
focus: false
notifications: <n>
dock: <n>
windows: <n>
settings-root: /home/aether/.config/aether
AETHER_SHELL_STATUS_OK
AETHER_COMPOSITOR_OUTPUT_COUNT: <n>
AETHER_COMPOSITOR_LAYER_SURFACE: namespace=aether-shell-<surface>
```

If `aether-shell --status` does not end with `AETHER_SHELL_STATUS_OK`, stop: the
shell cannot be accepted.

## 1. Layer-shell binding check (task 1)

```sh
say layer-host
aether-shell --host-plan | tail -n 2
journalctl -b -u aether-compositor --no-pager > /tmp/compositor.log
aether-shell --host-check /tmp/compositor.log; echo "host-check exit=$?"
```

EXPECT:

```
LAYER_REQUEST name=menubar namespace=aether-shell-menubar layer=top anchors=top,left,right size=1920x28 exclusive_zone=28 keyboard=none
...
AETHER_SHELL_LAYER_PLAN_OK
LAYER_SURFACE name=menubar bound=yes configured=1920x28 expected=1920x28 ok=yes
...
AETHER_SHELL_LAYER_HOST_OK
host-check exit=0
```

Failure text is `AETHER_SHELL_LAYER_HOST_PROBLEM: <surface> <reason>` followed by
`AETHER_SHELL_LAYER_HOST_FAILED` and `exit=1` — record the reason verbatim.
Sizes on a 1920×1080 logical output: menubar 1920×28 (exclusive zone 28), dock
720×64, launcher 640×480, notifications 360×1080, control centre 340×1080. On a
HiDPI output the panel stretches to the **logical** width (1920×28 for a
2880×1800 panel at 1.5×).

Headless evidence (development host, no device):

```sh
cargo test -p aether-shell --test layer_host      # 5 tests
cargo run -p aether-shell -- --render /tmp/aether-shots
ls /tmp/aether-shots/shell-{menubar,dock,launcher,notifications}.png
```

EXPECT `test result: ok. 5 passed` and four non-blank PNGs.

> Socket-level note: the Wayland client host that creates the surfaces needs
> `libwayland-client`; it runs on the image, not on the macOS development host.
> The compositor's `p3_t02_headless` test covers the server half, `--host-check`
> covers the client half at marker level
> (`docs/2026/11/26/results/p3-t02-layer-host-wiring.md`).

## 2. F1 (Desktop Shell) checklist

### SHL-AC-01 — Menu bar updates on focus change

```sh
# do: open Calculator and Text, then click between the two windows.
# verify:
aether-shell --status | grep '^active-app:'
shot shl-ac-01-menubar
```

EXPECT `active-app:` changes to the focused window's name within 100 ms of each
click (`aether-calculator` → `aether-text`), and the screenshot shows the new
name in the top bar. Pass when both names were observed.

### SHL-AC-02 — Dock running indicator

```sh
# do: launch `aether-calculator` from the launcher, wait for the window, quit it.
# verify:
aether-shell --status | grep -E '^(dock|windows):'
pgrep -a aether-calculator || echo "calculator exited"
```

EXPECT the dock entry for Calculator gains its running dot while the process is
alive and loses it within 1 s of exit; `calculator exited` after quitting.
Record both the `dock:` count with and without the app.

### SHL-AC-03 — Launcher by keyboard

```sh
# do: press Super+Space, type `ca`, press Enter; stopwatch the open with:
date +%s%3N   # before the keystroke
date +%s%3N   # after the launcher is visible
pgrep -a aether-calculator
```

EXPECT the launcher is visible in `< 200 ms` (delta of the two timestamps),
Enter launches Calculator (process present), and the top result is Calculator.

### SHL-AC-04 — Control centre

```sh
# do: open the control centre, toggle Wi-Fi and Bluetooth, move volume and brightness.
# verify:
nmcli radio wifi
bluetoothctl show | grep -E 'Powered|Name'
wpctl get-volume @DEFAULT_AUDIO_SINK@
brightnessctl -m | cut -d, -f4
```

EXPECT `enabled`/`disabled` follows the Wi-Fi toggle, `Powered: yes|no` follows
the Bluetooth toggle, `Volume: 0.xx` follows the slider, and the brightness
percentage follows the slider. Each control must change its service within 1 s.

### SHL-AC-05 — Focus mode

```sh
# do: enable Focus in the control centre, then send a notification:
notify-send "Focus test" "must stay hidden"
# verify:
aether-shell --status | grep -E '^(focus|notifications):'
```

EXPECT `focus: true` and `notifications:` unchanged while Focus is on; disabling
Focus makes the notification appear (count +1). Record the schedule used.

### SHL-AC-06 — Snap halves/quarters

```sh
# do: drag a window to the left edge, then the top-left corner.
# verify:
shot shl-ac-06-snap-half
```

EXPECT a snap preview appears while dragging and the window occupies exactly
half (960×1080 at 1920×1080) then a quarter (960×540). Both screenshots must
show the preview/border.

### SHL-AC-07 — Glass tiers

```sh
# do: Settings → Appearance → "Reduce transparency" on, then off.
# verify:
grep -R "reduce_transparency\|reduce-transparency" -n "$HOME/.config/aether" || true
shot shl-ac-07-solid
shot shl-ac-07-glass
```

EXPECT the config file shows the new value, the panels become opaque without a
shell restart, and the two screenshots differ in panel opacity.

### SHL-AC-08 — Lock screen

```sh
# do: press Super+L, type a wrong password, Enter, then the right password.
# verify:
journalctl -b -u aether-greeter --no-pager | tail -n 20
pgrep -a aether-compositor >/dev/null && echo "session alive"
```

EXPECT the wrong attempt is rejected with an attempt counter in the greeter log
(`authentication failure`/attempt count) and the right one unlocks back into the
same session (`session alive`). Three wrong attempts must not unlock.

### SHL-AC-09 — Settings writes config

```sh
# do: change one toggle in every Settings category.
# verify:
find "$HOME/.config/aether" -name '*.toml' -newer /tmp/shl-ac-09.stamp -print
touch /tmp/shl-ac-09.stamp
```

Create the stamp **before** changing the toggles:

```sh
touch /tmp/shl-ac-09.stamp
```

EXPECT one updated `.toml` per category (theme, keyboard, sound, network,
power, features) and the value is re-read by the UI (toggle stays set after
reopening Settings).

### SHL-AC-10 — Two monitors / HiDPI

```sh
# do: connect a second monitor at a different scale.
# verify:
journalctl -b -u aether-compositor --no-pager | grep OUTPUT_COUNT | tail -n 2
aether-shell --host-check /tmp/compositor.log
```

EXPECT `AETHER_COMPOSITOR_OUTPUT_COUNT: 2`, one configure per output, shell
surfaces re-sized to each output's logical width, no restart, and `--host-check`
still exits 0 after re-capturing the log.

### SHL-AC-11 — Screen reader

```sh
# do: start `orca`, focus the menu bar and tab through the items.
# verify:
pgrep -a orca
orca --replace --debug > /tmp/orca.log 2>&1 &
sleep 2; grep -i "speech\|utterance" /tmp/orca.log | head
```

EXPECT Orca runs and announces each focused menu-bar item (utterance log lines).
Record the announced strings; a silent focus change is a failure.

## 3. DA-* (Default Apps) checklist

### DA-AC-01 — Text opens UTF-16 / Windows-1252

```sh
printf '\xff\xfe' > /tmp/utf16.txt
printf 'Caf\xe9 na\xefve\n' | iconv -f UTF-8 -t UTF-16LE >> /tmp/utf16.txt
printf 'caf\xe9 \x80\n' > /tmp/cp1252.txt     # "café €" in Windows-1252
# do: open both in aether-text, add a character, save, close.
# verify:
iconv -f UTF-16LE -t UTF-8 /tmp/utf16.txt | tail -n 1
iconv -f WINDOWS-1252 -t UTF-8 /tmp/cp1252.txt | tail -n 1
```

EXPECT `Café naïve` (plus the edit) and `café €` (plus the edit) — no `?`
replacement characters and the files keep UTF-16LE / Windows-1252 encoding.

### DA-AC-02 — Text find/replace on 10 MB

```sh
python3 - <<'PY'
line = "lorem ipsum dolor sit amet 0123456789 aether\n"
open("/tmp/ten-mb.txt", "w").write(line * ((10 * 1024 * 1024) // len(line) + 1))
PY
ls -l /tmp/ten-mb.txt
# do: open it, Find "ipsum" (expect the count), Replace all with "IPSUM".
```

EXPECT the count equals `wc -l /tmp/ten-mb.txt` (233017 for the 45-byte line
used above), the replace-all completes in under 1 s with no "not responding"
state, and saving keeps the file ~10 MB.
Headless regression: `cargo test -p aether-text --lib` (20 tests, includes the
10 MB load/save/find budget).

### DA-AC-03 — Calculator 50 cases

```sh
cargo test -p aether-calculator --test acceptance   # on the image, if cargo is present
aether-calculator --render /tmp/aether-shots        # otherwise: binary smoke test
# manual spot checks: 2^10, (2+3)*4, 0.1+0.2, 100/8, sqrt(81)
```

EXPECT `test result: ok. 2 passed` (the 50-case set plus the error cases), or
five correct manual results (`1024`, `20`, `0.3`, `12.5`, `9`).

### DA-AC-04 — Images formats + rotation

```sh
# do: open PNG/JPEG/WebP/GIF/BMP/TIFF fixtures from ~/Pictures/aether-fixtures,
#     then rotate 90° once in each; save a copy of the PNG.
# verify:
ls -l ~/Pictures/aether-fixtures
aether-images --render /tmp/aether-shots
file /tmp/aether-shots/app-images.png
```

EXPECT all six formats decode (no error banner), rotation swaps width/height
for 90°, and the PNG copy's pixels are unchanged apart from the rotation
(rotation is lossless, covered by `cargo test -p aether-images --lib`, 12 tests).

### DA-AC-05 — Documents 200-page PDF search

```sh
which pdftotext pdftoppm pdfinfo
pdfinfo "$HOME/Documents/sample-200p.pdf" | grep -E '^(Title|Pages):'
pdftotext -layout "$HOME/Documents/sample-200p.pdf" - | grep -c '^'   # text extraction works
aether-documents --info "$HOME/Documents/sample-200p.pdf" 2>&1 | head -n 3  # text/markdown path only
# do: open the PDF in aether-documents and search a word known to be on page 150.
```

EXPECT `pdfinfo` prints `Pages: 200` (and a title), `pdftotext` emits lines,
`--info` on a *PDF* reports `cannot open ...: stream did not contain valid
UTF-8` (the CLI is the text path — the window uses the `pdf.rs` adapter), and
the in-app search jumps to the matching page in under 2 s. Headless regression:
`cargo test -p aether-documents --lib` (the poppler adapter tests cover page
count, text extraction and page rendering).

### DA-AC-06 — Player test files

```sh
aether-player --status
# do: play one FLAC, Vorbis, Opus, MP3 and WAV; then the H.264/AAC sample.
```

EXPECT `--status` prints `backend: gstreamer (...)` (GStreamer present) and
`restricted-codecs: H.264, H.265/HEVC, AAC, MPEG-4 AAC (optional bundle, P0-T03)`.
The five royalty-free files play. The H.264/AAC sample shows
`H.264, AAC playback needs the optional codec bundle (P0-T03)` unless the
bundle is installed, in which case it plays (`codec-bundle=present`, below).

### DA-AC-07 — Terminal

```sh
# do: open aether-terminal, run `ls`, copy the output, paste it, resize the window.
```

EXPECT the shell prompt accepts input, `ls` output renders, copy/paste works
both ways, and resizing reflows without losing scrollback.

### DA-AC-08 — Archive ZIP/TAR

```sh
# do: extract the supplied fixtures into a scratch directory, then create a tar.gz.
mkdir -p /tmp/da08 && cd /tmp/da08
# via the UI: extract ~/Documents/aether-fixtures/sample.zip and sample.tar.gz
# verify:
find /tmp/da08 -maxdepth 2 -printf '%M %p\n' | sort | head
# malicious fixture: the "../escape" member must be refused.
```

EXPECT file modes are preserved (`-rwxr-xr-x` for the executable fixture),
`../escape` is rejected with `refused unsafe archive entry: ../escape`, and
nothing is written outside `/tmp/da08`.

### DA-AC-09 — Cold launch ≤ 500 ms

```sh
which hyperfine || echo "install hyperfine (or use the loop below)"
for app in aether-text aether-calculator aether-files aether-images \
           aether-documents aether-player aether-terminal aether-calendar \
           aether-archive aether-browser; do
  /usr/bin/time -f "$app %e s" sh -c "$app --help >/dev/null"
done
```

EXPECT every line `≤ 0.500 s` (with `hyperfine --warmup 2 'aether-<app> --help'`
in place of `/usr/bin/time`). Record any app above the bound.

### DA-AC-10 — Idle cost 0 MB / 0% CPU

```sh
# do: close every app window, wait 60 s.
pgrep -a 'aether-(text|calculator|files|images|documents|player|terminal|calendar|archive|browser)' \
  || echo "no app process remains"
pidstat -u -p ALL 1 3 | grep aether | grep -v compositor
```

EXPECT `no app process remains` and no `aether-*` app line above `0.00 %CPU`.
The compositor and shell may remain (they are the session, not an app).

### DA-AC-11 — Browser media + streaming

```sh
aether-browser --media-status
aether-browser --media-check "$HOME/Videos/aether-sample.webm"; echo "exit=$?"
aether-browser --media-check "$HOME/Videos/aether-sample.mp4";  echo "exit=$?"
aether-browser --media-check https://<stream-host>/live/index.m3u8; echo "exit=$?"
# do: open the local video and the streaming page in the browser; play both.
aether-browser --render /tmp/aether-shots
```

EXPECT:

```
AETHER_BROWSER_MEDIA_OK
acceleration=hardware backend=vaapi          # or: acceleration=software backend=none
codec-bundle=absent                          # or: present
drm=disabled
restricted=H.264,H.265/HEVC,AAC,MPEG-4 AAC
video-base=VP8,VP9,AV1,Theora
audio-base=Opus,Vorbis,FLAC,MP3,WAV,PCM
streaming=hls,dash

AETHER_BROWSER_MEDIA_CHECK kind=progressive-video playable=true reduced=false
message=playing VP9 1080p — hardware decode
exit=0

AETHER_BROWSER_MEDIA_CHECK kind=progressive-video playable=false reduced=false
message=H.264, AAC playback needs the optional codec bundle (P0-T03)
exit=3

AETHER_BROWSER_MEDIA_CHECK kind=hls playable=true reduced=true
message=playing VP9 720p — hardware decode; reduced quality (the codec bundle adds the HD rung)
exit=0
```

Then the real playback check: audio + video play in the local file, and the
streaming site plays at the HD rung when `codec-bundle=present`, otherwise at
the reduced rung the status bar names. Record the status-bar line verbatim
(`media: <kind> · <plan message>`). `drm=enabled` is only expected after the
owner turns protected content on in Settings → Browser.

### DA-AC-12 — Browser cold launch ≤ 500 ms

```sh
hyperfine --warmup 2 'aether-browser --media-status >/dev/null'
/usr/bin/time -f "browser %e s" sh -c 'aether-browser --help >/dev/null'
```

EXPECT `≤ 0.500 s`, and the browser binary starts without a display (the
`--media-status` path does not open a window).

## 4. Calendar & Clock v1 (P4-T16, DA-07)

```sh
say calendar-clock
aether-calendar --events | head
aether-calendar --alarms
aether-calendar --render /tmp/aether-shots
ls /tmp/aether-shots/app-calendar{,-week,-clock}.png
# do: add an event, add an alarm at 07:30, pick New Delhi, start the timer.
cat "$HOME/.config/aether/calendar/events.json"
cat "$HOME/.config/aether/calendar/alarms.json"
```

EXPECT `--events` prints the stored JSON (`[]` on a fresh profile), `--alarms`
the alarm array (`[]` before the first alarm), three PNGs are written, and after
the UI steps the two files contain the new event and:
`{"id":"alarm-1","label":"Wake up","hour":7,"minute":30,"enabled":true,"days":[]}`.
When the clock reaches 07:30 the banner reads `Alarm: Wake up · 07:30` and
`Dismiss` clears it. The zone panel shows `New Delhi · UTC+05:30` with the
offset clock above it.

## 5. Features page (P7-T07) on device

```sh
cargo test -p aether-shell --lib features        # 9 tests
cargo test -p aether-shell --test features       # drives the real surface
cargo test -p aether-config --lib                # the manager itself

# On the device: install/remove a pack and confirm persistence + aetherctl
# agreement:
aether-shell   # Settings -> Features -> Install "Advanced monitoring" (D)
cat "$HOME/.config/aether/features/features.toml"
aetherctl features list 2>/dev/null || true
```

EXPECT the row flips to "Installed", the file contains `D`, removing it clears
the entry, and a fresh `FeaturesPage::load` shows the same rows (covered by
`a_fresh_page_sees_the_persisted_install_state`).

## 6. Recording template

For every row, append one line to
`docs/2026/11/16/os-project/features/01-desktop-shell.md` (SHL-AC-*) or
`docs/2026/11/17/os-addendum-1/F8-default-apps.md` (DA-AC-*):

```
2026-11-XX | SHL-AC-0N | pass | <command> | <expected line observed> | <screenshot>
```

Screenshots go under `docs/2026/11/26/results/screenshots/` with the row id in
the name (`shl-ac-01-menubar.png`).

## 7. Automated evidence already recorded (development host, 2026-11-26)

- Layer host: `cargo test -p aether-shell --test layer_host` (5 tests) and
  `aether-shell --host-check` against complete/broken/HiDPI logs —
  `docs/2026/11/26/results/p3-t02-layer-host-wiring.md`.
- Player transport: `cargo test -p aether-player` (19 lib, 1 `codec`, 1
  `transport_edges`, 1 `ui`) —
  `docs/2026/11/26/results/p4-t14-player.md`.
- Shared media policy: `cargo test -p aether-media` (10 tests) — the same
  H.264/AAC gate the Player and Browser consume —
  `docs/2026/11/26/results/p4-t16-t18a-media-calendar.md`.
- Browser media: `cargo test -p aether-browser` (46 lib incl. 6 `media` tests,
  1 headless UI test that asserts the media status line) and
  `aether-browser --media-status` / `--media-check` —
  `docs/2026/11/26/results/p4-t16-t18a-media-calendar.md`.
- Calendar & Clock: `cargo test -p aether-calendar` (21 lib incl. week grid,
  time zones, alarms; 1 headless UI test) and the three `--render` surfaces —
  `docs/2026/11/26/results/p4-t16-t18a-media-calendar.md`.
- Default app unit suites: text 20, calculator 11 + 2 acceptance, images 12,
  archive 15, each with a headless UI test.
- Features page: `cargo test -p aether-shell --lib features` (9) and
  `--test features` — `docs/2026/11/26/results/p7-t07-features-ui.md`.

## 8. Known blockers

- Audio (DA-AC-06) needs GStreamer in the image; the restricted-codec bundle is
  optional (P0-T03). DA-AC-06 also depends on P2-T04 PipeWire.
- PDF (DA-AC-05) needs `pdftotext`/`pdftoppm` in the image.
- Browser media (DA-AC-11/12) needs WebKitGTK and the graphics session (P3);
  the policy and status lines are already testable without either.
- Cold-launch and idle timings (DA-AC-09/10/12) need the real device.
- The interactive layer-shell host (§1) needs `libwayland-client` on the image;
  the development host can neither link nor run the compositor.
- Screen reader (SHL-AC-11) needs `orca` plus the AT-SPI bus in the image.
