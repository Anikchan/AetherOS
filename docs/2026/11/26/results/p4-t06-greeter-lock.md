# D6 — P4-T06 Lock screen and login (greetd + custom greeter)

- Date: 2026-11-26
- Session: S3
- Task: P4-T06, output `shell/lockscreen`, gate F1 SHL-AC-08
- Host: macOS development host (headless; no greetd, no libpam)

## What was verified

| Check | Command | Result |
| --- | --- | --- |
| Shell tests (unit + greeter + interaction + features) | `cargo test -p aether-shell` | 79 lib, 1 `greeter`, 1 `features`, 1 `interaction` — all pass |
| PAM FFI type-check | `cargo check -p aether-shell --features pam` | Finished, no warnings |
| greeter status | `cargo run -p aether-shell --bin aether-greeter -- --status` | `pam-linked: false`, socket `/run/greetd-ipc` absent, session `aether-shell`, `AETHER_GREETER_STATUS_OK` |
| greeter surfaces | `aether-greeter --render /tmp/aether-greeter-shots` | writes `greeter-login.png` (1440×900) and `greeter-denied.png` |
| missing greetd | `aether-greeter --stdin` (no socket) | exits 1 with `greetd is not available: /run/greetd-ipc: No such file or directory` |

## Evidence

- greetd protocol: request JSON asserted byte-for-byte against greetd-ipc(7)
  (`{"type":"create_session","username":"bob"}`), native-endian u32 framing,
  all three response variants decoded, real UNIX-socket round-trip.
- Login flow: secret prompt → authenticated; info/error messages answered with
  no response; `auth_error` reports attempts left; attempt budget cancels the
  session; refused `start_session` returns to the form; missing greetd is
  `Unavailable`, not a panic.
- Lock screen: five wrong attempts rate-limit the surface for the next minute
  tick; only the backend's `Allowed` unlocks; PAM status mapping covers
  auth/maxtries/account/service errors.
- Rendered surfaces: `docs/2026/11/23/results/screenshots` pipeline unchanged;
  greeter frames inspected manually (brand, account/password fields, Sign in,
  attempt message in danger colour).

## Not verified (honest status)

- Real PAM authentication on AetherOS: requires the image built with
  `--features pam`, `pam_unix`, `/etc/pam.d/aether-lock` and greetd installed.
  The FFI compiles and the mapping is tested; the syscall path is not executed
  on the macOS host.
- Layer-shell hosting of the greeter window (P3-T02/S2).
