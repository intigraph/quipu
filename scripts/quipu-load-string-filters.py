"""Shared tl2q4j shapes and strict string coverage for the mixed-load ratchet."""
from __future__ import annotations

import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "query_perf_for_load", Path(__file__).with_name("quipu-query-perf.py"))
perf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(perf)
NAMES = tuple(name for name in perf.CLASSES if name.startswith(("C0_", "Q")))


def request(sequence: int) -> tuple[str, str, dict]:
    name = NAMES[sequence % len(NAMES)]
    return "string_filter_" + name, "/query", {
        "query": perf.render(perf.CLASSES[name], False, limit=True)}


def validate_response(raw: bytes) -> bool:
    """HTTP 200 alone does not prove a query returned valid, nonempty bindings."""
    import json
    try:
        body = json.loads(raw)
    except (ValueError, UnicodeError):
        return False
    return (isinstance(body, dict) and "error" not in body
            and perf.valid_rows(body.get("rows")) and bool(body["rows"]))


def evaluate(report: dict, budget: dict) -> list[str]:
    failures = []
    for name in NAMES:
        operation = "string_filter_" + name
        row = report["operations"].get(operation)
        if not row or row["requests"] < 3:
            failures.append(operation + ": fewer than three mixed samples")
            continue
        # Uncontended p95/differential/relative controls retain their original
        # budgets below. Mixed p99 permits queueing behind concurrent writers.
        ceiling = (30000 if name.startswith("C0_") else
                   budget["classes"][name]["max_p95_ms"] * 8)
        if row["p99_ms"] > ceiling:
            failures.append(f"{operation}: mixed p99 {row['p99_ms']} > {ceiling} ms")
        if row["statuses"] != {"ok": row["requests"]}:
            failures.append(operation + ": failed mixed response")
    return failures


def differential(url: str, budget: dict, timeout: float) -> dict:
    # Same runner, fixture, query bytes and strict decoder as the standalone CI
    # gate. Without #433 the defeated/pushed ratios collapse and refuse.
    report, _ = perf.gate(url, budget, 5, timeout, list(NAMES))
    return report
