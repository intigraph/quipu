"""Discriminating controls for the judge, without invoking a merge operator."""
import unittest

from oracle import evaluate


class OracleTests(unittest.TestCase):
    def fixture(self):
        # R3/R5: incoming receipt stays in history; ours closes only at Td.
        return {"schema": "quipu-usecase-oracle/v1", "case": "R3-ours",
                "expected_current": ["ours"], "expected_history": [
                    {"value": "ours", "from": "To", "to": None, "role": "root"},
                    {"value": "theirs", "from": "Td", "to": "Td", "role": "receive"}]}

    def test_rejected_incoming_is_not_current_loss_but_must_remain_history(self):
        fixture = self.fixture()
        observation = {"current": ["ours"], "history": fixture["expected_history"]}
        self.assertTrue(evaluate(fixture, observation)["correct"])
        observation["history"] = fixture["expected_history"][:1]
        result = evaluate(fixture, observation)
        self.assertFalse(result["correct"])
        self.assertEqual(result["current_facts_lost"], 0)
        self.assertEqual(result["history_records_missing"], 1)

    def test_current_loss_does_not_mean_history_was_erased(self):
        fixture = self.fixture()
        result = evaluate(fixture, {"current": [], "history": fixture["expected_history"]})
        self.assertFalse(result["correct"])
        self.assertEqual(result["current_facts_lost"], 1)
        self.assertEqual(result["history_records_missing"], 0)

    def test_wrong_interval_and_extra_live_winner_fail_even_with_all_values(self):
        fixture = self.fixture()
        history = [dict(v) for v in fixture["expected_history"]]
        history[1]["to"] = None
        result = evaluate(fixture, {"current": ["ours", "theirs"], "history": history})
        self.assertFalse(result["correct"])
        self.assertEqual(result["history_records_missing"], 1)
        self.assertEqual(len(result["unexpected_history"]), 1)

    def test_list_order_and_duplicates_are_not_erased_by_set_scoring(self):
        fixture = {"schema": "quipu-usecase-oracle/v1", "case": "C6-list",
                   "expected_current": [{"list": ["x", "x", "y"]}], "expected_history": []}
        for wrong in [["x", "y"], ["y", "x", "x"]]:
            self.assertFalse(evaluate(fixture, {"current": [{"list": wrong}], "history": []})["correct"])
        self.assertTrue(evaluate(fixture, {"current": fixture["expected_current"], "history": []})["correct"])

    def test_missing_views_are_unknown_input_not_a_successful_empty_result(self):
        for observation in [{}, {"current": []}, {"history": []}]:
            with self.assertRaises(ValueError):
                evaluate(self.fixture(), observation)


if __name__ == "__main__":
    unittest.main()
