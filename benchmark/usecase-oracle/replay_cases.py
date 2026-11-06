"""C8 observations after a second action, normalized by a future adapter.

The merge and transaction names are fixture-local tokens, not native IDs.
Adapters preserve identity and order while replacing native IDs with these
tokens. The judge does not invoke an apply or manufacture these observations.
"""

from temporal_cases import T0, TO, TD, row


CASES = [
    {"schema": "quipu-usecase-oracle/v1", "case": "C8a",
     "source": "docs/design/merge-behaviour.md", "path": "store",
     "expected_current": ["c"],
     "expected_history": [row("a", T0, TO), row("b", TO, TD),
                          row("c", TD, TD), row("c", TD, None)],
     "expected_transactions": [],  # The second apply adds no transactions.
     "expected_report": {"status": "already-applied", "merge_id": "merge-a",
                         "tx_ids": ["receive-a", "decide-a"],
                         "transaction_count_before": 2, "transaction_count_after": 2}},
    {"schema": "quipu-usecase-oracle/v1", "case": "C8b",
     "source": "docs/design/merge-behaviour.md", "path": "store",
     # ROOT changed from the reviewed b to d before the attempted apply.
     "expected_current": ["d"],
     "expected_history": [row("a", T0, TO), row("b", TO, TD), row("d", TD, None)],
     "expected_transactions": [],
     "expected_report": {"status": "stale", "transaction_count_before": 2,
                         "transaction_count_after": 2}},
    {"schema": "quipu-usecase-oracle/v1", "case": "C8c",
     "source": "docs/design/merge-behaviour.md", "path": "store",
     "expected_current": ["c"],
     "expected_history": [row("a", T0, TO), row("c", TO, None)],
     "expected_transactions": [],
     "expected_report": {"transaction_count_before": 1, "transaction_count_after": 1}},
    {"schema": "quipu-usecase-oracle/v1", "case": "C8d",
     "source": "docs/design/merge-behaviour.md", "path": "store",
     "expected_current": ["c", "d"],
     "expected_history": [row("c", TO, None), row("d", TD, None)],
     "expected_transactions": [{"role": "receive", "close": [],
                                "assert": [row("d", TD, None)]}],
     "expected_report": {"second_already_applied": False,
                         "merge_ids": ["merge-a", "merge-b"],
                         "filtered_tx_ids": {"merge-a": ["receive-a"],
                                             "merge-b": ["receive-b"]},
                         "transaction_count_before": 1, "transaction_count_after": 2}},
]
