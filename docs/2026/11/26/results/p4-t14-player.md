# D6 — P4-T14 Player (GStreamer backend + codec policy)

- Date: 2026-11-26
- Session: S3
- Task: P4-T14, output `apps/player`, gate DA-AC-06
- Host: macOS development host (no GStreamer installed)

## What was verified

| Check | Command | Result |
| --- | --- | --- |
| Player tests | `cargo test -p aether-player` | 19 lib, 1 `codec`, 1 `transport_edges`, 1 `ui` — all pass |
| status | `cargo run -p aether-player -- --status` | reports `gst-play-1.0`/`gst-discoverer-1.0` missing and the restricted-codec list |

## Play / pause / seek edge cases (2026-11-26)

The adapter and the controller now agree on one rule: **record the position,
never seek to it.** `Engine::set_position` only records; `Engine::seek` relaunches
a pipeline only while one is running. Verified by
`engine::tests::pause_seek_and_resume_never_leave_a_stray_pipeline`, which runs
the real `GStreamerEngine` against a `gst-play-1.0` stand-in that appends its
argument list to a log, so the number and order of process starts is asserted:

| Step | Assertion |
| --- | --- |
| `play` at 0 | one invocation, no `--start-position` |
| `seek(42)` while playing | exactly one relaunch: `--start-position=42 <uri>` |
| `pause` twice | no invocation, position kept at 42, pipeline gone |
| `seek(90)` while paused | no invocation, position is 90 |
| resume | one invocation at `--start-position=90` |
| `seek(999)` on a 200 s track | clamped: `--start-position=200` |
| `seek(0)` | flag dropped again |
| `stop` twice | no invocation, position 0, URI cleared |
| empty URI (`""`, `"   "`) | `EngineError::Failed("empty media URI")`, nothing spawned |

Controller order (`tests/transport_edges.rs`, shared `MockEngine`):

| Action | Engine calls |
| --- | --- |
| Play | `position 0`, `play <uri>` (no stray seek) |
| Pause | `pause`, `position 30` (a seek first would relaunch `gst-play-1.0` just to kill it) |
| Resume | `position 30`, `play <uri>` |
| Slider while paused | one `seek 116`, no pipeline start |
| Next track | `play <new uri>` last, no `seek` of the old URI |
| Repeat one at the end | `play <same uri>` from zero |
| Restricted codec | no engine call at all |

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
  (unit-tested against a fake `PATH`, including the exact number of process
  starts for play/pause/seek); the pause/seek position remains transport-side
  because the CLI player has no control socket.
- On-device DA-AC-06 audio check remains with the image/hardware run
  (`docs/2026/11/26/results/on-device-acceptance-harness.md` §3).
