"""Independent ledger and measurement-instrument controls, without a merge mock."""
import unittest
import tempfile
from pathlib import Path
import sys
import model
from run import Run, equal


class OracleControls(unittest.TestCase):
    def test_seed_reproducibility_and_work_count(self):
        first = model.scenario(100, 5, 42)
        self.assertEqual(first, model.scenario(100, 5, 42))
        self.assertNotEqual(first, model.scenario(100, 5, 7))
        self.assertTrue(all(len(side) == 100 for side in first[1]))
        self.assertEqual(len(first[2]), 10)

    def test_retractions_replacements_and_mappings_are_in_oracle(self):
        base, sides, _ = model.scenario(100, 2, 42)
        expected = model.oracle(base, sides)
        self.assertTrue(base - expected)  # cannot pass an append-only engine
        self.assertTrue(expected - base)
        for i, side in enumerate(sides):
            for op in side:
                if op["kind"] != "conflict" or i == 0:
                    self.assertTrue(op["add"] <= expected)
                    self.assertFalse(op["remove"] & expected)
        self.assertTrue(any(p == model.SSSOM + "predicate_id" for _, p, _ in expected))

    def test_conflicts_are_only_seeded_overlapping_slots(self):
        _, sides, expected = model.scenario(100, 5, 42)
        changed = [{(s, p) for op in side for s, p, _ in op["remove"] | op["add"]}
                   for side in sides]
        for i, a in enumerate(changed):
            for b in changed[i+1:]:
                self.assertEqual(a & b, expected)

    def test_oracle_detects_lost_spurious_and_resurrected_facts(self):
        base, sides, _ = model.scenario(100, 2, 42)
        expected = model.oracle(base, sides)
        equal(model.render(expected), expected)
        for mutant in (expected - {next(iter(expected))}, expected | (base-expected),
                       expected | {(model.NS+"spurious", model.TAG, '"bad"')}):
            with self.assertRaises(AssertionError):
                equal(model.render(mutant), expected)
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            equal(model.render(expected) + model.render({next(iter(expected))}), expected)

    def test_hold_and_resolution_are_distinct(self):
        base, sides, conflicts = model.scenario(100, 2, 42)
        hold, resolved = model.oracle(base, sides, hold=True), model.oracle(base, sides)
        self.assertNotEqual(hold, resolved)
        for s, p in conflicts:
            self.assertIn((s, p, '"base"'), hold)
            self.assertIn((s, p, '"side-0"'), resolved)

    def test_measurement_refuses_missing_tool_and_bounds_owned_child(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = Run(Path(directory), Path('/unused'), Path('/unused'), 1)
            with self.assertRaises(RuntimeError):
                runner.cmd(['/definitely-not-a-merge-tool'])
            with self.assertRaises(TimeoutError):
                runner.cmd([sys.executable, '-c', 'import time;time.sleep(30)'])


if __name__ == "__main__":
    unittest.main()
