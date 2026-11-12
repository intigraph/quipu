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
    _case("C1b", ["a"], ["b"], ["a"], ["b"],
          [row("a", T0, TO), row("b", TO, None)], [],
          {"kind": "one-side", "held": 0}),
    _case("C1c", ["a"], ["a"], [], [], [row("a", T0, TT)],
          [{"role": "receive", "close": [{"value": "a", "at": TT}], "assert": []}],
          {"kind": "one-side", "held": 0}),
    _case("C2a", ["a"], ["c"], ["c"], ["c"],
          [row("a", T0, TO), row("c", TO, None)], [],
          {"kind": "convergent", "decision": "continue", "applied": 0, "held": 0}),
    _case("C2b", ["a"], [], [], [], [row("a", T0, TO)], [],
          {"kind": "convergent", "decision": "continue", "applied": 0, "held": 0}),
    _case("C2c", ["a"], ["c"], ["c"], ["d"],
          [row("a", T0, TO), row("c", TO, TD), row("d", TD, None)],
          [{"role": "decide", "close": [{"value": "c", "at": TD}],
            "assert": [row("d", TD, None)]}],
          {"kind": "convergent", "decision": "value", "held": 0}),
    _case("C3a", ["a"], ["a", "b"], ["a", "c"], ["a", "b", "c"],
          [row("a", T0, None), row("b", TO, None), row("c", TT, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TT, None)]}],
          {"kind": "set-merge", "held": 0}) | {"card": "many"},
    _case("C3b", ["a"], ["b"], ["c"], ["b", "c"],
          [row("a", T0, TO), row("b", TO, None), row("c", TT, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TT, None)]}],
          {"kind": "set-merge", "held": 0}) | {"card": "many"},
    _case("C3c", ["a"], ["a", "b"], [], ["b"],
          [row("a", T0, TT), row("b", TO, None)],
          [{"role": "receive", "close": [{"value": "a", "at": TT}], "assert": []}],
          {"kind": "set-merge", "held": 0}) | {"card": "many"},
    _case("C3d", [], ["d"], ["d", "e"], ["d", "e"],
          [row("d", TO, None), row("e", TT, None)],
          [{"role": "receive", "close": [], "assert": [row("e", TT, None)]}],
          {"kind": "set-merge", "held": 0}) | {"card": "many"},
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
    _case("C4-base", ["a"], ["b"], ["c"], ["a"],
          [row("a", T0, TO), row("b", TO, TD), row("c", TD, TD), row("a", TD, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]},
           {"role": "decide", "close": [{"value": "b", "at": TD}],
            "assert": [row("a", TD, None)]}],
          {"kind": "maxcount-exceeded", "decision": "base", "held": 0}),
    _case("C4-value", ["a"], ["b"], ["c"], ["d"],
          [row("a", T0, TO), row("b", TO, TD), row("c", TD, TD), row("d", TD, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]},
           {"role": "decide", "close": [{"value": "b", "at": TD}],
            "assert": [row("d", TD, None)]}],
          {"kind": "maxcount-exceeded", "decision": "value", "held": 0}),
    _case("C4-both-refused", ["a"], ["b"], ["c"], ["b"],
          [row("a", T0, TO), row("b", TO, None)], [],
          {"kind": "maxcount-exceeded", "decision": "both", "status": "refused"}),
    _case("C4-defer-atomic", ["a"], ["b"], ["c"], ["b"],
          [row("a", T0, TO), row("b", TO, None)], [],
          {"kind": "maxcount-exceeded", "decision": "defer", "held": 1}),
    _case("C5a-ours", ["a"], [], ["c"], [],
          [row("a", T0, TO), row("c", TD, TD)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]}],
          {"kind": "delete-replace", "decision": "ours", "held": 0}),
    _case("C5a-theirs", ["a"], [], ["c"], ["c"],
          [row("a", T0, TO), row("c", TD, TD), row("c", TD, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]},
           {"role": "decide", "close": [], "assert": [row("c", TD, None)]}],
          {"kind": "delete-replace", "decision": "theirs", "held": 0}),
    _case("C5a-base", ["a"], [], ["c"], ["a"],
          [row("a", T0, TO), row("c", TD, TD), row("a", TD, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TD, TD)]},
           {"role": "decide", "close": [], "assert": [row("a", TD, None)]}],
          {"kind": "delete-replace", "decision": "base", "held": 0}),
    _case("C5b-ours", ["a"], ["c"], [], ["c"],
          [row("a", T0, TO), row("c", TO, None)], [],
          {"kind": "delete-replace", "decision": "ours", "held": 0}),
    _case("C5b-theirs", ["a"], ["c"], [], [],
          [row("a", T0, TO), row("c", TO, TD)],
          [{"role": "decide", "close": [{"value": "c", "at": TD}], "assert": []}],
          {"kind": "delete-replace", "decision": "theirs", "held": 0}),
    _case("C5c", ["a"], [], ["c"], ["c"],
          [row("a", T0, TO), row("c", TT, None)],
          [{"role": "receive", "close": [], "assert": [row("c", TT, None)]}],
          {"kind": "set-merge", "held": 0}) | {"card": "many"},
    # Fixture fixes ROOT's earlier close at To; the reclaimed row starts at Tr.
    _case("C7", ["a"], ["a"], [], [],
          [row("a", T0, TO), row("a", TR, TD)],
          [{"role": "receive", "close": [{"value": "a", "at": TD}], "assert": []}],
          {"kind": "one-side", "held": 0}),
]
