#!/usr/bin/env python3
"""Differential SHACL Core conformance of the write path (aegis-1c8q).

Every W3C SHACL Core case is validated whole (batch) and written through the
real /knot handler in several splits (the adapter's --write-path mode). A
(case, mode) pair DIVERGES when that mode's verdict differs from the W3C
expected verdict while the batch verdict agrees with it.

Gate (sattler's ruling, a ratchet): the committed ledger lists today's known
divergences. A run FAILS on any divergence not in the ledger (new), any ledger
entry that no longer diverges (stale: remove it in the same change), any batch
disagreement with the W3C verdict, an inventory change, or a missing positive
control. The control is the adapter's deliberately weak payload-only mode,
which must diverge on the cross-write cardinality case; without it the run is
UNKNOWN, never clean.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import shacl_core  # noqa: E402

SCHEMA = "quipu-shacl-write-path/v1"
CONTROL_CASE = "core/property/maxCount-001#maxCount-001"
CONTROL_MODE = "control_payload_only"
EXPECTED_CASES = 98
COMPONENT = re.compile(r"\bsh:(class|datatype|nodeKind|minCount|maxCount|minExclusive|minInclusive|"
                       r"maxExclusive|maxInclusive|minLength|maxLength|pattern|languageIn|uniqueLang|"
                       r"equals|disjoint|lessThanOrEquals|lessThan|not|and|or|xone|node|property|"
                       r"qualifiedValueShape|closed|hasValue|in)\b")


def components(case) -> list[str]:
    return sorted(set(COMPONENT.findall(case.shapes.read_text(errors="replace"))))


def run_case(case, adapter: Path) -> dict:
    started = time.perf_counter()
    proc = subprocess.run(
        [str(adapter), "--write-path", "--shapes", str(case.shapes), "--data", str(case.data),
         "--base", case.data.resolve().as_uri()],
        capture_output=True, text=True, timeout=300)
    if proc.returncode != 0:
        return {"status": "error", "reason": proc.stderr.strip()[-400:]}
    out = json.loads(proc.stdout)
    expected = case.expected_conforms
    batch = out["batch"].get("conforms")
    modes = {name: mode["conforms"] for name, mode in out["modes"].items()}
    errors = sorted({w["error"] for m in out["modes"].values() for w in m["writes"] if "error" in w})
    if batch is None:
        status = "batch_error"
    elif batch != expected:
        status = "batch_mismatch"
    elif all(verdict == expected for verdict in modes.values()):
        status = "agrees"
    else:
        status = "diverges"
    return {"status": status, "expected_conforms": expected, "batch_conforms": batch,
            "modes": modes, "control_conforms": out["control_payload_only"]["conforms"],
            "write_errors": errors[:3], "triples": out["triples"],
            "duration_ms": round((time.perf_counter() - started) * 1000, 1)}


def divergences(results: list[dict]) -> list[list[str]]:
    """Every (case, mode) whose verdict differs from the W3C expected verdict."""
    out = []
    for row in results:
        if row.get("status") not in ("agrees", "diverges"):
            continue
        for mode, verdict in sorted(row["modes"].items()):
            if verdict != row["expected_conforms"]:
                out.append([row["id"], mode])
    return sorted(out)


def control_fired(results: list[dict]) -> bool:
    row = next((r for r in results if r["id"] == CONTROL_CASE), None)
    return bool(row and row.get("expected_conforms") is False and row.get("control_conforms") is True)


def gate(results: list[dict], ledger: dict | None) -> list[str]:
    """Reasons this run fails; empty means PASS against the ledger."""
    failures = []
    if len(results) != EXPECTED_CASES:
        failures.append(f"inventory: {len(results)} Core cases, expected {EXPECTED_CASES}")
    if not control_fired(results):
        failures.append(f"UNKNOWN: positive control {CONTROL_MODE} did not diverge on {CONTROL_CASE}")
    for row in results:
        if row.get("status") not in ("agrees", "diverges"):
            failures.append(f"{row['id']}: {row.get('status')} {row.get('reason', '')}".strip())
    if ledger is None:
        return failures
    current = {tuple(d) for d in divergences(results)}
    known = {tuple(d) for d in ledger.get("divergences", [])}
    failures += [f"NEW divergence: {case} under {mode}" for case, mode in sorted(current - known)]
    failures += [f"STALE ledger entry (no longer diverges; remove it): {case} under {mode}"
                 for case, mode in sorted(known - current)]
    return failures


def summary(results: list[dict]) -> dict:
    by_component = defaultdict(Counter)
    for row in results:
        for comp in row.get("components") or ["(none)"]:
            by_component[comp][row["status"]] += 1
    return {
        "cases": len(results),
        "status": dict(Counter(r["status"] for r in results)),
        "diverging_modes": dict(Counter(mode for _, mode in divergences(results))),
        "by_component": {k: dict(v) for k, v in sorted(by_component.items())},
        "control_fired": control_fired(results),
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", type=Path, required=True)
    parser.add_argument("--quipu", type=Path, required=True, help="quipu-shacl-conformance binary")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--ledger", type=Path, help="committed ledger to gate against")
    parser.add_argument("--write-ledger", type=Path, help="write a new ledger from this run")
    parser.add_argument("--suite-revision", default=shacl_core.PINNED_SUITE_REVISION)
    args = parser.parse_args(argv)
    checkout = args.suite.resolve()
    while checkout != checkout.parent and not (checkout / ".git").exists():
        checkout = checkout.parent
    revision = shacl_core.git_output(checkout, "rev-parse", "HEAD")
    if revision != args.suite_revision or shacl_core.git_output(checkout, "status", "--porcelain"):
        parser.error(f"suite must be clean at {args.suite_revision}; got {revision}")
    cases = [c for c in shacl_core.discover_cases(args.suite) if c.category != "shacl-sparql"]
    results = []
    for case in cases:
        row = {"id": case.identifier, "category": case.category, "components": components(case)}
        row.update(run_case(case, args.quipu))
        results.append(row)
    report = {"schema": SCHEMA, "suite_revision": revision, "summary": summary(results),
              "divergences": divergences(results), "results": results}
    args.output.write_text(json.dumps(report, indent=1) + "\n")
    print(json.dumps(report["summary"], indent=1))
    if args.write_ledger:
        ledger = {"schema": SCHEMA, "suite_revision": revision,
                  "note": "Known write-path divergences. This list may only shrink (aegis-1c8q ratchet).",
                  "cases": len(results),
                  "by_component": report["summary"]["by_component"],
                  "divergences": report["divergences"]}
        args.write_ledger.write_text(json.dumps(ledger, indent=1) + "\n")
    ledger = json.loads(args.ledger.read_text()) if args.ledger else None
    failures = gate(results, ledger)
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
