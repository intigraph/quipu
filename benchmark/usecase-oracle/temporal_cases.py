"""Explicit single-slot store expectations from merge-behaviour.md.

Transactions are ordered observations, not commands for the merge operator.
The history projection includes the fixture's ROOT history, even if unchanged.
"""

T0 = "2026-01-01T00:00:00Z"
TO = "2026-01-02T00:00:00Z"
TT = "2026-01-03T00:00:00Z"
TF = "2026-01-04T00:00:00Z"
TR = "2026-01-05T00:00:00Z"
TD = "2026-01-10T00:00:00Z"


def row(value, start, end):
    return {"value": value, "from": start, "to": end}


def _case(name, base, ours, theirs, current, history, transactions, report):
    return {"schema": "quipu-usecase-oracle/v1", "case": name,
            "source": "docs/design/merge-behaviour.md", "path": "store",
            "card": "one", "base": base, "ours": ours, "theirs": theirs,
            "expected_current": current, "expected_history": history,
            "expected_transactions": transactions, "expected_report": report}


CASES = [
    _case("C1a", ["a"], ["a"], ["c"], ["c"],
          [row("a", T0, TT), row("c", TT, None)],
          [{"role": "receive", "close": [{"value": "a", "at": TT}],
            "assert": [row("c", TT, None)]}],
          {"kind": "one-side", "held": 0}),
    _case("C2a", ["a"], ["c"], ["c"], ["c"],
          [row("a", T0, TO), row("c", TO, None)], [],
          {"kind": "convergent", "decision": "continue", "applied": 0, "held": 0}),
    _case("C4-ours", ["a"], ["b"], ["c"], ["b"],
          [row("a", T0, TO), row("b", TO, None), row("c", TD, TD)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]}],
          {"kind": "maxcount-exceeded", "decision": "ours", "held": 0}),
    _case("C4-theirs", ["a"], ["b"], ["c"], ["c"],
          [row("a", T0, TO), row("b", TO, TD), row("c", TD, TD), row("c", TD, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]},
           {"role": "decide", "close": [{"value": "b", "at": TD}],
            "assert": [row("c", TD, None)]}],
          {"kind": "maxcount-exceeded", "decision": "theirs", "held": 0}),
    _case("C5b-theirs", ["a"], ["c"], [], [],
          [row("a", T0, TO), row("c", TO, TD)],
          [{"role": "decide", "close": [{"value": "c", "at": TD}], "assert": []}],
          {"kind": "delete-replace", "decision": "theirs", "held": 0}),
    # Fixture fixes ROOT's earlier close at To; the reclaimed row starts at Tr.
    _case("C7", ["a"], ["a"], [], [],
          [row("a", T0, TO), row("a", TR, TD)],
          [{"role": "receive", "close": [{"value": "a", "at": TD}], "assert": []}],
          {"kind": "one-side", "held": 0}),
]
