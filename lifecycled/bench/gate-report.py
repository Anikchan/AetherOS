#!/usr/bin/env python3
"""P5-T08 — gate report generator for G-PM1 (50-app benchmark) and G-PM5
(72-hour soak), D6 verification framework.

Reads the JSON emitted by `aether-bench --json` plus optional soak samples and
emits a D6-format Markdown verdict. It never turns a model-host result into a
PASS: hardware gates are PENDING until the reference machines run the harness
(04 §10, `docs/2026/11/17/os-addendum-1/D6-verification-framework.md`).

Usage:
  gate-report.py --bench-json FILE [--gate all|G-PM1|G-PM5]
                 [--host-label TEXT] [--reference-hardware]
                 [--soak-hours H] [--soak-samples FILE]
                 [--b2-restore-ms MS] [--b6-share PERCENT]
                 [--out FILE] [--json FILE]

Exit codes: 0 = PASS or PENDING (model host), 1 = FAIL, 2 = evidence missing
on the reference hardware (INCOMPLETE).
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from typing import Any

PASS = "PASS"
FAIL = "FAIL"
PENDING = "PENDING"
INCOMPLETE = "INCOMPLETE"

# 04 §4.3 pass criteria.
B1_MAX_INACTIVE_SHARE = 0.25
B2_MAX_RESTORE_MS = 500.0
B6_MAX_FULL_PRESSURE_SHARE = 5.0
GPM1_APPS = 50
GPM5_MIN_HOURS = 72.0
# "Memory returns to baseline after each cycle": allow 5% of MemAvailable drift.
GPM5_MAX_BASELINE_DRIFT_PERCENT = 5.0


@dataclass
class Gate:
    id: str
    title: str
    status: str = PENDING
    checks: list[tuple[str, str, str]] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)


def load_bench(path: str) -> dict[str, Any]:
    with open(path, "r", encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, dict):
        raise ValueError("bench JSON must be an object")
    cycles = data.get("cycles_detail")
    if not isinstance(cycles, list) or not cycles:
        raise ValueError("bench JSON has no cycles_detail")
    return data


def load_soak_samples(path: str) -> list[dict[str, float]]:
    """TSV: cycle<TAB>mem_available_kb_before<TAB>mem_available_kb_after[<TAB>exit].

    `exit` is the per-cycle `aether-bench` status; older files without the fourth
    column are treated as successful cycles.
    """
    samples: list[dict[str, float]] = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) < 3:
                continue
            try:
                samples.append(
                    {
                        "cycle": float(parts[0]),
                        "before_kb": float(parts[1]),
                        "after_kb": float(parts[2]),
                        "exit": float(parts[3]) if len(parts) > 3 else 0.0,
                    }
                )
            except ValueError:
                continue
    return samples


def failed_cycles(samples: list[dict[str, float]]) -> int:
    """Cycles whose `aether-bench` run exited non-zero."""
    return sum(1 for sample in samples if sample.get("exit", 0.0) != 0.0)



def baseline_drift_percent(samples: list[dict[str, float]]) -> float | None:
    """Largest per-cycle MemAvailable drop relative to the cycle baseline."""
    worst: float | None = None
    for sample in samples:
        before = sample["before_kb"]
        if before <= 0:
            continue
        drift = max(0.0, (before - sample["after_kb"]) / before * 100.0)
        worst = drift if worst is None else max(worst, drift)
    return worst


def summarize_cycles(cycles: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "cycles": len(cycles),
        "all_b1": all(bool(cycle.get("b1")) for cycle in cycles),
        "all_b3": all(bool(cycle.get("b3")) for cycle in cycles),
        "all_b4": all(bool(cycle.get("b4")) for cycle in cycles),
        "all_b5": all(bool(cycle.get("b5")) for cycle in cycles),
        "foreground_kills": sum(int(cycle.get("foreground_kills", 0)) for cycle in cycles),
        "max_inactive_share": max(float(cycle.get("inactive_share", 0.0)) for cycle in cycles),
    }


def evaluate(
    bench: dict[str, Any],
    reference: bool,
    soak_hours: float | None,
    soak_samples: list[dict[str, float]],
    b2_restore_ms: float | None,
    b6_share: float | None,
    gates: str,
) -> list[Gate]:
    summary = summarize_cycles(bench["cycles_detail"])
    apps = int(bench.get("apps", 0))
    results: list[Gate] = []

    if gates in ("all", "G-PM1"):
        gate = Gate("G-PM1", "50-app benchmark (B1–B6, 04 §4.3)")
        if apps < GPM1_APPS:
            gate.status = INCOMPLETE
            gate.notes.append(
                f"only {apps} apps per cycle; G-PM1 requires {GPM1_APPS}"
            )
        measured_ok = (
            summary["all_b1"]
            and summary["all_b3"]
            and summary["all_b4"]
            and summary["all_b5"]
        )
        gate.checks.append(
            ("B1 inactive memory ≤ 25%", PASS if summary["all_b1"] else FAIL,
             f"{summary['max_inactive_share'] * 100:.1f}% worst cycle")
        )
        gate.checks.append(
            ("B2 hibernated restore ≤ 500 ms", PASS if b2_restore_ms is not None and b2_restore_ms <= B2_MAX_RESTORE_MS else (FAIL if b2_restore_ms is not None else PENDING),
             "not measured" if b2_restore_ms is None else f"{b2_restore_ms:.0f} ms")
        )
        gate.checks.append(
            ("B3 foreground kills = 0", PASS if summary["all_b3"] else FAIL,
             f"{summary['foreground_kills']} foreground kills")
        )
        gate.checks.append(
            ("B4 audio + download exceptions", PASS if summary["all_b4"] else FAIL,
             "protected apps alive" if summary["all_b4"] else "protected app was suspended or killed")
        )
        gate.checks.append(
            ("B5 killer priority order", PASS if summary["all_b5"] else FAIL,
             "kills follow D3 bands" if summary["all_b5"] else "kill order violated or no kills")
        )
        gate.checks.append(
            ("B6 full-pressure share < 5%", PASS if b6_share is not None and b6_share < B6_MAX_FULL_PRESSURE_SHARE else (FAIL if b6_share is not None else PENDING),
             "not measured" if b6_share is None else f"{b6_share:.1f}%")
        )
        if not measured_ok:
            gate.status = FAIL
        elif reference:
            missing = [name for name, status, _ in gate.checks if status == PENDING]
            if missing:
                gate.status = INCOMPLETE
                gate.notes.append("missing reference-hardware evidence: " + ", ".join(missing))
            else:
                gate.status = PASS
        else:
            gate.status = PENDING
            gate.notes.append(
                "policy-level checks pass on a model host; B2/B6 and real cgroup memory need the 8 GB and 4 GB reference machines"
            )
        results.append(gate)

    if gates in ("all", "G-PM5"):
        gate = Gate("G-PM5", "72-hour soak with 50 apps and random focus switching")
        drift = baseline_drift_percent(soak_samples)
        failures = failed_cycles(soak_samples)
        hours_ok = soak_hours is not None and soak_hours >= GPM5_MIN_HOURS
        gate.checks.append(
            ("No foreground app killed", PASS if summary["all_b3"] else FAIL,
             f"{summary['foreground_kills']} foreground kills")
        )
        gate.checks.append(
            ("Inactive memory ≤ 25% every cycle", PASS if summary["all_b1"] else FAIL,
             f"{summary['max_inactive_share'] * 100:.1f}% worst cycle")
        )
        gate.checks.append(
            ("Memory returns to baseline each cycle", PASS if drift is not None and drift <= GPM5_MAX_BASELINE_DRIFT_PERCENT else (FAIL if drift is not None else PENDING),
             "no soak samples" if drift is None else f"{drift:.1f}% worst MemAvailable drift")
        )
        gate.checks.append(
            ("No failed soak cycles", PASS if soak_samples and failures == 0 else (FAIL if failures else PENDING),
             "no soak samples" if not soak_samples else f"{failures} of {len(soak_samples)} cycles exited non-zero")
        )
        gate.checks.append(
            ("Soak duration ≥ 72 h", PASS if hours_ok else (INCOMPLETE if soak_hours is not None else PENDING),
             "not measured" if soak_hours is None else f"{soak_hours:.2f} h of {summary['cycles']} cycles")
        )
        if not summary["all_b3"] or not summary["all_b1"] or failures:
            gate.status = FAIL
        elif reference:
            missing = [name for name, status, _ in gate.checks if status == PENDING]
            if not hours_ok:
                gate.status = INCOMPLETE
                gate.notes.append("full 72-hour run required for G-PM5")
            elif missing:
                gate.status = INCOMPLETE
                gate.notes.append("missing evidence: " + ", ".join(missing))
            else:
                gate.status = PASS
        else:
            gate.status = PENDING
            gate.notes.append(
                "compressed soak validates the harness only; the 72-hour run needs the reference machine"
            )
        results.append(gate)
    return results


def render_markdown(
    gates: list[Gate],
    bench: dict[str, Any],
    host_label: str,
    reference: bool,
    soak_hours: float | None,
) -> str:
    lines = [
        "# P5-T08 — G-PM1 / G-PM5 gate report",
        "",
        f"- Host: {host_label}",
        f"- Reference hardware: {'yes' if reference else 'no (model host)'}",
        f"- Apps per cycle: {bench.get('apps', '?')}",
        f"- Cycles in this bench file: {len(bench.get('cycles_detail', []))}",
    ]
    if soak_hours is not None:
        lines.append(f"- Soak duration: {soak_hours:.2f} h")
    lines += ["", "## Verdicts", "", "| Gate | Verdict |", "|---|---|"]
    for gate in gates:
        lines.append(f"| {gate.id} — {gate.title} | **{gate.status}** |")
    for gate in gates:
        lines += ["", f"## {gate.id} checks", "", "| Check | Result | Detail |", "|---|---|---|"]
        for name, status, detail in gate.checks:
            lines.append(f"| {name} | {status} | {detail} |")
        for note in gate.notes:
            lines.append(f"\n> {note}")
    lines += [
        "",
        "## Evidence rules",
        "",
        "- PENDING is not a pass. Only a reference-machine run with every check",
        "  measured can turn G-PM1 or G-PM5 into PASS (D6, 04 §10).",
        "- G-PM1 needs the 8 GB and the 4 GB machine; G-PM5 needs one full 72-hour run.",
        "",
    ]
    return "\n".join(lines)


def self_test() -> int:
    bench = {
        "apps": 50,
        "cycles": 2,
        "cycles_detail": [
            {"b1": True, "b3": True, "b4": True, "b5": True, "foreground_kills": 0, "inactive_share": 0.1},
            {"b1": True, "b3": True, "b4": True, "b5": True, "foreground_kills": 0, "inactive_share": 0.2},
        ],
    }
    model = evaluate(bench, False, None, [], None, None, "all")
    assert [gate.status for gate in model] == [PENDING, PENDING], model
    ref_incomplete = evaluate(bench, True, 72.0, [], None, None, "all")
    assert [gate.status for gate in ref_incomplete] == [INCOMPLETE, INCOMPLETE], ref_incomplete
    samples = [{"cycle": 1, "before_kb": 1_000_000, "after_kb": 990_000}]
    ref_pass = evaluate(bench, True, 72.0, samples, 400.0, 2.0, "all")
    assert [gate.status for gate in ref_pass] == [PASS, PASS], ref_pass
    failed_cycle = [{"cycle": 1, "before_kb": 1_000_000, "after_kb": 990_000, "exit": 1.0}]
    assert evaluate(bench, True, 72.0, failed_cycle, 400.0, 2.0, "G-PM5")[0].status == FAIL
    failed = {
        "apps": 50,
        "cycles": 1,
        "cycles_detail": [
            {"b1": False, "b3": False, "b4": True, "b5": True, "foreground_kills": 1, "inactive_share": 0.5}
        ],
    }
    assert all(gate.status == FAIL for gate in evaluate(failed, False, None, [], None, None, "all"))
    assert evaluate(bench, True, 10.0, samples, 400.0, 2.0, "G-PM5")[0].status == INCOMPLETE
    print("gate-report self-test: PASS (8 assertions)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bench-json")
    parser.add_argument("--gate", default="all", choices=["all", "G-PM1", "G-PM5"])
    parser.add_argument("--host-label", default="unspecified")
    parser.add_argument("--reference-hardware", action="store_true")
    parser.add_argument("--soak-hours", type=float)
    parser.add_argument("--soak-samples")
    parser.add_argument("--b2-restore-ms", type=float)
    parser.add_argument("--b6-share", type=float)
    parser.add_argument("--out")
    parser.add_argument("--json")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if not args.bench_json:
        parser.error("--bench-json is required (or use --self-test)")

    try:
        bench = load_bench(args.bench_json)
        samples = load_soak_samples(args.soak_samples) if args.soak_samples else []
    except (OSError, ValueError) as error:
        print(f"gate-report: {error}", file=sys.stderr)
        return 2

    gates = evaluate(
        bench,
        args.reference_hardware,
        args.soak_hours,
        samples,
        args.b2_restore_ms,
        args.b6_share,
        args.gate,
    )
    markdown = render_markdown(
        gates, bench, args.host_label, args.reference_hardware, args.soak_hours
    )
    if args.out:
        with open(args.out, "w", encoding="utf-8") as handle:
            handle.write(markdown)
    else:
        print(markdown)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as handle:
            json.dump(
                {
                    "host": args.host_label,
                    "reference_hardware": args.reference_hardware,
                    "gates": [
                        {"id": gate.id, "status": gate.status, "checks": gate.checks}
                        for gate in gates
                    ],
                },
                handle,
                indent=2,
            )
    statuses = {gate.status for gate in gates}
    if FAIL in statuses:
        return 1
    if INCOMPLETE in statuses:
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
