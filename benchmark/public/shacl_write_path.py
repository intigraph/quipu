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
# Every split the adapter must exercise (src/bin/quipu_shacl_conformance.rs::splits).
# A case missing one was never measured under it, so it gets no verdict (franklin, #707).
EXPECTED_MODES = frozenset({"single", "subject_halves", "subject_halves_reversed", "per_subject",
                            "per_subject_reversed", "per_triple", "per_triple_reversed"})
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
    row = classify(json.loads(proc.stdout), case.expected_conforms)
    row["duration_ms"] = round((time.perf_counter() - started) * 1000, 1)
    return row


def unjudged(write: dict) -> bool:
    """A write the gate never judged: no verdict, or an error that is not a SHACL rejection."""
    return write.get("accepted") is None or ("error" in write and write.get("rejection") != "shacl")


def classify(out: dict, expected: bool) -> dict:
    """One case's status from the adapter's --write-path output.

    Only a SHACL judgement counts as a verdict. A split with any unjudged write
    has no verdict and the case is a write_error, which fails the gate whatever
    the ledger says; otherwise a store or validator outage would score as a
    correct rejection of every invalid case (aegis-1c8q review F1).
    """
    batch = out["batch"].get("conforms")
    missing, extra = mode_inventory(out["modes"])
    empty = sorted(name for name, mode in out["modes"].items()
                   if out.get("triples", 0) > 0 and not mode.get("writes"))
    if missing or extra or empty:
        return {"status": "mode_inventory", "expected_conforms": expected, "batch_conforms": batch,
                "modes": {}, "control_conforms": out["control_payload_only"]["conforms"],
                "write_errors": [], "triples": out.get("triples"),
                "reason": f"missing modes {missing}, unexpected modes {extra}, "
                          f"no writes for nonempty data in {empty}"}
    modes = {name: (None if any(unjudged(w) for w in mode["writes"]) else mode["conforms"])
             for name, mode in out["modes"].items()}
    errors = sorted({w["error"] for m in out["modes"].values() for w in m["writes"] if unjudged(w)})
    reason = ""
    if batch is None:
        status = "batch_error"
    elif batch != expected:
        status = "batch_mismatch"
    elif any(verdict is None for verdict in modes.values()):
        status = "write_error"
        reason = "; ".join(f"{name}: no SHACL verdict" for name, v in sorted(modes.items()) if v is None)
        reason += f" ({'; '.join(errors[:3]) or 'no error text'})"
    elif all(verdict == expected for verdict in modes.values()):
        status = "agrees"
    else:
        status = "diverges"
    row = {"status": status, "expected_conforms": expected, "batch_conforms": batch,
           "modes": modes, "control_conforms": out["control_payload_only"]["conforms"],
           "write_errors": errors[:3], "triples": out["triples"]}
    if reason:
        row["reason"] = reason
    return row


def divergences(results: list[dict]) -> list[list[str]]:
    """Every judged (case, mode) whose verdict differs from the W3C expected verdict.

    A write_error row still contributes its JUDGED modes, so an unjudged mode
    can never hide a divergence elsewhere in the same case.
    """
    out = []
    for row in results:
        if row.get("status") not in ("agrees", "diverges", "write_error"):
            continue
        for mode, verdict in sorted(row["modes"].items()):
            if verdict is not None and verdict != row["expected_conforms"]:
                out.append([row["id"], mode])
    return sorted(out)


def mode_inventory(modes: dict) -> tuple[list[str], list[str]]:
    """(missing, unexpected) split modes against EXPECTED_MODES."""
    return sorted(EXPECTED_MODES - set(modes)), sorted(set(modes) - EXPECTED_MODES)


def unjudged_pairs(results: list[dict]) -> list[list[str]]:
    """Every (case, mode, error) the write gate never judged (aegis-1c8q option A).

    Bound to the exact error text: a known CannotVerify that changes its reason
    is a different fact, so it fails the gate as one new entry plus one stale one.
    """
    out = []
    for row in results:
        if row.get("status") != "write_error":
            continue
        error = "; ".join(row.get("write_errors") or []) or "no error text"
        out += [[row["id"], mode, error] for mode, verdict in sorted(row["modes"].items())
                if verdict is None]
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
    # Re-checked here, not only in classify: a row that never came through
    # classify (or a hand-edited result) must not pass with a split silently gone.
    for row in results:
        if row.get("status") in ("agrees", "diverges", "write_error"):
            missing, extra = mode_inventory(row.get("modes", {}))
            if missing or extra:
                failures.append(f"{row['id']}: mode inventory: missing {missing}, unexpected {extra}")
    known_unjudged = {tuple(u) for u in (ledger or {}).get("unjudged", [])}
    current_unjudged = {tuple(u) for u in unjudged_pairs(results)}
    for row in results:
        if row.get("status") in ("agrees", "diverges"):
            continue
        # A write_error passes only when EVERY unjudged pair is a known, owned
        # ledger entry with the same error text; anything else is reported as before.
        mine = {u for u in current_unjudged if u[0] == row["id"]}
        if row.get("status") == "write_error" and mine and mine <= known_unjudged:
            continue
        failures.append(f"{row['id']}: {row.get('status')} {row.get('reason', '')}".strip())
    if ledger is None:
        return failures
    failures += [f"STALE unjudged entry (now judged or its error changed; remove it): {case} under {mode}: {error}"
                 for case, mode, error in sorted(known_unjudged - current_unjudged)]
    current = {tuple(d) for d in divergences(results)}
    known = {tuple(d) for d in ledger.get("divergences", [])}
    failures += [f"NEW divergence: {case} under {mode}" for case, mode in sorted(current - known)]
    failures += [f"STALE ledger entry (no longer diverges; remove it): {case} under {mode}"
                 for case, mode in sorted(known - current)]
    # sattler's ruling: every known divergence names its class and owning fix item.
    # Unjudged cases are owned the same way.
    owners = ledger.get("owners", {})
    ledger_cases = {case for case, _ in known} | {case for case, _, _ in known_unjudged}
    failures += [f"UNOWNED ledger case (needs class and item): {case}"
                 for case in sorted(ledger_cases)
                 if not (owners.get(case, {}).get("class") and owners.get(case, {}).get("item"))]
    failures += [f"STALE owner entry (case no longer in the ledger; remove it): {case}"
                 for case in sorted(set(owners) - ledger_cases)]
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
              "divergences": divergences(results), "unjudged": unjudged_pairs(results), "results": results}
    args.output.write_text(json.dumps(report, indent=1) + "\n")
    print(json.dumps(report["summary"], indent=1))
    if args.write_ledger:
        previous = json.loads(args.write_ledger.read_text()) if args.write_ledger.exists() else {}
        diverging = ({case for case, _ in report["divergences"]}
                     | {case for case, _, _ in report["unjudged"]})
        ledger = {"schema": SCHEMA, "suite_revision": revision,
                  "note": "Known write-path divergences and unjudged writes. Both lists may only shrink "
                          "(aegis-1c8q ratchet; unjudged = sattler option A).",
                  "cases": len(results),
                  "by_component": report["summary"]["by_component"],
                  "owners": {case: owner for case, owner in sorted(previous.get("owners", {}).items())
                             if case in diverging},
                  "divergences": report["divergences"],
                  "unjudged": report["unjudged"]}
        args.write_ledger.write_text(json.dumps(ledger, indent=1) + "\n")
    ledger = json.loads(args.ledger.read_text()) if args.ledger else None
    failures = gate(results, ledger)
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
