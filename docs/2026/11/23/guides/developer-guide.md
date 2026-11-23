# AetherOS — Developer Guide (draft, 2026-11-23)

## 1. Repository map

| Path | What it is |
|---|---|
| `docs/` | Blueprint, roadmap (tracker §4), design docs D1–D10, feature docs F1–F15, evidence logs |
| `compositor/` | Smithay compositor (S2) |
| `shell/`, `ui/`, `apps/text…browser` | Desktop shell and default apps (S3) |
| `lifecycled/` | Process lifecycle daemon (P5) |
| `powerd/` | Power policy, telemetry, analytics, screen time (P5) |
| `apps/taskmanager/` | Activity Manager core (P5-T07) |
| `apps/pkg`, `apps/sandbox`, `apps/runner`, `apps/store` | App platform (P6) |
| `apps/storage-analyzer/` | Pack G storage analyzer |
| `config/` | Layered config, theme, keybindings, hooks, feature packs, `aetherctl` (P7) |
| `installer/` | Installer detection/planner/pre-checks/uninstall (P8) |
| `os-build/` | Kernel fragment, Buildroot defconfig, CI scripts, budgets, metrics |

## 2. Build and test

```bash
cargo build --workspace
cargo test --workspace
cargo test -p aether-lifecycled -p aether-powerd -p aether-taskmanager
cargo test -p aether-pkg -p aether-sandbox -p aether-runner -p aether-store
cargo test -p aether-config -p aether-installer -p aether-storage-analyzer
```

Local Linux type-check (macOS host): `os-build/scripts/type-check.sh`.
CI: `.github/workflows/ci.yml`; budget gate: `os-build/check-budgets.py`.

## 3. Contracts first

- Process lifecycle: `docs/2026/11/16/os-project/design/process-manager-design.md`
  (D3). Policy has no I/O; mechanisms are traits.
- Power: `design/power-management-design.md` (D5). Same split.
- App platform: `docs/2026/11/17/os-addendum-1/D8-app-platform-and-ai-runtime-design.md`.
- Config: `docs/2026/11/16/os-project/features/04-customization.md` (F4); every
  setting is a key in `config/src/schema.rs`.
- Gates/evidence: D6 framework and `04-test-and-acceptance.md`.

When changing behaviour, update the contract doc first, then the tests, then the
code. If a gate cannot be measured on the current host, record "not measured".

## 4. Adding a setting

1. Add a `KeySpec` to `config/src/schema.rs` (key, file, TOML key, kind,
   default, description, range).
2. `cargo test -p aether-config` — the schema tests check defaults, file
   mapping and settings-screen coverage.
3. Consume it with `ConfigStore::get_*`; the settings UI maps one screen row to
   one key.

## 5. Adding a package kind

1. Extend `PackageKind` in `apps/pkg/src/manifest.rs`.
2. Teach `Installer::inspect` to infer a manifest and `install` to stage the
   payload.
3. Add an `EngineId` in `apps/runner` and a plan/launch path.
4. Add a sandbox profile only from granted permissions, never from the manifest.
5. Add tests for inspect, install, launch plan and uninstall.

## 6. Sandbox rules (non-negotiable)

- Network off unless granted; only the app data folder writable; picked files
  read-only.
- If `bwrap`/user namespaces are missing, launch fails. Never run unsandboxed.
- Denied permissions persist as denied.

## 7. Evidence and honesty

- Every measurable task writes a D6 log under `docs/<year>/<month>/<day>/results/`.
- Results from macOS/VMs do not count for battery, speed, hang, or hardware
  gates (04 §10). Say "not measured" instead of estimating.
- Tracker §4 is updated by S1 only; parallel sessions update their own plan and
  `os-build` budgets without touching other territories.

## 8. Session territories (v2)

See `docs/2026/11/22/parallel-plan-v2.md`. S4 owns process/power, app platform,
installer, config, docs/design, scripts, metrics, budgets. Never edit another
session's files without coordination.

## 9. Release checks

- Checksums: `os-build/scripts/image-checksum.sh create|verify IMAGE`.
- Firewall: `os-build/security/aether-firewall.nft` (`nft -c -f`).
- Feature packs: `aetherctl features list` and idle-budget check.
- Pending: live image (P8-T01), Secure Boot (P8-T05), fuzzing/security review
  (P9-T06), 30-day dogfood (P10-T04), public release (P10-T05).
