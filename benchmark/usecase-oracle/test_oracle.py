"""Discriminating controls for the judge, without invoking a merge operator."""
import unittest

from oracle import evaluate
from list_cases import CASES
from temporal_cases import CASES as TEMPORAL_CASES
from temporal_cases import TT


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

    def test_c6_explicit_oracles_and_wrong_reports_are_discriminated(self):
        self.assertEqual([case["case"] for case in CASES], [f"C6{x}" for x in "abcde"])
        for fixture in CASES:
            observation = {"current": fixture["expected_current"], "history": [],
                           "report": dict(fixture["expected_report"])}
            self.assertTrue(evaluate(fixture, observation)["correct"])
            observation["report"]["held"] = 1 - fixture["expected_report"]["held"]
            result = evaluate(fixture, observation)
            self.assertFalse(result["correct"])
            self.assertEqual(result["current_facts_lost"], 0)
            self.assertIn("held", result["report_mismatches"])

    def test_report_required_fields_cannot_disappear_into_an_empty_success(self):
        fixture = CASES[2]  # agreeing sequences still need the convergent flag
        observation = {"current": fixture["expected_current"], "history": [], "report": {}}
        result = evaluate(fixture, observation)
        self.assertFalse(result["correct"])
        self.assertFalse(result["report_mismatches"]["kind"]["present"])
        del observation["report"]
        with self.assertRaises(ValueError):
            evaluate(fixture, observation)

    def test_report_counts_do_not_accept_booleans_or_float_coercion(self):
        fixture = CASES[0]
        for wrong in [False, 0.0]:
            observation = {"current": fixture["expected_current"], "history": [],
                           "report": {"kind": "one-side", "held": wrong}}
            self.assertFalse(evaluate(fixture, observation)["correct"])
        observation["report"]["held"] = 0
        self.assertTrue(evaluate(fixture, observation)["correct"])

    def test_history_multiplicity_cannot_be_erased_by_set_comparison(self):
        fixture = self.fixture()
        duplicate = fixture["expected_history"] + [fixture["expected_history"][0]]
        observation = {"current": fixture["expected_current"], "history": duplicate}
        result = evaluate(fixture, observation)
        self.assertFalse(result["correct"])
        self.assertEqual(len(result["unexpected_history"]), 1)
        fixture["expected_history"], observation["history"] = duplicate, fixture["expected_history"]
        result = evaluate(fixture, observation)
        self.assertFalse(result["correct"])
        self.assertEqual(result["history_records_missing"], 1)

    def observation(self, fixture):
        import copy
        return copy.deepcopy({"current": fixture["expected_current"],
                              "history": fixture["expected_history"],
                              "report": fixture["expected_report"],
                              "transactions": fixture["expected_transactions"]})

    def test_temporal_fixtures_check_trace_order_and_receipt_presence(self):
        for fixture in TEMPORAL_CASES:
            self.assertTrue(evaluate(fixture, self.observation(fixture))["correct"])
        fixture = next(f for f in TEMPORAL_CASES if f["case"] == "C4-theirs")
        observation = self.observation(fixture)
        observation["transactions"].reverse()
        result = evaluate(fixture, observation)
        self.assertFalse(result["correct"])
        self.assertFalse(result["transactions_match"])
        observation = self.observation(fixture)
        del observation["transactions"][0]
        self.assertFalse(evaluate(fixture, observation)["correct"])
        del observation["transactions"]
        with self.assertRaises(ValueError):
            evaluate(fixture, observation)

    def test_old_authored_time_cannot_replace_a_decision_time_live_winner(self):
        fixture = next(f for f in TEMPORAL_CASES if f["case"] == "C4-theirs")
        observation = self.observation(fixture)
        observation["history"][-1]["from"] = TT
        self.assertFalse(evaluate(fixture, observation)["correct"])

    def test_set_merge_does_not_rewrite_root_history_or_duplicate_current_claims(self):
        fixture = next(f for f in TEMPORAL_CASES if f["case"] == "C3b")
        observation = self.observation(fixture)
        observation["history"][0]["to"] = TT
        self.assertFalse(evaluate(fixture, observation)["correct"])

        fixture = next(f for f in TEMPORAL_CASES if f["case"] == "C3d")
        observation = self.observation(fixture)
        observation["history"].append(dict(observation["history"][0]))
        self.assertFalse(evaluate(fixture, observation)["correct"])
        observation = self.observation(fixture)
        observation["transactions"][0]["assert"].append(dict(observation["history"][0]))
        self.assertFalse(evaluate(fixture, observation)["correct"])

    def test_delete_replace_receipt_does_not_become_a_live_claim(self):
        fixture = next(f for f in TEMPORAL_CASES if f["case"] == "C5a-ours")
        observation = self.observation(fixture)
        observation["history"][-1]["to"] = None
        self.assertFalse(evaluate(fixture, observation)["correct"])
        fixture = next(f for f in TEMPORAL_CASES if f["case"] == "C5b-ours")
        observation = self.observation(fixture)
        observation["transactions"].append({"role": "receive", "close": [], "assert": []})
        self.assertFalse(evaluate(fixture, observation)["correct"])


if __name__ == "__main__":
    unittest.main()
