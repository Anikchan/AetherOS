# App platform (P6)

| Crate | Role | Key files |
|---|---|---|
| `aether-pkg` | manifest, bundle format, inspector, atomic installer, registry | `src/manifest.rs`, `src/installer.rs`, bin `aether-pkg` |
| `aether-sandbox` | permission broker, profiles, bwrap enforcement, capabilities | `src/permissions.rs`, `src/profile.rs`, `src/enforce.rs`, bin `aether-sandbox` |
| `aether-runner` | engine registry, sandboxed launch, Windows prefixes, compatibility DB, samples | `src/lib.rs`, `src/compat.rs`, bins `aether-runner`, `aether-samples` |
| `aether-store` | repository index, search, install/update/remove | `src/lib.rs`, bin `aether-store` |

## Bundle format (P6-T01)

A native bundle is a directory with `aether-manifest.toml`:

```toml
id = "org.aether.samples.Native"
name = "Aether Native Sample"
version = "1.0.0"
kind = "native-bundle"          # or appimage, windows-installer, cross-platform-bundle
entry = "bin/native-sample"
installed_bytes = 115
permissions = [{ kind = "files", scope = "picked-by-user" }]

[[files]]
path = "bin/native-sample"
sha256 = "…"
size = 78
```

`aether-pkg inspect` never writes. `install` stages, verifies every checksum,
moves into `~/Applications/<id>`, writes `install-registry.json`, and rolls
back staging on any failure. AppImages get an inferred manifest and are
installed as one executable file.

## Sandbox rules (P6-T04)

- Network is blocked unless `network` was granted by the user.
- Only the app data folder is writable.
- Files must come from the file picker (read-only) or an explicit grant.
- Camera/microphone device paths appear only after a grant.
- Enforcement uses bubblewrap namespaces. If `bwrap` or user namespaces are
  missing, launch fails with an explanation; the app is never started
  unsandboxed (D8 §7).
- Denied permissions are persisted as denied and never silently re-prompted.

## Commands

```bash
aether-pkg inspect BUNDLE
aether-pkg verify BUNDLE
aether-pkg install BUNDLE --accept-permissions   # or interactive prompt
aether-pkg list
aether-pkg uninstall APP-ID --yes

aether-sandbox doctor
aether-sandbox grant APP-ID network
aether-sandbox revoke APP-ID camera
aether-sandbox profile --manifest BUNDLE --data DIR --granted network

aether-runner list
aether-runner plan APP-ID
aether-runner launch APP-ID
aether-runner engines
aether-runner prefixes list
aether-runner compat plan|set|report

aether-store --repo DIR list|search QUERY|install APP-ID|updates|update-all|remove APP-ID
aether-samples --out DIR
```

## Status and limits

- Sandboxed execution is verified by construction and tests on macOS, but the
  real bwrap run must happen on AetherOS.
- Landlock is detected; enforcement is namespace-based in this version.
- Windows engine packaging (Pack B) is not included: only engine discovery,
  prefix management and launch planning.
- Compatibility results are all `Untested` until measured on the Dell.
- The Software app UI belongs to the shell (S3); this crate is its data layer.
