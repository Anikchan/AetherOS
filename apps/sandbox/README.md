# aether-sandbox — sandbox, permission broker and Landlock enforcement (P6-T04)

Profiles come from the persisted **grants**, never from the manifest alone (a
denied permission stays denied). Launch is always enforced and **fail-closed**:
if bubblewrap, user namespaces or Landlock are unavailable, the app does not
start and the reason is printed — it is never started unsandboxed
(D8 section 5 failure mode).

## Layers

| Layer | Where | What it stops |
|---|---|---|
| Namespaces + mounts | `src/enforce.rs` via `bwrap` | App sees only its granted mounts, no network unless granted, no host PID/IPC/UTS |
| Landlock ruleset | `src/landlock.rs` + `src/bin/aether-landlock.rs` | Filesystem access outside the profile paths, even through symlinks or `..` |
| Permission broker | `src/permissions.rs` | Prompts, "don't ask again" denials that survive restarts |
| Profiles | `src/profile.rs` | Builds the path set from grants (read-only system dirs, writable app folder) |

## Landlock (fail-closed)

`landlock::probe()` asks the kernel for the Landlock ABI
(`landlock_create_ruleset` with the VERSION flag). `landlock::plan()` masks the
requested rights down to the ABI the kernel supports (ABI 2 adds `refer`,
ABI 3 `truncate`, ABI 5 `ioctl-dev`) and returns an error when nothing can be
enforced.

`Enforcer::command_with_landlock` wraps the entry point with the shim inside
the namespace sandbox:

```
bwrap … -- /usr/libexec/aether-landlock --ro /usr --ro /etc --rw /data/app-… -- /data/app-…/bin/app
```

The shim applies the ruleset (create → add rules → `PR_SET_NO_NEW_PRIVS` →
`restrict_self`) and only then `exec`s the app. On any failure it exits 126
without executing the program, so a partial ruleset can never let the app run.

`Runner::require_landlock(true)` (CLI: `aether-runner launch APP --strict-landlock`)
uses this path. The image must install the shim at
`/usr/libexec/aether-landlock`.

## Commands

```bash
aether-sandbox doctor                 # capabilities, Landlock ABI, strict verdict
aether-sandbox profile --manifest APP/manifest.json --data DIR --granted network
aether-sandbox landlock --manifest APP/manifest.json --data DIR   # plan or refusal
aether-sandbox grant APP-ID PERMISSION
aether-sandbox revoke APP-ID PERMISSION
aether-sandbox list APP-ID

aether-landlock --probe [--json]      # ABI or LANDLOCK_UNAVAILABLE (exit 126)
aether-landlock --ro /usr --rw /data/app -- /data/app/bin/app
```

## Honest limits

- Kernel enforcement is only exercised on Linux. On the macOS dev host the
  shim and the plan gate are tested for their refusal path (exit 126, program
  never run); the allow/deny ruleset test is skipped with a printed reason.
- Landlock cannot mediate network access below ABI 4 TCP rules; network is
  still controlled by the namespace (`--unshare-net`) and by the permission
  broker.
- seccomp filtering is a separate layer and is not implemented yet (P6-T04
  remainder).
