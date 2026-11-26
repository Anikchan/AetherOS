# D6 — P4-T18a (browser media) and P4-T16 (Calendar & Clock v1)

- Date: 2026-11-26
- Session: S3 (`session - 3 (Nov26)`)
- Tasks: P4-T18a (browser media and streaming, gate DA-AC-11), P4-T16
  (Calendar and Clock v1, gate DA-07)
- Host: macOS development host (no GStreamer, no VA-API, no WebKitGTK, no
  display) — every number below is from the headless test suites and the
  software renderer.

## 1. P4-T18a — shared media policy and browser media

### What changed

| File | Purpose |
| --- | --- |
| `apps/media/` (new crate `aether-media`) | the one codec/DRM decision table both apps use: restricted-codec detection (caps **and** RFC 6381 manifest strings), `CodecPolicy`, `blocked_message`, VA-API acceleration detection, `MediaCapabilities`, rendition-ladder selection |
| `apps/player/src/codec.rs` | delegates `restricted_codec`/`CodecPolicy`/`blocked_message` to `aether-media` and keeps its GStreamer discovery; the public player API is unchanged |
| `apps/browser/src/media.rs` | URL classification (`page`, `progressive-video`, `progressive-audio`, `hls`, `dash`), the ladder each kind offers, `plan_for`/`status_line` |
| `apps/browser/src/lib.rs` | media note in the status bar after a navigation (hardware decode, reduced rung, or the codec-bundle hint) |
| `apps/browser/src/main.rs` | `--media-status` and `--media-check <url>` for the acceptance harness |

### Policy (P0-T03 + Q24)

1. DRM is refused unless the owner enabled protected content
   (`PlaybackBlock::DrmNotEnabled`).
2. Restricted codecs (H.264, H.265/HEVC, AAC) play only with the optional
   bundle. Manifest strings are recognised too: `avc1.*`, `hvc1.*`, `mp4a.40.*`
   are restricted; `mp4a.6b` (MP3 in MP4) is not.
3. The highest playable rung wins (resolution, then bandwidth).
4. `reduced = true` when a higher rung exists but needs the bundle — the
   "HD-or-reduced playback" answer from Q24, reported in the status bar.
5. Hardware decode whenever VA-API is present (`vainfo`/`LIBVA_DRIVER_NAME`),
   overridable with `AETHER_MEDIA_HW_DECODE=on|off`.

### Verified

| Check | Command | Result |
| --- | --- | --- |
| Shared policy | `cargo test -p aether-media` | 10 passed |
| Player unchanged | `cargo test -p aether-player` | 19 lib, 1 `codec`, 1 `transport_edges`, 1 `ui` — all pass |
| Browser | `cargo test -p aether-browser` | 46 lib (6 `media`), 1 headless UI — all pass |
| Capability report | `cargo run -p aether-browser -- --media-status` | `AETHER_BROWSER_MEDIA_OK`, `acceleration=software backend=none`, `codec-bundle=absent`, `drm=disabled`, `restricted=H.264,H.265/HEVC,AAC,MPEG-4 AAC`, `streaming=hls,dash` |
| HLS policy | `--media-check https://cdn.example/live/index.m3u8` | `kind=hls playable=true reduced=true`; `playing VP9 720p — software decode; reduced quality (the codec bundle adds the HD rung)`; exit 0 |
| WebM policy | `--media-check https://a.example/clip.webm` | `kind=progressive-video playable=true reduced=false`; `playing VP9 1080p — software decode`; exit 0 |
| MP4 without bundle | `--media-check https://a.example/clip.mp4` (unit test) | `playable=false`, `H.264, AAC playback needs the optional codec bundle (P0-T03)`, exit 3 |

Rung decisions asserted by tests:

| Ladder | Caps | Chosen |
| --- | --- | --- |
| H.264 1080 + VP9 720 (HLS) | software, no bundle | VP9 720, hardware=false, **reduced** |
| H.264 1080 + VP9 720 | hardware, no bundle | VP9 720, **hardware**, reduced |
| H.264 1080 + AAC (MP4) | no bundle | blocked, needs bundle |
| same | bundle installed | H.264 1080, not reduced |
| VP9 720 ×2 | no bundle | higher bandwidth wins |
| any | `drm=true`, DRM off | blocked, `DrmNotEnabled` |

## 2. P4-T16 — Calendar & Clock v1 (DA-07)

DA-07 asks for "month and week view; clock, alarm, timer, stopwatch; time
zones". Month view, clock, timer and stopwatch existed; this round adds the
missing week view, alarms and time zones (core + UI + persistence).

### What changed

| File | Purpose |
| --- | --- |
| `apps/calendar/src/calendar.rs` | `Date::start_of_week`, `week_grid`, `WeekGrid`, `week_label` (same-month, cross-month and cross-year forms), shared `config_dir()` |
| `apps/calendar/src/clock.rs` | `Zone`/`ZONES`/`offset_clock`, `Alarm` (repeat days, `matches`, `minutes_until`, validation), `AlarmStore` (atomic JSON beside `events.json`) |
| `apps/calendar/src/app.rs` | week-tab model, morning-to-night alarm polling once per minute + banner, zone selection, alarm add/toggle/remove, persistence |
| `apps/calendar/ui/app.slint` | tabs `Month / Week / Clock`; week columns with per-day event titles; alarm banner, alarm list/form, time-zone grid |
| `apps/calendar/src/main.rs` | clock render at tab 2, new week render, `--alarms` |

### Verified

| Check | Command | Result |
| --- | --- | --- |
| Core + UI | `cargo test -p aether-calendar` | 21 lib, 1 headless UI — all pass |
| Renders | `cargo run -p aether-calendar -- --render /tmp/aether-cal-shots` | `AETHER_APP_CALENDAR_RENDERED … (1200x800)`, `AETHER_APP_CALENDAR_WEEK_RENDERED …`, `AETHER_APP_CALENDAR_CLOCK_RENDERED …` |
| Alarm dump | `cargo run -p aether-calendar -- --alarms` | `[]` on a fresh profile |

The headless UI test drives the real widgets: clicking `Week` sets tab 1 and a
seven-column model, `Next week` moves the selection by seven days, `New Delhi`
switches the zone (`UTC+05:30`), the alarm form adds `Wake up · 07:30`, the
banner rings once per minute and `Dismiss` clears it, and `Remove alarm Wake up`
deletes it.

Rule checks (unit): week starts on Monday and spans 7 days across month/year
boundaries; labels `23 – 29 November 2026`, `30. Nov – 6. Dec 2026`,
`28. Dec 2026 – 3. Jan 2027`; zones wrap past midnight (`23:30 +60 → 00:30`,
`01:15 −480 → 17:15`) and `UTC+05:30` is formatted; alarms repeat daily/weekdays/
weekends/custom days, `minutes_until` crosses weekends (`Fri 10:00 → Mon 09:00 =
4260 min`), invalid times and duplicate ids are refused; a corrupt alarms file
is reported and the loaded alarms survive, and disabled alarms do not fire.

## 3. Default apps — unit-test gaps closed

| App | New test(s) | Count before → after |
| --- | --- | --- |
| Text | save without a path, first-run recent list, `short_path`/autosave path, encoding labels | 18 → 20 |
| Images | missing folder error, `ThumbnailCache::get`, negative rotation steps | 11 → 12 |
| Archive | empty `create_tar_gz` refused, `tree_size`, `format_bytes` boundaries | 14 → 15 |
| Calculator | no gap found (50-case acceptance + error/incomplete coverage already present) | 11 + 2 |

## 4. Acceptance harness

`docs/2026/11/26/results/on-device-acceptance-harness.md` (rev 2) now has one
executable block per row for SHL-AC-01…11 and DA-AC-01…12, expected output for
each (including the `AETHER_*` markers and the `--media-status` /
`--media-check` / `--events` / `--alarms` lines), plus a new §4 for DA-07 and a
recording template. Concrete commands are only listed where the tooling exists
in the image plan; hardware-only steps are marked as observations with the exact
evidence to capture.

## 5. Not verified (honest status)

- Real media playback in WebKitGTK, VA-API hardware decode and any streaming
  site: hardware- and graphics-gated (P2/P3). The policy and the status lines
  are fully covered headlessly.
- DA-AC-11 playback itself, DA-AC-09/10/12 timings and every SHL-AC row remain
  on-device checks in the harness.
- Calendar alarms only fire while the app's tick loop runs; a system-level
  alarm daemon is not part of v1.
