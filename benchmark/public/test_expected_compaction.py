"""CI migration equality and independent negative controls."""
import copy
import json
import unittest
from pathlib import Path

import expected_results
import prove_expected_compaction as proof


class Compaction(unittest.TestCase):
    def test_exact_legacy_contract(self):
        document = json.loads(Path(__file__).with_name("expected-results.json").read_text())
        legacy = proof.prove(document)
        self.assertEqual(expected_results.expand(document), legacy)
        self.assertEqual(expected_results.compact(legacy), document)

    def test_changed_pin_id_outcome_missing_and_duplicate_refused(self):
        document = json.loads(Path(__file__).with_name("expected-results.json").read_text())
        for mutation in ("pin", "id", "outcome", "missing", "duplicate"):
            changed = copy.deepcopy(document)
            suite = next(iter(changed["suites"].values()))
            outcomes = next(iter(next(iter(suite["groups"].values())).values()))
            status = next(iter(outcomes))
            ids = outcomes[status]
            if mutation == "pin":
                suite["suite_revision"] = "0" * 40
            elif mutation == "id":
                ids[0] += "-changed"
            elif mutation == "outcome":
                outcomes.setdefault("pass" if status == "xfail" else "xfail", []).append(ids.pop())
            elif mutation == "missing":
                ids.pop()
            else:
                ids.append(ids[0])
            with self.subTest(mutation=mutation), self.assertRaises(AssertionError):
                proof.prove(changed)
