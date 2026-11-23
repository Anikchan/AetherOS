# P6 — App platform: packages, sandbox, runner, store, compatibility

- Date: 2026-11-23
- Session: S4 (v2 plan), autonomous run
- Scope: `apps/pkg/`, `apps/sandbox/`, `apps/runner/`, `apps/store/`
- Design: D8 sections 2–5, 04 §7

## Verified

### Tests

| Crate | Command | Result |
|---|---|---|
| `aether-pkg` | `cargo test -p aether-pkg` | **15 pass** |
| `aether-sandbox` | `cargo test -p aether-sandbox` | **15 pass** |
| `aether-runner` | `cargo test -p aether-runner` | **12 pass** |
| `aether-store` | `cargo test -p aether-store` | **5 pass** |

Total: **47 tests, 0 failed**.

Coverage highlights:

- **P6-T01 manifest/format:** TOML round trip; checksum tampering detected;
  entry cannot escape the bundle (`../`); invalid app IDs rejected; missing
  permissions listed; AppImage manifest inferred.
- **P6-T02 install/run:** native bundle install → files + data folder +
  registry; uninstall removes app/data/registry; checksum failure cleans
  staging and leaves no registry entry; upgrade replaces the old version;
  AppImage copied with executable bit; registry persists; arbitrary directory
  rejected.
- **P6-T03 AppImage:** inferred manifest, install, executable bit, permission
  request for network/picked files.
- **P6-T04 sandbox:** permission acceptance/denial persisted across restarts;
  profiles keep network off unless granted; only the app data folder is
  writable; picked files read-only; denied home access stays denied;
  camera/microphone device paths only when granted; bwrap command contains
  `--unshare-user/pid/ipc/uts` and conditional `--unshare-net`; missing bwrap
  or user namespaces refuses launch (never unsandboxed).
- **P6-T05/T06 Windows:** engine registry finds `wine-<version>` directories
  (newest first) and PATH fallback; plan uses the app prefix (`WINEPREFIX`),
  adds the prefix to writable paths, and the prefix manager creates/lists/
  removes prefixes; Windows installer payload is staged without copying the
  repository (bug found and fixed during the test run).
- **P6-T07 samples:** `aether-samples` generates native, cross-platform and
  AppImage samples with correct checksums; inspect/verify/install/plan all work
  (see CLI transcript below).
- **P6-T08 macOS detection:** `.dmg`/`.pkg`/`.app` produce
  `MacosPackage` and `Unsupported` with three alternatives.
- **P6-T09 store:** repository scan, search, install, update detection
  (numeric version compare), update, remove, index round trip.
- **P6-T10 compatibility DB:** exactly 40 slots from 04 §7, all `Untested`;
  gate opens only at 30 Tier A/B; result entry persists and the report renders.

### End-to-end CLI transcript

```text
$ aether-samples --out /tmp/aether-samples
SAMPLES_OK native=org.aether.samples.Native cross=org.aether.samples.CrossPlatform appimage=.../AetherSample.AppImage

$ aether-pkg verify /tmp/aether-samples/native
VERIFY_OK id=org.aether.samples.Native version=1.0.0 files=2 bytes=115

$ aether-pkg install /tmp/aether-samples/native --accept-permissions --root /tmp/aether-store-test
INSTALLED org.aether.samples.Native (data folder: .../apps/org.aether.samples.Native/data)

$ aether-runner plan org.aether.samples.Native --root /tmp/aether-store-test
...
  Network: blocked
  Filesystem: read-only system directories, writable app data folder

$ aether-store --repo /tmp/aether-samples list
org.aether.samples.Native    Aether Native Sample        1.0.0  native-bundle
org.aether.samples.CrossPlatform  Cross-platform Sample  1.0.0  cross-platform-bundle

$ aether-sandbox doctor
{"bwrap": null, ..., "usable": false}
ENFORCER_UNAVAILABLE ... apps will refuse to launch unsandboxed (D8 failure mode)
```

## Task status

| Task | Status | Notes |
|---|---|---|
| P6-T01 Package format + manifest | Done | spec + inspector; sample bundles verified |
| P6-T02 Native bundle install & run | Done (core) | install/uninstall/registry; launch requires bwrap, enforced |
| P6-T03 AppImage runner | Done (core) | inferred manifest + install + exec bit; plan/launch |
| P6-T04 Sandbox + permission broker | Done (core) | namespaces via bwrap; Landlock detection; denied stays denied |
| P6-T05 Windows engine packaging/prefix manager | Partial | engine discovery + prefix manager; engine *packaging* (download/Buildroot) pending |
| P6-T06 Windows installer flow + uninstall | Partial | install keeps payload + creates prefix; full wine installer run needs an engine |
| P6-T07 Cross-platform bundles + samples | Done | `aether-samples`, 3 bundles verified |
| P6-T08 macOS detection | Done | explanation + alternatives |
| P6-T09 Software app (basic) | Done (core) | repository scan/search/install/update/remove; UI is S3 |
| P6-T10 Compatibility DB + 40-app test | Partial | 40-slot DB, gate math, report; real results need the Dell + Windows engine |

## Honest limits

- Actual sandboxed execution was not run on this host: macOS has no bwrap or
  user namespaces, and the daemon correctly refuses to launch unsandboxed. The
  command construction is tested; the integration run belongs to AetherOS.
- Landlock is detected but enforcement uses namespaces (bwrap) in this
  version; direct Landlock syscalls are a follow-up.
- Windows engine packaging (downloading/licensing Wine, Pack B) is not done;
  only discovery, prefix management and launch planning are.
- The 40-app compatibility results are all `Untested`; the gate is open and
  must not be claimed until measured.
- No store network code: the index points at local bundle directories.

## Reproduce

```bash
cargo test -p aether-pkg -p aether-sandbox -p aether-runner -p aether-store
cargo run -p aether-runner --bin aether-samples -- --out /tmp/aether-samples
cargo run -p aether-pkg -- verify /tmp/aether-samples/native
cargo run -p aether-sandbox -- doctor
cargo run -p aether-runner --bin aether-runner -- compat plan --db /tmp/compat.json
```
