import importlib.util
import pathlib
import sys
import unittest

PATH = pathlib.Path(__file__).with_name("shacl_write_path.py")
SPEC = importlib.util.spec_from_file_location("shacl_write_path", PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def row(case, expected=True, modes=None, control=None, status=None):
    modes = modes or {"single": expected, "per_triple": expected}
    if status is None:
        status = "agrees" if all(v == expected for v in modes.values()) else "diverges"
    return {"id": case, "components": [], "status": status, "expected_conforms": expected,
            "batch_conforms": expected, "modes": modes,
            "control_conforms": expected if control is None else control}


def suite(*extra):
    """98 agreeing cases with the cardinality control firing, plus `extra`."""
    rows = [row(MODULE.CONTROL_CASE, expected=False, control=True)]
    rows += [row(f"core/x/c{i}#c{i}") for i in range(MODULE.EXPECTED_CASES - 1 - len(extra))]
    return rows + list(extra)


class WritePathGateTests(unittest.TestCase):
    def test_clean_run_against_an_empty_ledger_passes(self):
        self.assertEqual(MODULE.gate(suite(), {"divergences": []}), [])

    def test_a_new_divergence_fails(self):
        bad = row("core/x/late#late", expected=False, modes={"single": False, "per_triple": True})
        failures = MODULE.gate(suite(bad), {"divergences": []})
        self.assertEqual(failures, ["NEW divergence: core/x/late#late under per_triple"])

    def test_a_known_divergence_passes_and_a_stale_one_fails(self):
        bad = row("core/x/late#late", expected=False, modes={"single": False, "per_triple": True})
        owned = {"core/x/late#late": {"class": "late-targeting", "item": "aegis-x"}}
        ledger = {"owners": owned, "divergences": [["core/x/late#late", "per_triple"]]}
        self.assertEqual(MODULE.gate(suite(bad), ledger), [])
        failures = MODULE.gate(suite(), ledger)
        self.assertEqual(len(failures), 1)
        self.assertIn("STALE ledger entry", failures[0])

    def test_an_owner_left_behind_after_its_entry_is_removed_fails(self):
        owned = {"core/x/late#late": {"class": "late-targeting", "item": "aegis-x"}}
        self.assertEqual(MODULE.gate(suite(), {"owners": owned, "divergences": []}),
                         ["STALE owner entry (case no longer in the ledger; remove it): core/x/late#late"])

    def test_a_known_divergence_without_class_and_item_fails(self):
        bad = row("core/x/late#late", expected=False, modes={"single": False, "per_triple": True})
        ledger = {"owners": {"core/x/late#late": {"class": "late-targeting"}},
                  "divergences": [["core/x/late#late", "per_triple"]]}
        self.assertEqual(MODULE.gate(suite(bad), ledger),
                         ["UNOWNED ledger case (needs class and item): core/x/late#late"])

    def test_a_missing_control_is_unknown_not_clean(self):
        rows = suite()
        rows[0]["control_conforms"] = False
        failures = MODULE.gate(rows, {"divergences": []})
        self.assertTrue(any(f.startswith("UNKNOWN: positive control") for f in failures), failures)

    def test_an_inventory_change_fails(self):
        failures = MODULE.gate(suite()[:-1], {"divergences": []})
        self.assertTrue(any(f.startswith("inventory:") for f in failures), failures)

    def test_a_batch_disagreement_fails_and_is_not_a_write_path_result(self):
        broken = row("core/x/batch#batch", status="batch_mismatch")
        rows = suite(broken)
        self.assertTrue(any("batch_mismatch" in f for f in MODULE.gate(rows, {"divergences": []})))
        self.assertNotIn(["core/x/batch#batch", "per_triple"], MODULE.divergences(rows))


if __name__ == "__main__":
    unittest.main()
