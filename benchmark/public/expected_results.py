#!/usr/bin/env python3
"""Strict, deterministic W3C expectations; run provenance stays in CI artifacts."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from check_regression import LedgerError, load_rows


def outcome(row: dict) -> str:
    """Reject unknown measurements rather than treating them as known failures."""
    if "status" in row:
        status = row["status"]
        if status not in {"passed", "failed", "unsupported"}:
            raise LedgerError(f"unknown result status: {status!r}")
        return "pass" if status == "passed" else "xfail"
    if type(row.get("passed")) is not bool:
        raise LedgerError("result needs a boolean passed or supported status")
    return "pass" if row["passed"] else "xfail"


def expectations(directory: Path) -> dict:
    """Explicit authoring operation, never called automatically by CI."""
    suites = {}
    for path in sorted(directory.glob("*.json")):
        document = json.loads(path.read_text())
        if "results" not in document:
            continue  # The separately tested SHACL write-path inventory is not a run ledger.
        revision = document.get("suite_revision")
        if not isinstance(revision, str) or len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
            raise LedgerError(f"{path}: suite_revision must be a full pinned SHA")
        rows = load_rows(path)
        suites[path.name] = {
            "suite_revision": revision,
            "cases": [[*key, outcome(row)] for key, row in sorted(rows.items())],
        }
    if not suites:
        raise LedgerError("no suites")
    return {"schema": "quipu-expected-results/v1", "suites": suites}


def check(expected: dict, directory: Path) -> list[str]:
    """Both regressions and unexpected passes require reviewed expectation changes."""
    if expected.get("schema") != "quipu-expected-results/v1" or not isinstance(expected.get("suites"), dict) or not expected["suites"]:
        raise LedgerError("unsupported or empty expected-results document")
    failures = []
    for name, suite in sorted(expected["suites"].items()):
        if Path(name).name != name or not name.endswith(".json"):
            raise LedgerError("suite name must be a JSON basename")
        revision = suite.get("suite_revision") if isinstance(suite, dict) else None
        if not isinstance(revision, str) or len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
            raise LedgerError(f"{name}: invalid expected suite pin")
        cases = suite.get("cases")
        if not isinstance(cases, list) or not cases:
            raise LedgerError(f"{name}: empty expectations")
        wanted = {}
        for case in cases:
            if not isinstance(case, list) or len(case) != 4 or not all(isinstance(v, str) and v for v in case) or case[3] not in {"pass", "xfail"}:
                raise LedgerError(f"{name}: malformed expected case")
            key = tuple(case[:3])
            if key in wanted:
                raise LedgerError(f"{name}: duplicate expected case {key}")
            wanted[key] = case[3]
        path = directory / name
        try:
            measured = json.loads(path.read_text())
        except (OSError, ValueError) as error:
            raise LedgerError(f"{name}: missing/invalid measured ledger") from error
        if measured.get("suite_revision") != suite.get("suite_revision"):
            raise LedgerError(f"{name}: suite revision differs")
        actual = {key: outcome(row) for key, row in load_rows(path).items()}
        for key in sorted(set(wanted) | set(actual)):
            if wanted.get(key) != actual.get(key):
                failures.append(f"{name}: {key}: expected {wanted.get(key, 'ABSENT')}, observed {actual.get(key, 'ABSENT')}")
    extra = {p.name for p in directory.glob("*.json") if "results" in json.loads(p.read_text())} - set(expected["suites"])
    failures.extend(f"unlisted suite: {name}" for name in sorted(extra))
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", type=Path, default=Path("benchmark/public/expected-results.json"))
    parser.add_argument("--results-dir", type=Path, required=True)
    parser.add_argument("--write", action="store_true", help="author a changed expectation file for review; never use in a gate")
    args = parser.parse_args(argv)
    try:
        if args.write:
            args.expected.write_text(json.dumps(expectations(args.results_dir), indent=2) + "\n")
            return 0
        failures = check(json.loads(args.expected.read_text()), args.results_dir)
    except (LedgerError, OSError, ValueError, TypeError) as error:
        print(f"expected-results: UNKNOWN: {error}", file=sys.stderr)
        return 2
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print("All pinned pass/xfail expectations match; no regression or unexpected pass.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
