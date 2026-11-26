# D6 — P7-T07 Settings → Features (optional feature manager UI)

- Date: 2026-11-26
- Session: S3
- Task: P7-T07 UI integration, output Settings → Features, gates OPT-AC-01..05
- API owner: S4 (`config/src/features.rs` `FeatureManager`), used read-only

## What was verified

| Check | Command | Result |
| --- | --- | --- |
| Shell tests | `cargo test -p aether-shell` | 79 lib + 3 integration tests pass (includes 5 `features` unit tests and `tests/features.rs`) |
| Surface render | `aether-shell --render` | `shell-features.png` (1440×900) shows packs A–G with cost, status, Install, budget bar and the budget-refusal note |

## Evidence

- Sidebar: `Category::Features` at index 10 (`FEATURES_INDEX`), `Download`
  icon, 12 categories; the generic schema-row list is hidden on that page.
- Rows: 7 packs in catalog order; disk/idle CPU/memory come from the catalog
  even when not installed (the manager reports 0 MB for uninstalled packs), so
  cost is visible before install (OPT-AC-02).
- Install D → status "Installed", success message, `features/features.toml`
  written under the settings root (same path as `aetherctl features`), usage
  label becomes `1 pack installed · 0.5% of 1.0% idle CPU · 30 MB of 256 MB ·
  20 MB disk`.
- Disable D → installed stays true, enabled false, status "Disabled"
  (OPT-AC-05).
- Remove D → row back to "Not installed", message "Removed Advanced
  monitoring", state file empty.
- Over-budget install refused in the wrapper with the manager's reason
  (`idle budget would be exceeded …`), failed flag set, pack not installed
  (OPT-AC-04).
- Unknown pack and remove-before-install errors are surfaced in the page.

## Not verified (honest status)

- Pack payload install/removal is delegated to the package manager (S4); this
  task only wires the manager's state and the cost/budget UI.
