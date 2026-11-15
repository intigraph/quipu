"""Explicit C6 expectations from merge-behaviour.md, not a merge algorithm.

These are history-free pendant projections. Store receipts and clocks require
separate fixtures and are not inferred from this projection.
"""


def _case(name, mode, ours, theirs, expected, kind, held):
    return {"schema": "quipu-usecase-oracle/v1", "case": name,
            "source": "docs/design/merge-behaviour.md#c6-list-shaped-property",
            "path": "pendant", "card": "list", "mode": mode,
            "base": ["x", "y"], "ours": ours, "theirs": theirs,
            "expected_current": [{"subject": "urn:owner", "predicate": "urn:list",
                                  "list": expected}],
            "expected_history": [],
            "expected_report": {"kind": kind, "held": held}}


CASES = [
    _case("C6a", "whole", ["x", "y"], ["x", "y", "z"],
          ["x", "y", "z"], "one-side", 0),
    _case("C6b", "whole", ["y", "x"], ["x", "y", "z"],
          ["x", "y"], "list-divergent", 1),
    _case("C6c", "whole", ["x", "y", "z"], ["x", "y", "z"],
          ["x", "y", "z"], "convergent", 0),
    _case("C6d", "sequence", ["w", "x", "y"], ["x", "y", "z"],
          ["w", "x", "y", "z"], "divergent", 0),
    _case("C6e", "sequence", ["x", "q"], ["x", "r"],
          ["x", "y"], "list-divergent", 1),
]
