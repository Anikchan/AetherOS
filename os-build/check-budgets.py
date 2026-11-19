#!/usr/bin/env python3
"""check-budgets.py — P0-T11: compare a metrics file against os-build/budgets.toml.

Metrics file format (fixed text, D6 section 4; one `key = value` per line,
`#` starts a comment):

    image_size_mb = 412
    idle_ram_mb = 231

Rules:
- Every budget key found in the metrics file is compared against its limit.
  Over the limit -> exit 1 (build fails); over the target but within the limit
  -> warning.
- Budget keys missing from the metrics file are skipped with a warning, so the
  skeleton build passes until real metrics exist.

Usage: check-budgets.py [metrics-file]   (default: os-build/metrics/sample.txt)
"""

from __future__ import annotations

import sys
try:
    import tomllib
except ModuleNotFoundError:
    try:
        import tomli as tomllib
    except ModuleNotFoundError:
        # Minimal TOML parser for budgets.toml (Python 3.10 fallback)
        class _TomlShim:
            @staticmethod
            def loads(text):
                result = {}
                section = None
                for line in text.splitlines():
                    line = line.strip()
                    if not line or line.startswith("#"):
                        continue
                    if line.startswith("[") and line.endswith("]"):
                        section = line[1:-1]
                        parts = section.split(".")
                        cur = result
                        for p in parts:
                            cur = cur.setdefault(p, {})
                        continue
                    if "=" in line:
                        key, _, val = line.partition("=")
                        key = key.strip()
                        val = val.strip()
                        # Parse inline table { limit = 150 } or { limit = 512, target = 400 }
                        if val.startswith("{"):
                            inner = val.strip("{}").strip()
                            d = {}
                            for pair in inner.split(","):
                                pair = pair.strip()
                                if "=" in pair:
                                    k, _, v = pair.partition("=")
                                    v = v.strip()
                                    try:
                                        d[k.strip()] = float(v) if "." in v else int(v)
                                    except ValueError:
                                        d[k.strip()] = v
                            if section:
                                parts = section.split(".")
                                cur = result
                                for p in parts:
                                    cur = cur.setdefault(p, {})
                                cur[key] = d
                        else:
                            try:
                                v = float(val) if "." in val else int(val)
                            except ValueError:
                                v = val
                            if section:
                                parts = section.split(".")
                                cur = result
                                for p in parts:
                                    cur = cur.setdefault(p, {})
                                cur[key] = v
                return result
        tomllib = _TomlShim()
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUDGETS = ROOT / "os-build" / "budgets.toml"


def load_metrics(path: Path) -> dict[str, float]:
    metrics: dict[str, float] = {}
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line or "=" not in line:
            continue
        key, _, value = line.partition("=")
        try:
            metrics[key.strip()] = float(value.strip())
        except ValueError:
            print(f"ERROR: cannot parse metric line: {raw}")
            sys.exit(2)
    return metrics


def main() -> int:
    metrics_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "os-build" / "metrics" / "sample.txt"
    budgets = tomllib.loads(BUDGETS.read_text())
    metrics = load_metrics(metrics_path)

    failures: list[str] = []
    warnings: list[str] = []
    checked = 0
    for group in budgets["budgets"].values():
        for key, spec in group.items():
            if key not in metrics:
                warnings.append(f"{key}: not measured yet (skipped)")
                continue
            checked += 1
            value = metrics[key]
            if value > spec["limit"]:
                failures.append(f"{key}: {value} > limit {spec['limit']}")
            elif "target" in spec and value > spec["target"]:
                warnings.append(f"{key}: {value} > target {spec['target']} (within limit)")

    for warning in warnings:
        print(f"WARN: {warning}")
    for failure in failures:
        print(f"FAIL: {failure}")
    print(f"Budgets: {checked} checked, {len(warnings)} skipped/warned, {len(failures)} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
