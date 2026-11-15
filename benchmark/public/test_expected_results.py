"""Real gate controls, including both directions of behavioural drift."""
import copy
import json
import tempfile
import unittest
from pathlib import Path

import expected_results as gate


class ExpectedResults(unittest.TestCase):
    def test_strict_inventory_outcomes_and_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "suite.json"
            ledger = {"suite_revision": "a" * 40, "generated_at": "old", "results": [
                {"class": "eval", "manifest": "one", "id": "pass", "status": "passed", "duration_ms": 9},
                {"class": "eval", "manifest": "one", "id": "known", "status": "failed"}]}
            path.write_text(json.dumps(ledger))
            expected = gate.expectations(root)
            self.assertNotIn("generated_at", json.dumps(expected))
            self.assertNotIn("duration_ms", json.dumps(expected))
            self.assertEqual(gate.check(expected, root), [])
            for index, status in [(0, "failed"), (1, "passed")]:
                changed = copy.deepcopy(ledger)
                changed["results"][index]["status"] = status
                path.write_text(json.dumps(changed))
                self.assertTrue(gate.check(expected, root))
                # Updating expectations is a separate author action, not a gate side effect.
                self.assertEqual(gate.check(gate.expectations(root), root), [])
            for changed in [dict(ledger, suite_revision="b" * 40), dict(ledger, results=[ledger["results"][0]]),
                            dict(ledger, results=ledger["results"] * 2)]:
                path.write_text(json.dumps(changed))
                try:
                    self.assertTrue(gate.check(expected, root))
                except gate.LedgerError:
                    pass
            changed = copy.deepcopy(ledger)
            changed["results"][1]["status"] = "runner_crashed"
            path.write_text(json.dumps(changed))
            with self.assertRaises(gate.LedgerError):
                gate.check(expected, root)

    def test_real_committed_inventory_is_deterministic(self):
        root = Path(__file__).parent / "results"
        generated = gate.expectations(root)
        committed = json.loads((root.parent / "expected-results.json").read_text())
        self.assertEqual(generated, committed)
        self.assertEqual(gate.check(committed, root), [])


if __name__ == "__main__":
    unittest.main()
