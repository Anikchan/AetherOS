# P4-T04 ↔ P3-T05 effect-tier control + P4-T18b browser security channel

- Date: 2026-11-27
- Session: S3 (`session - 3 (Nov26)`)
- Scope: (1) control-centre Full/Reduced/Minimal effects control wired to the
  compositor's tier env/marker contract and persisted through aether-config;
  (2) shell tests for the tier control and the layer-host check; (3) acceptance
  harness effect-tier + frame/idle checks; (4) P4-T18b browser security and
  update-channel surface.
- No `compositor/`, `os-build/`, `lifecycled/`, `powerd/`, `installer/` or
  tracker §4 changes: the compositor side is consumed read-only (env + markers).

## 1. Changes

| File | Purpose |
| --- | --- |
| `shell/src/effects.rs` (new) | `EffectSettings` (requested tier + reduce-transparency → effective tier), persistence through `SettingsStore`, the `AETHER_COMPOSITOR_EFFECT_TIER` export, and `EffectsReport` which reads `AETHER_COMPOSITOR_EFFECT_TIER`, `..._EFFECT_TIER_SWITCH`, `..._EFFECT_SUMMARY`, `..._FRAME`, `..._FRAME_CALLBACKS`, `..._IDLE`, `..._FRAME_STATS` and reports tier/restart, frame, idle and frame-budget problems. |
| `shell/src/app.rs` | Controller owns the resolved tier: loads it at startup, sets `ShellRoot.tier`/`effects-tier`/`effects-note` on every sync, handles the control-centre callback, reloads on `theme.reduce-transparency`, and keeps the settings-app path in step. |
| `shell/src/host.rs` | `field()` made `pub(crate)` so `effects` reads markers with the same parser. |
| `shell/src/main.rs` | `--effects-tier`, `--effects-env`, `--effects-check <log|->`; `--status` now prints the requested/effective tier and env line. |
| `shell/ui/surfaces.slint` | Control centre: "Effects" row + Full/Reduced/Minimal segmented control + material subtitle, wired through `ShellRoot.effect-tier-changed`. Panel height 428→508 px. |
| `shell/tests/effects.rs` (new) | 6 command tests: default Full, persisted tier → env, accessibility override, mismatch, missing markers/cost overrun, stdin. |
| `shell/tests/effects_ui.rs` (new) | 1 headless UI test driving the real switch (persistence, live tier, env, settings path, reduce-transparency, fresh-controller reload). |
| `shell/tests/layer_host.rs` | +1 test: `--host-check` accepts a real journal that interleaves effect/frame/idle markers and still reports only the layer contract. |
| `apps/browser/src/security.rs` (new) | Update channel (stable/beta, `os-image` source), WebKitGTK floor `2.44` with dotted-version compare, hardening baseline (HTTPS-only, mixed content, third-party cookies), `security.toml` loading, status/problem lines. |
| `apps/browser/src/main.rs` | `--security-status` (adapter-only host is a note) and `--security-check` (engine must be installed). Exit codes 0/2/4/5. |
| `apps/browser/tests/security.rs` (new) | 5 command tests: supported, outdated floor, absent engine, beta/HTTPS-only-off, malformed file. |
| `apps/browser/Cargo.toml` | Adds `toml = "1.2"` for `security.toml`. |
| `docs/2026/11/16/os-project/features/01-desktop-shell.md` | SHL-AC-04/SHL-AC-07 updated, new §10 describing the tier contract. |
| `docs/2026/11/26/results/on-device-acceptance-harness.md` | New §1b (effect tier + frame/idle checks), SHL-AC-04/07 commands, DA-AC-13 (browser security), evidence and blocker updates. |

## 2. The contract (unchanged on the compositor side)

| Direction | Interface |
| --- | --- |
| Shell → compositor | `AETHER_COMPOSITOR_EFFECT_TIER=full\|reduced\|minimal` (read once at startup) |
| Shell → config | `theme.effects-tier`, `theme.reduce-transparency` (`theme/tokens.toml`) |
| Compositor → shell | `AETHER_COMPOSITOR_EFFECT_TIER`, `..._EFFECT_TIER_SWITCH`, `..._EFFECT_SUMMARY`, `..._FRAME`, `..._FRAME_CALLBACKS`, `..._IDLE`, `..._FRAME_STATS` |

The compositor reads the tier only at startup, so a change applies to the next
compositor start (runtime switches are its own frame-budget policy). The shell
therefore does not claim a live switch: `--effects-check` compares the log with
the setting and prints
`compositor runs tier <x>, the setting resolves to <y> — restart the compositor to apply it`.
`reduce-transparency = true` resolves to Minimal regardless of the request
(D1 §9).

## 3. Verification (run on the development host, 2026-11-27)

```
$ cargo test -p aether-shell -p aether-browser -p aether-config
aether-browser lib      54 passed; 0 failed
aether-browser security  5 passed; 0 failed
aether-browser ui        1 passed; 0 failed
aether-config lib       34 passed; 0 failed
aether-shell lib        99 passed; 0 failed
aether-shell effects     6 passed; 0 failed
aether-shell effects_ui  1 passed; 0 failed
aether-shell features    1 passed; 0 failed
aether-shell greeter     1 passed; 0 failed
aether-shell interaction 1 passed; 0 failed
aether-shell layer_host  6 passed; 0 failed
```

Command checks (temporary `XDG_CONFIG_HOME`):

```
$ printf '[theme]\neffects-tier = "reduced"\n' > "$XDG_CONFIG_HOME/aether/theme/tokens.toml"
$ aether-shell --effects-tier
reduced
$ aether-shell --effects-env
# effects: requested=reduced reduce-transparency=false
export AETHER_COMPOSITOR_EFFECT_TIER=reduced
$ aether-shell --status | tail -4
effects-tier: reduced
effects-effective: reduced
effects-env: AETHER_COMPOSITOR_EFFECT_TIER=reduced
AETHER_SHELL_STATUS_OK
```

```
$ aether-shell --effects-check /tmp/comp.log
EFFECT_TIER expected=reduced compositor=reduced switches=0 last_switch=none
EFFECT_FRAMES frames=12 idle_skips=48 idle_pct=93.8 callbacks=5 markers=1 est_ms=1.601 budget_ms=16.67
AETHER_SHELL_EFFECTS_OK
exit=0

$ aether-shell --effects-check /tmp/comp2.log          # log says full, setting reduced
EFFECT_TIER expected=reduced compositor=full switches=0 last_switch=none
EFFECT_FRAMES frames=- idle_skips=- idle_pct=- callbacks=- markers=0 est_ms=- budget_ms=16.67
AETHER_SHELL_EFFECTS_PROBLEM: compositor runs tier full, the setting resolves to reduced — restart the compositor to apply it
AETHER_SHELL_EFFECTS_PROBLEM: no AETHER_COMPOSITOR_FRAME_STATS marker: the compositor did not report frames
AETHER_SHELL_EFFECTS_PROBLEM: no idle measurement: AETHER_COMPOSITOR_FRAME_STATS carries no idle_pct
AETHER_SHELL_EFFECTS_FAILED
exit=1
```

```
$ aether-browser --security-status            # development host: adapter only
AETHER_BROWSER_SECURITY_OK
channel=stable updates=os-image
https-only=on mixed-content=blocked third-party-cookies=blocked
engine=webkitgtk version=absent minimum=2.44 verdict=absent
private-window=no-profile-writes
downloads=prompt
AETHER_BROWSER_SECURITY_NOTE: WebKitGTK is not installed on this host (engine adapter only); --security-check runs on the image
exit=0
```

Visual check: `aether-shell --render /tmp/aether-eff-shots` wrote all ten
surfaces; `shell-control-center.png` shows the Effects row
("Blur, refraction and shadow") with the Full/Reduced/Minimal control and no
clipping.

## 4. On-device steps that remain (hardware paused)

- Export the tier in the session launcher
  (`eval "$(aether-shell --effects-env)"` before `aether-compositor`) and run
  harness §1b after a compositor restart: `EFFECT_TIER expected=<tier>
  compositor=<tier>` and exit 0.
- Measured 60 fps Full-tier gate with three windows (the compositor's GPU
  profile is still `measured=false`); frame cost/idle numbers in harness §1b.
- Browser security: real WebKitGTK version, `verdict=supported`, and a beta
  channel run on the image (DA-AC-13).

## 5. Notes for the other sessions

- No compositor edit was needed: the tier env and all markers already exist in
  `compositor/src/glass.rs`/`lib.rs` (P3-T05/T06).
- The session launcher that starts `aether-compositor` must export the tier;
  until then the effective tier is Full, which is the compositor default.
- F8 (`docs/2026/11/17/os-addendum-1/F8-default-apps.md`) is not S3 territory;
  its browser row still lists P4-T18b as "bookmarks, history, downloads" and
  could gain the security-channel line when its owner next edits it.
