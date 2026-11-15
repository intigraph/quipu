"""Explicit pinned-read answers from C4-gate, independent of final-state rows.

Transaction tokens name actual receive/decide receipts after adapter binding.
An adapter must query each pin; it must not replay the expected trace locally.
"""
from copy import deepcopy

from temporal_cases import CASES as TEMPORAL_CASES, T0, TD


CASE = deepcopy(next(case for case in TEMPORAL_CASES if case["case"] == "C4-theirs"))
CASE["case"] = "C4-gate-as-of"
CASE["expected_as_of"] = [
    # The receipt [Td,Td) is never visible, even at its own transaction.
    {"tx": "receive", "valid_at": TD, "values": ["b"]},
    {"tx": "decide", "valid_at": TD, "values": ["c"]},
    # Deciding cannot rewrite ROOT's earlier valid-time history.
    {"tx": "decide", "valid_at": T0, "values": ["a"]},
]
