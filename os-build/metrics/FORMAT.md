# Metrics Format (P0-T11)

Fixed text format for the verification framework (addendum 1 D6 section 4,
addendum 2 D10). The owner must be able to read it without tools.

## Format

One `key = value` line per metric. `#` starts a comment. Keys are the budget
keys in `os-build/budgets.toml` plus free-form event fields.

```
# 2026-11-17T10:00:00Z boot
stage = firmware 0.9
stage = bootloader 0.3
stage = kernel 1.2
boot_median_s = 8.4
```

## Where metrics live at runtime

- System services: `/var/log/aether/metrics/` (D6 writes the old `nexa`
  placeholder; renamed to `aether` per ADR-012).
- User services: `~/.local/state/aether/metrics/`.
- Ring buffer capped at 50 MB, oldest removed first. Nothing leaves the
  machine; export is manual via `aectl metrics export` (P7-T03).

## Checks in CI

`os-build/check-budgets.py <metrics-file>` compares the metrics against
`os-build/budgets.toml` every build and fails the build when a limit is
exceeded (G-RES*, G-SPD*, G-BAT2, G-TM1).
