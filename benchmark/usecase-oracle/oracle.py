"""Compare an observed merge with an explicit use-case fixture, not SHACL.

Adapters supply canonical facts and history records. This module never imports
the operator, reads shapes, fetches data, or computes a three-way merge.
"""
from __future__ import annotations

import json
from collections import Counter
from typing import Any


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def _records(values: Any, name: str) -> set[str]:
    if not isinstance(values, list):
        raise ValueError(f"{name} must be an explicit list")
    return {_canonical(value) for value in values}


def _history_records(values: Any, name: str) -> Counter[str]:
    if not isinstance(values, list):
        raise ValueError(f"{name} must be an explicit list")
    return Counter(_canonical(value) for value in values)


def evaluate(fixture: dict, observation: dict) -> dict:
    """Require explicit current/history expectations and both observed views.

    History entries are canonical records (including transaction role and valid
    interval), not just triples ever seen. Extra entries fail exact equality;
    missing history is never inferred from current-state loss or vice versa.
    Lists must remain a single structured value, so their order and duplicates
    are significant even though outer fact collections are sets.
    """
    if fixture.get("schema") != "quipu-usecase-oracle/v1":
        raise ValueError("unknown fixture schema")
    if not isinstance(fixture.get("case"), str) or not fixture["case"]:
        raise ValueError("fixture needs a use-case id")
    expected_current = _records(fixture.get("expected_current"), "expected_current")
    expected_history = _history_records(fixture.get("expected_history"), "expected_history")
    current = _records(observation.get("current"), "current")
    history = _history_records(observation.get("history"), "history")
    missing_current = sorted(expected_current - current)
    extra_current = sorted(current - expected_current)
    missing_history = sorted((expected_history - history).elements())
    extra_history = sorted((history - expected_history).elements())
    report_mismatches = {}
    if "expected_report" in fixture:
        expected_report = fixture["expected_report"]
        actual_report = observation.get("report")
        if not isinstance(expected_report, dict) or not isinstance(actual_report, dict):
            raise ValueError("expected_report and observed report must be objects")
        for key, expected in expected_report.items():
            if key not in actual_report or _canonical(actual_report[key]) != _canonical(expected):
                report_mismatches[key] = {"expected": expected,
                                          "observed": actual_report.get(key),
                                          "present": key in actual_report}
    return {
        "case": fixture["case"],
        "correct": not (missing_current or extra_current or missing_history
                        or extra_history or report_mismatches),
        "current_facts_lost": len(missing_current),
        "history_records_missing": len(missing_history),
        "missing_current": [json.loads(v) for v in missing_current],
        "unexpected_current": [json.loads(v) for v in extra_current],
        "missing_history": [json.loads(v) for v in missing_history],
        "unexpected_history": [json.loads(v) for v in extra_history],
        "report_mismatches": report_mismatches,
    }
