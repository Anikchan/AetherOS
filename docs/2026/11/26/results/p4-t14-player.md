# D6 — P4-T14 Player (GStreamer backend + codec policy)

- Date: 2026-11-26
- Session: S3
- Task: P4-T14, output `apps/player`, gate DA-AC-06
- Host: macOS development host (no GStreamer installed)

## What was verified

| Check | Command | Result |
| --- | --- | --- |
| Player tests | `cargo test -p aether-player` | 17 lib, 1 `codec`, 1 `ui` — all pass |
| status | `cargo run -p aether-player -- --status` | reports `gst-play-1.0`/`gst-discoverer-1.0` missing and the restricted-codec list |

## Evidence

- `gst-discoverer-1.0` output parsing: durations with and without hours
  (`0:03:12.345` = 192.345 s, `00:03.250` = 3.250 s, `1:02:03.5` = 3723.5 s);
  negative/invalid values rejected; audio/video caps captured.
- Codec policy: `video/x-h264` → H.264, `video/x-h265`/HEVC → H.265,
  `audio/mpeg, mpegversion=(int)4` → AAC; MP3 and Opus allowed.
- Controller gate: a file carrying H.264/AAC does not reach the engine, the
  transport stays `Stopped` and the status says "needs the optional codec
  bundle (P0-T03)".
- Allowed media: discovered duration replaces the playlist estimate
  (232 s) and the engine receives `play file:///…`.
- Missing discoverer: playback proceeds and the engine reports failures.
- `--status` prints backend, discoverer and the restricted codec list.

## Not verified (honest status)

- Real playback through GStreamer: not installed on the development host and no
  audio device in the headless run. The adapter shells out to `gst-play-1.0`
  (already unit-tested against a fake `PATH`); pause/seek remain transport-side
  because the CLI player has no control socket.
- On-device DA-AC-06 audio check remains with the image/hardware run.
