# P6-T05 / P6-T06 — Windows engine packaging and installer flow (D6 evidence)

- Date: 2026-11-26
- Session: S4
- Host: macOS arm64 (engine execution simulated with a stub `wine` script)
- Gates: AP-AC-03 (installer runs in its own prefix, app launches from the
  launcher), AP-AC-04 (removing an app removes only its prefix)

## Delivered

| Part | File |
|---|---|
| Engine package format, checksums, versioning | `apps/runner/src/engine_package.rs` |
| Engine registry discovery/verify/package | `apps/runner/src/lib.rs` |
| Windows install flow, record, launcher, prefix-only uninstall | `apps/runner/src/windows.rs` |
| CLI | `apps/runner/src/bin/aether-runner.rs` |

Engine manifest (`engines/wine-10.0/aether-engine.toml`) pins every file's
size + SHA-256; `engines verify` rejects a tampered engine before it runs an
app. `Runner::plan` prefers the program recorded by the installer over the
installer payload itself.

## Verification

```text
$ cargo test -p aether-runner
21 passed; 0 failed

$ aether-runner engines package --from /tmp/aeth-run/eng/wine-10.0 --version 10.0
ENGINE_PACKAGED wine-10.0 files=1 bytes=125
$ aether-runner engines verify
wine-10.0	verified

$ aether-runner windows install --installer /tmp/aeth-run/setup.exe \
    --engine-root /tmp/aeth-run/eng --root /tmp/aeth-run/store
WINDOWS_INSTALLED win.setup prefix=.../prefixes/win.setup \
  entry=drive_c/Program Files/Hello/hello.exe launcher=.../win.setup.desktop executed=true

$ aether-runner windows uninstall win.setup --engine-root /tmp/aeth-run/eng --root /tmp/aeth-run/store
WINDOWS_UNINSTALLED win.setup prefix_removed=true engine_kept=true
$ test -x /tmp/aeth-run/eng/wine-10.0/bin/wine && echo ENGINE_KEPT
ENGINE_KEPT
```

Unit tests cover: packaging round-trip + tamper detection, registry listing,
per-prefix installs, entry validation (`../escape.exe` rejected), dry-run writes
nothing, and uninstall leaving other prefixes and the shared engine intact.

## Honest limits

- The vendor installer was executed with a stub `wine` that produced a
  `drive_c/Program Files/Hello/hello.exe`; a real Wine engine and a real
  Windows installer need AetherOS on the Dell.
- `.msi` installers are staged through `aether-pkg`; running `msiexec` inside
  the prefix is the same flow with a different argv and is not separately
  tested.
- AP-AC-03/AP-AC-04 stay "not tested on hardware" until that run.
