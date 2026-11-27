# On-device acceptance harness — F1 (Desktop Shell), DA-* (Default Apps), media

- Date: 2026-11-26 (rev 3, 2026-11-27 additions: effect tier + frame/idle,
  browser security)
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
pgrep -af 'aether-shell --interactive'    # the running shell is the journal host
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
aether-shell --status | tail -n 18
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
effects-tier: full
effects-effective: full
effects-env: AETHER_COMPOSITOR_EFFECT_TIER=full
safe-mode: false
start-attempts: 0
nohang: idle
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
Sizes on a 1920×1080 logical output (2026-11-28, nine surfaces): background
1920×1080 (Background layer, all four anchors, exclusive zone −1, keyboard
none), menubar 1920×28 (exclusive zone 28), dock 720×64, launcher 640×420,
notifications 360×1052, control centre 340×1052 (both topped by the 28 px menu
bar), and the three **lazy** `Overlay` surfaces `settings`, `overview` and
`lock` at 1920×1080 (created on first open, unmapped on close; a fresh session
reports `planned=9 created=6 mapped=3`, an opened Settings `created=7 mapped=4`).
On a HiDPI output the panel stretches to the **logical** width (1920×28 for a
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

## 1b. Effect tier and frame/idle checks (P3-T05/T06 ↔ P4-T04)

The shell selects the glass tier, persists it, and publishes the compositor's
own environment contract; the compositor reports what it actually runs.

```sh
say effects
aether-shell --effects-tier
eval "$(aether-shell --effects-env)"; echo "EFFECTS_ENV=$AETHER_COMPOSITOR_EFFECT_TIER"
journalctl -b -u aether-compositor --no-pager > /tmp/compositor.log
aether-shell --effects-check /tmp/compositor.log; echo "effects-check exit=$?"

# do: control centre → Effects → Reduced, then Minimal, then Full.
grep -E 'effects-tier|reduce-transparency' "$HOME/.config/aether/theme/tokens.toml"
# Repeat the check after a compositor restart (the tier is read at startup):
systemctl --user restart aether-compositor
sleep 3; journalctl -b -u aether-compositor --no-pager > /tmp/compositor.log
aether-shell --effects-check /tmp/compositor.log; echo "effects-check exit=$?"
```

EXPECT (fresh profile → Full):

```
full
EFFECTS_ENV=full
EFFECT_TIER expected=full compositor=full switches=0 last_switch=none
EFFECT_FRAMES frames=<n> idle_skips=<n> idle_pct=<pct> callbacks=<n> markers=<n> est_ms=<ms> budget_ms=16.67
AETHER_SHELL_EFFECTS_OK
effects-check exit=0
effects-tier = "reduced"          # after the control-centre clicks
```

Before the restart the check may report a mismatch (the image's compositor was
started with the previous value); record it verbatim — that is the documented
behaviour, not a failure of the switch:

```
AETHER_SHELL_EFFECTS_PROBLEM: compositor runs tier full, the setting resolves to reduced — restart the compositor to apply it
AETHER_SHELL_EFFECTS_FAILED
```

After the restart with `AETHER_COMPOSITOR_EFFECT_TIER=reduced` exported by the
session launcher, `EFFECT_TIER expected=reduced compositor=reduced` and exit 0.
Record from the same log:

| Measurement | Source marker | Pass condition |
| --- | --- | --- |
| Effect tier | `AETHER_COMPOSITOR_EFFECT_TIER` / `EFFECT_SUMMARY` | equals `aether-shell --effects-tier` |
| Frames presented | `AETHER_COMPOSITOR_FRAME_STATS frames=` | `> 0` with the shell surfaces mapped |
| Idle ratio | `..._FRAME_STATS idle_pct=` / `..._IDLE` | present; near-idle desktop ≥ 90% |
| Frame callbacks | `..._FRAME_CALLBACKS n=` | `> 0` for every presented frame |
| Modelled frame cost | `..._FRAME est_ms=` and `..._GLASS_PLAN` | `≤ 16.67 ms` at Full with three windows (the 60 fps gate; real GPU timing still needs the Dell) |
| Pacing | `..._FRAME_STATS avg_gap_ms=` | `≈ 16.0–16.7 ms` with motion on screen |

`reduce-transparency = true` in `theme/tokens.toml` must make `--effects-tier`
print `minimal` even when `effects-tier = "full"` (D1 §9 accessibility override).

Headless evidence (development host, no device):

```sh
cargo test -p aether-shell --lib effects        # 8 unit tests
cargo test -p aether-shell --test effects       # 6 command tests
cargo test -p aether-shell --test effects_ui    # control-centre switch
```

EXPECT `test result: ok. 8 passed`, `6 passed`, `1 passed`.

## 1c. G-NH2 no-hang: "Not responding", shell watchdog, safe mode (P3-T08 ↔ shell)

The compositor detects, the shell decides. The whole contract is the
compositor's journal; the shell has no new protocol:

| Compositor marker | Shell reaction |
| --- | --- |
| `AETHER_COMPOSITOR_APP_STATE: client=… state=not_responding after_ms=…` | show "Not responding" with **Wait** / **Force Quit** |
| `AETHER_COMPOSITOR_APP_STATE: client=… state=responsive recovered=true` | close the dialog (the app answered a late ping) |
| `AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=… timeouts=…` | the shell stopped committing; the service manager restarts it |

Shell markers (`shell/src/nohang.rs`): `AETHER_SHELL_NOHANG_DIALOG`,
`…_ACTION action=wait|force_quit`, `…_RECOVERED`, `…_TERMINATE`,
`…_SUMMARY`, `AETHER_SHELL_START`, `…_START_CONFIRMED`, `…_SAFE_MODE`,
`…_SAFE_MODE_RESET`.

### 1c-1 — Hung app shows the dialog within 6 s (G-NH2)

```sh
say g-nh2-dialog
( aether-text & echo $! > /tmp/hang.pid )
sleep 1; kill -STOP "$(cat /tmp/hang.pid)"       # the app stops dispatching
# do: watch for the dialog; screenshot it; press Wait.
shot g-nh2-dialog
journalctl -b -u aether-compositor --no-pager | grep -E 'APP_STATE|FRAME_STATS' | tail -n 4
journalctl -b -u aether-shell --no-pager | grep AETHER_SHELL_NOHANG | tail -n 3
```

EXPECT:

```
AETHER_COMPOSITOR_APP_STATE: client=<n> state=not_responding after_ms=5000
AETHER_COMPOSITOR_FRAME_STATS: frames=<n> ...            # the desktop kept drawing
SCREENSHOT g-nh2-dialog                                  # "<App> is not responding"
AETHER_SHELL_NOHANG_DIALOG: client=<n> app=aether-text after_ms=5000
```

Pass conditions: the dialog is visible **within 6 s** of `kill -STOP` (the
`after_ms` value the compositor prints must be `≤ 6000`), the frame counter kept
increasing while the app was stopped (no desktop freeze), and the screenshot
shows both buttons.

### 1c-2 — Wait re-offers, Force Quit ends the app

```sh
# do: press Wait, leave the app stopped for 15 s, press Force Quit on the second dialog.
journalctl -b -u aether-shell --no-pager | grep AETHER_SHELL_NOHANG | tail -n 5
pgrep -a aether-text || echo "app ended"
journalctl -b -u aether-compositor --no-pager > /tmp/compositor.log
aether-shell --nohang-check /tmp/compositor.log; echo "nohang-check exit=$?"
```

EXPECT:

```
AETHER_SHELL_NOHANG_ACTION: client=<n> app=aether-text action=wait grace_ms=15000
AETHER_SHELL_NOHANG_DIALOG: client=<n> app=aether-text after_ms=5000      # 15 s later
AETHER_SHELL_NOHANG_ACTION: client=<n> app=aether-text action=force_quit
AETHER_SHELL_NOHANG_TERMINATE: client=<n>
AETHER_SHELL_NOHANG_SUMMARY: prompts=2 waits=1 force_quits=1 recoveries=0 open=0
app ended

NOHANG_HANG client=<n> after_ms=5000 within_limit=true
NOHANG_WATCHDOG timeouts=<n> last_state=<ok|timeout> recovered=<bool> idle_ms=<ms> timeout_ms=5000 restart_required=<bool>
NOHANG_START attempts=<n> confirmed=<bool> safe_mode=<bool>
AETHER_SHELL_NOHANG_OK
nohang-check exit=0
```

Recovery instead of Force Quit: `kill -CONT "$(cat /tmp/hang.pid)"` and EXPECT
`AETHER_SHELL_NOHANG_RECOVERED: client=<n> app=aether-text` with the dialog
closing on its own (no click). A late `after_ms=8000` fails the check with
`client=<n> was marked not responding after 8000 ms; G-NH2 allows 6000 ms`.

### 1c-3 — Shell watchdog and safe mode after 3 failed starts

```sh
say g-nh2-watchdog
# do: stall the shell three times (the compositor declares a timeout and the
#     service manager restarts only the shell; windows are kept).
for i in 1 2 3; do kill -STOP "$(pgrep -x aether-shell)"; sleep 6; kill -CONT "$(pgrep -x aether-shell)"; sleep 2; done
journalctl -b -u aether-compositor --no-pager | grep SHELL_WATCHDOG | tail -n 6
# Restart loop: three starts that never reach "session ready".
aether-shell --start-begin     # attempt=1
aether-shell --start-begin     # attempt=2, previous=unconfirmed
aether-shell --start-begin     # attempt=3, safe mode
aether-shell --status | grep -E '^(safe-mode|start-attempts|effects-effective):'
```

EXPECT:

```
AETHER_COMPOSITOR_SHELL_WATCHDOG: state=timeout idle_ms=<ms> timeout_ms=5000 timeouts=1
AETHER_COMPOSITOR_SHELL_WATCHDOG: state=ok recovered=true timeouts=1
... timeouts=3
AETHER_SHELL_START: attempt=1 safe_mode=false previous=clean
AETHER_SHELL_START: attempt=2 safe_mode=false previous=unconfirmed
AETHER_SHELL_START: attempt=3 safe_mode=true previous=unconfirmed
AETHER_SHELL_SAFE_MODE: attempts=3 timeouts=0 reason=3 starts failed in a row effects=minimal
safe-mode: true
start-attempts: 3
effects-effective: minimal
```

Restart within 5 s without losing windows: after each `kill -CONT` the
compositor must log `state=ok recovered=true` and `pgrep -a aether-*` must still
list the open apps (the compositor keeps their windows).

Watchdog escalation from the journal (3 stalls, healthy starts):

```sh
aether-shell --start-begin /tmp/compositor.log
```

EXPECT `AETHER_SHELL_SAFE_MODE: attempts=1 timeouts=3 reason=the shell stalled 3 times`.

The banner and its reset:

```sh
# do: in safe mode an amber banner reads "Safe mode: 3 starts failed in a row.
#     Glass effects are off; reset to leave it." Press Reset.
shot g-nh2-safe-mode
aether-shell --start-confirm    # attempts=0; the banner stays until reset
aether-shell --safe-mode-reset  # leaves safe mode
aether-shell --status | grep -E '^(safe-mode|effects-effective):'
```

EXPECT `AETHER_SHELL_START_CONFIRMED: attempts=0`,
`AETHER_SHELL_SAFE_MODE_RESET: attempts=0 safe_mode=false`, then
`safe-mode: false` and `effects-effective: full`. The Reset click must produce
the same reset marker in `journalctl -u aether-shell`.

The ledger lives at `~/.config/aether/shell/session.json`
(`{"attempts":3,"confirmed":false,"safe_mode":true,...}`).

Headless evidence (development host, no device):

```sh
cargo test -p aether-shell --lib nohang      # 12 unit tests
cargo test -p aether-shell --test nohang     # 7 command tests
cargo test -p aether-shell --test nohang_ui  # dialog + banner through the widget tree
```

EXPECT `test result: ok. 12 passed`, `7 passed`, `1 passed`.

#### 1c-0 — The interactive host is running (preflight for 1c-1..1c-3)

The rows above assume `aether-shell` is the interactive host, not the CLI. The
host reads the compositor journal, so the whole chain can be replayed without a
pointer by pointing it at a journal and pressing the button with `--act`:

```sh
say g-nh2-host
pgrep -a aether-shell
journalctl -b -u aether-shell --no-pager | grep -E 'HOST_(JOURNAL|READY|DONE)|START:' | tail -n 5

# Replay form (fixture journal, no display interaction, deterministic):
cp /tmp/compositor.log /tmp/host-fixture.log
aether-shell --interactive --journal /tmp/host-fixture.log \
    --frames 3 --step-ms 200 --act force-quit --frames-out /tmp/g-nh2-shots
echo "host exit=$?"
```

EXPECT (the journal lines came from the compositor, the markers from the host):

```
<pid> aether-shell
AETHER_SHELL_HOST_JOURNAL: path=/var/log/aether-compositor.log step_ms=250 frames=until-stopped
AETHER_SHELL_START: attempt=1 safe_mode=false previous=clean
AETHER_SHELL_HOST_READY: frames=1 polls=1
...
AETHER_SHELL_NOHANG_DIALOG: client=<n> app=<app> after_ms=<ms>
AETHER_SHELL_NOHANG_ACTION: client=<n> app=<app> action=force_quit
AETHER_SHELL_NOHANG_TERMINATE: client=<n>
AETHER_SHELL_START_CONFIRMED: attempts=0
AETHER_SHELL_HOST_DONE: frames=3 polls=3 dialogs=<n> actions=1 terminations=1 ready=true
AETHER_SHELL_HOST_TERMINATIONS_PUBLISHED: count=1
host exit=0
```

Pass conditions: the host starts before any surface exists (`AETHER_SHELL_START`),
confirms on the first presented frame (`AETHER_SHELL_START_CONFIRMED: attempts=0`)
and publishes the terminate intent for the client it force-quit. The ledger at
`~/.config/aether/shell/session.json` must read `"attempts":0,"confirmed":true`
after the run. `--act wait` is the same proof for the Wait re-offer; omit `--act`
for the click-driven rows 1c-1..1c-3.

If `HOST_DONE … ready=true` is missing, the host never presented a frame — check
for `AETHER_SHELL_HOST_CAPTURE_SKIPPED` (harmless) versus a `cannot show the
shell surfaces` error (fatal).

> Seams not claimed here: the layer-shell `Platform` that maps these surfaces on
> the real output, and the compositor/service-manager consumption of
> `AETHER_SHELL_NOHANG_TERMINATE`, still need the image. The loop, the dialog
> decisions, the ledger handshake and every marker are verified headlessly:
> `cargo test -p aether-shell --test host` (journal line → dialog → action →
> terminate marker) and `--test host_ledger` (start / confirm / safe mode through
> the host). Evidence: `docs/2026/11/27/results/g-nh2-shell-host.md`.

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
# do: open the control centre, toggle Wi-Fi and Bluetooth, move volume and brightness,
#     then switch Effects to Reduced / Minimal / Full.
# verify:
nmcli radio wifi
bluetoothctl show | grep -E 'Powered|Name'
wpctl get-volume @DEFAULT_AUDIO_SINK@
brightnessctl -m | cut -d, -f4
aether-shell --effects-tier
grep 'effects-tier' "$HOME/.config/aether/theme/tokens.toml"
```

EXPECT `enabled`/`disabled` follows the Wi-Fi toggle, `Powered: yes|no` follows
the Bluetooth toggle, `Volume: 0.xx` follows the slider, the brightness
percentage follows the slider, and the printed tier plus the config line follow
the Effects switch (`full` → `reduced` → `minimal`). Each control must change
its service or file within 1 s.

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
# do: control centre → Effects → Reduced, then Minimal, then Full.
# verify:
aether-shell --effects-tier
grep -E 'effects-tier|reduce-transparency' "$HOME/.config/aether/theme/tokens.toml"
journalctl -b -u aether-compositor --no-pager > /tmp/compositor.log
aether-shell --effects-check /tmp/compositor.log; echo "effects-check exit=$?"
shot shl-ac-07-reduced
shot shl-ac-07-minimal
# Then Settings → Appearance → "Reduce transparency" on, then off.
shot shl-ac-07-solid
shot shl-ac-07-glass
```

EXPECT the config file shows the new tier, `--effects-tier` prints the same
value, the panels re-render at the resolved tier (screenshots differ), and with
"Reduce transparency" on `--effects-tier` prints `minimal` regardless of the
requested tier. Section 1b defines the tier/frame/idle evidence; a compositor
started before the change reports the mismatch and needs a restart, which the
check names.

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

### SHL-AC-12 — Reduce motion, text scale, keyboard navigation and the focus ring

Headless preflight (development host, no display server):

```sh
aether-shell --a11y-status | tee /tmp/a11y-status.txt
echo "status exit=$?"
aether-shell --render-a11y /tmp/a11y-shots
cargo test -p aether-shell --lib a11y
cargo test -p aether-shell --lib kbdnav
cargo test -p aether-shell --test a11y
cargo test -p aether-shell --test a11y_ui
```

EXPECT `A11Y_REDUCE_MOTION: off`, `A11Y_TEXT_SCALE: 100%`, the five
`A11Y_KEYBOARD: surface=…` lines, `AETHER_SHELL_FOCUS: surface=menubar`, and
`AETHER_SHELL_A11Y_OK` as the last line; tests report `6 passed`, `5 passed`,
`2 passed`, `1 passed`.

On-device steps:

```sh
say shl-ac-12-reduce-motion
# do: Settings → Appearance → Reduce motion on; open and close the dock and
#     control centre. The panels must fade (120 ms), never slide.
# do: Settings → Appearance → Text scale → 200 %; open every surface.
shot shl-ac-12-text-200
aether-shell --a11y-status | grep -E 'A11Y_(TEXT_SCALE|MOTION)'

say shl-ac-12-keyboard
# do: with the pointer parked, press Tab repeatedly and screenshot each stop.
#     Tab through menu bar → dock → launcher → control centre → notifications.
#     Press Enter on a dock app, a quick tile, the effects segment and a card.
shot shl-ac-12-focus-ring
journalctl -b -u aether-shell --no-pager | grep AETHER_SHELL_FOCUS | tail -n 10

say shl-ac-12-orca
# do: start orca, Tab to a dock item and a launcher result.
grep -i "focused" /tmp/orca.log | head
```

EXPECT at 200 %: menu bar, control centre and settings text stay inside their
panels (nothing clipped at the 2× step), the effect tier is untouched by text
scale, and `A11Y_TEXT_SCALE: 200% scale=2.00` with
`A11Y_MOTION: slide_ms=0 fade_ms=120 motion_ms=120` when reduce motion is on.
Every Tab stop draws the 2 px accent ring on exactly one item, the journal shows
one `AETHER_SHELL_FOCUS` line per step, and Orca announces the focused control
(a silent focus change is a failure). Reduce transparency on must resolve the
tier to Minimal (`aether-shell --status | grep effects-effective:`).

Recorded hardware pass: pending the Dell run. Headless evidence:
`docs/2026/11/27/results/shl-18-accessibility.md`.

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

### DA-AC-13 — Browser security posture and update channel (P4-T18b)

```sh
say browser-security
aether-browser --security-status; echo "status exit=$?"
aether-browser --security-check;  echo "check exit=$?"
cat "$HOME/.config/aether/browser/security.toml" 2>/dev/null || echo "security.toml absent (defaults: stable, HTTPS-only)"
pkg-config --modversion webkit2gtk-4.1 || echo "webkit2gtk-4.1 not installed"
```

EXPECT on the image:

```
AETHER_BROWSER_SECURITY_OK
channel=stable updates=os-image
https-only=on mixed-content=blocked third-party-cookies=blocked
engine=webkitgtk version=<2.44 or newer> minimum=2.44 verdict=supported
private-window=no-profile-writes
downloads=prompt
status exit=0
check exit=0
```

Set `channel = "beta"` in `security.toml`, re-run, and EXPECT
`channel=beta updates=os-image` with exit 0. An engine below 2.44 must print
`verdict=outdated`, the problem line naming the floor and the channel, and exit
4; an image without WebKitGTK must print `verdict=absent` and make
`--security-check` exit 5. The development host has only the adapter, so it
prints `AETHER_BROWSER_SECURITY_NOTE` and exits 0 for `--security-status`.

Headless regression: `cargo test -p aether-browser --test security` (5 tests).

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
- Effect tier: `cargo test -p aether-shell --lib effects` (8),
  `--test effects` (6), `--test effects_ui` (1) and
  `--test layer_host` (6, now including a journal with effect/frame markers) —
  `docs/2026/11/27/results/p4-t04-t18b-effects-and-browser-security.md`.
- Browser security: `cargo test -p aether-browser --lib security` (8) and
  `--test security` (5), plus `aether-browser --security-status` /
  `--security-check` — same results log.
- Calendar & Clock: `cargo test -p aether-calendar` (21 lib incl. week grid,
  time zones, alarms; 1 headless UI test) and the three `--render` surfaces —
  `docs/2026/11/26/results/p4-t16-t18a-media-calendar.md`.
- Default app unit suites: text 20, calculator 11 + 2 acceptance, images 12,
  archive 15, each with a headless UI test.
- Features page: `cargo test -p aether-shell --lib features` (9) and
  `--test features` — `docs/2026/11/26/results/p7-t07-features-ui.md`.
- G-NH2 shell side: `cargo test -p aether-shell --lib nohang` (12),
  `--test nohang` (7) and `--test nohang_ui` (1), plus
  `aether-shell --nohang-check` / `--start-begin` / `--start-confirm` /
  `--safe-mode-reset` — `docs/2026/11/27/results/g-nh2-shell-nohang.md`.

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
- The effect-tier switch applies to the next compositor start (the tier is read
  once at startup); `--effects-check` reports the mismatch until the session
  launcher exports `AETHER_COMPOSITOR_EFFECT_TIER` and the compositor restarts.
  The measured 60 fps Full-tier gate still needs the Dell GPU (`measured=false`).
- Browser security (DA-AC-13) needs WebKitGTK in the image; the policy, channel
  and version floor are already testable without it.
- G-NH2 on device (§1c) needs the interactive shell host to read the compositor
  journal and a service manager restart rule for `aether-shell`; the dialog,
  safe-mode ledger and the marker contract are verified headlessly today. Force
  Quit is an intent marker (`AETHER_SHELL_NOHANG_TERMINATE`) until the host wires
  the compositor's xdg unresponsive error.

## 6. Graphical session evidence (added 2026-11-28)

`sh /root/test.sh` gained a `### UI / GRAPHICAL SESSION (P3/P4)` section that
records, without any manual step:

- `aether-session.service` / `aether-shell.service` active+enabled state,
- `/etc/aether/ui-build.txt` (the post-build manifest: rustc, compositor
  features, installed binaries, wiring guard result),
- presence of `aether-compositor`, `aether-shell`, `aether-greeter`,
- `/dev/dri` nodes and every `card*-*/status` connector,
- the last 20 lines of `/var/log/aether/compositor.log` plus marker counts
  (`SOCKET_READY`, `CLIENT_MAPPED`, `SEAT`, `FOCUS`),
- the last 10 journal lines of both units.

A successful first graphical boot therefore shows `aether-session
active/enabled`, `SOCKET_READY: 1`, `CLIENT_MAPPED: 1` and an `eDP` connector
marked `connected`, with the shell's surfaces visible on tty1.

Boot switching for repeated testing is described in
`docs/2026/11/28/results/boot-switch-ubuntu-aetheros.md`: from Ubuntu run
`aether-boot-switch aether` before rebooting, and AetherOS hands the next boot
back to Ubuntu by itself.
