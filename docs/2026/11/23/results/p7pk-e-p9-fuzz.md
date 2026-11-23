# P7-PK-T02 Pack E + P9-T06 fuzzing (partial)

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run

## P7-PK-T02 — Screen Time limits and downtime (Pack E)

`powerd/src/limits.rs` + `aether-limits` (powerd suite now **64 tests**):

- per-app daily allowance in minutes, one extension per day;
- warn at 90% (configurable), block at the limit;
- usage keyed by epoch day, so it resets at midnight without a timer;
- downtime windows, including windows that cross midnight (22:00–07:00);
- downtime can target all apps or a specific app list;
- config (TOML) and state (JSON) round trip.

CLI smoke:

```text
$ aether-limits set org.example.Chat 30 --extension 15
LIMIT_SET org.example.Chat 30m extension=15m
$ aether-limits record org.example.Chat 1800 --day 100
USAGE_RECORDED org.example.Chat 1800s day=100
$ aether-limits decide org.example.Chat --day 100 --minute 720
"BlockedLimit { used_seconds: 1800, limit_seconds: 1800 }"   (exit 3)
```

The enforcement point is the shell/compositor (S3): it calls `decide` on focus
and shows the extension prompt. The core semantics are tested here.

## P9-T06 — fuzzing harness (partial, no findings claimed)

Deterministic fuzz tests run on every `cargo test`:

| Target | Corpus | Result |
|---|---|---|
| `aether-pkg` manifest TOML parser | 20,000 random texts + 5,000 structured mutations + extreme lengths | no panic; invalid input returns `Err` |
| `aether-installer` lsblk JSON parser | 10,000 random JSON-like texts + 5,000 mutations | no panic; malformed input returns `Err` |
| installer planner | 2,000 extreme size/free-space combinations | no panic; returns a plan or an error |

This is parser-level fuzzing, not a full campaign. A dedicated fuzzing run with
coverage guidance and a review of the installer/package service/Android bridge
is still required before the release claim (P9-T06 remains open).
