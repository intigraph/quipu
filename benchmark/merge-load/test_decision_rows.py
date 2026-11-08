"""Decision-report controls keep seeded conflicts and convergence distinct."""
import copy
import unittest
from run import decision_conflicts


class DecisionRows(unittest.TestCase):
    def setUp(self):
        self.pending = {"subject": "<urn:pending>", "predicate": "urn:p",
                        "kind": "max_count_exceeded", "ours": ["a"], "theirs": ["b"],
                        "decision": None}
        self.agreed = {"subject": "<urn:agreed>", "predicate": "urn:p",
                       "kind": "convergent", "ours": ["a"], "theirs": ["a"],
                       "decision": {"choose": "continue"}}
        self.expected = {("urn:pending", "urn:p")}

    def test_convergent_rows_do_not_hide_or_inflate_conflicts(self):
        file = {"rows": [self.agreed, self.pending]}
        pending = decision_conflicts(file, self.expected)
        self.assertEqual(pending, [self.pending])
        pending[0]["decision"] = {"choose": "ours"}
        self.assertEqual(file["rows"][0]["decision"], {"choose": "continue"})
        self.assertEqual(decision_conflicts({"rows": [self.agreed]}, set()), [])

    def test_missing_extra_and_duplicate_pending_rows_still_refuse(self):
        for rows in [[], [self.pending, self.pending], [self.pending, dict(self.pending, subject="<urn:extra>")]]:
            with self.subTest(rows=rows), self.assertRaises(AssertionError):
                decision_conflicts({"rows": rows}, self.expected)

    def test_forged_convergence_cannot_mask_a_conflict(self):
        for change in [{"theirs": ["b"]}, {"decision": None}, {"subject": "<urn:pending>"}]:
            forged = copy.deepcopy(self.agreed)
            forged.update(change)
            with self.subTest(change=change), self.assertRaises(AssertionError):
                decision_conflicts({"rows": [self.pending, forged]}, self.expected)
        with self.assertRaises(AssertionError):
            decision_conflicts({"rows": [self.agreed, self.agreed]}, set())
