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


SHACL_REJECTION = {"accepted": False, "rejection": "shacl"}
LOCKED = {"accepted": None, "error": "database is locked"}


def full_modes(expected, **overrides):
    """All seven required split verdicts agreeing with `expected`, plus overrides."""
    return {**{mode: expected for mode in MODULE.EXPECTED_MODES}, **overrides}


def row(case, expected=True, modes=None, control=None, status=None):
    modes = full_modes(expected, **(modes or {}))
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


def judged_mode(expected):
    """A split whose one write got the expected SHACL verdict."""
    write = {"accepted": True} if expected else dict(SHACL_REJECTION)
    return {"conforms": expected, "writes": [write]}


def adapter_out(expected, single_writes, control=None):
    """--write-path output: `single` made of `single_writes`; every other required split agrees."""
    conforms = all(w.get("accepted") is True for w in single_writes)
    if any(w.get("accepted") is None for w in single_writes):
        conforms = None
    modes = {mode: judged_mode(expected) for mode in MODULE.EXPECTED_MODES}
    modes["single"] = {"conforms": conforms, "writes": single_writes}
    return {"batch": {"conforms": expected}, "triples": 2, "modes": modes,
            "control_payload_only": {"conforms": expected if control is None else control}}




class WriteErrorIsNotAVerdictTests(unittest.TestCase):
    """aegis-1c8q review F1: an operational error must never score as a SHACL rejection."""

    def test_a_shacl_rejection_of_an_invalid_case_still_agrees(self):
        row = MODULE.classify(adapter_out(False, [{"accepted": True}, SHACL_REJECTION]), False)
        self.assertEqual(row["status"], "agrees")
        self.assertEqual(row["write_errors"], [])

    def test_an_operational_error_on_an_invalid_case_is_a_write_error(self):
        row = MODULE.classify(adapter_out(False, [{"accepted": True}, LOCKED]), False)
        self.assertEqual(row["status"], "write_error")
        self.assertIn("database is locked", row["reason"])
        self.assertIsNone(row["modes"]["single"])
        self.assertEqual({m: v for m, v in row["modes"].items() if m != "single"},
                         {m: False for m in MODULE.EXPECTED_MODES - {"single"}})

    def test_an_error_reported_as_false_without_a_shacl_rejection_is_still_unjudged(self):
        # Defence in depth: the driver does not trust an adapter that folds an
        # error into conforms=false (the shape of the reviewer's falsifier).
        out = adapter_out(False, [{"accepted": False, "error": "database is locked"}])
        out["modes"]["single"]["conforms"] = False
        self.assertEqual(MODULE.classify(out, False)["status"], "write_error")

    def test_a_write_error_fails_the_gate(self):
        bad = {"id": "core/x/locked#locked", "components": []}
        bad.update(MODULE.classify(adapter_out(False, [LOCKED]), False))
        failures = MODULE.gate(suite(bad), {"divergences": []})
        self.assertTrue(any(f.startswith("core/x/locked#locked: write_error") for f in failures), failures)

    def test_a_write_error_on_a_known_divergent_case_is_not_masked_by_the_ledger(self):
        bad = {"id": "core/x/late#late", "components": []}
        bad.update(MODULE.classify(adapter_out(False, [LOCKED]), False))
        owned = {"core/x/late#late": {"class": "late-targeting", "item": "aegis-x"}}
        ledger = {"owners": owned, "divergences": [["core/x/late#late", "single"]]}
        failures = MODULE.gate(suite(bad), ledger)
        self.assertTrue(any(f.startswith("core/x/late#late: write_error") for f in failures), failures)


CANNOT_VERIFY = {"accepted": None, "error": "cannot verify: complex path"}
SEQ = "core/x/seq#seq"
SEQ_OWNER = {SEQ: {"class": "cannot-verify-complex-path", "item": "aegis-x"}}


def seq_row(single=False, errors=(CANNOT_VERIFY,)):
    """An invalid case whose split writes are unjudged and whose single write is judged."""
    out = adapter_out(False, [{"accepted": True} if single else dict(SHACL_REJECTION)])
    out["modes"]["single"]["conforms"] = single
    out["modes"]["per_triple"] = {"conforms": None, "writes": list(errors)}
    row = {"id": SEQ, "components": []}
    row.update(MODULE.classify(out, False))
    return row


class KnownUnjudgedLedgerTests(unittest.TestCase):
    """sattler's option A: a known unjudged write is an owned ledger entry bound to its error text."""

    def ledger(self, error="cannot verify: complex path", owners=SEQ_OWNER):
        return {"owners": owners, "divergences": [], "unjudged": [[SEQ, "per_triple", error]]}

    def test_a_known_owned_unjudged_pair_passes(self):
        self.assertEqual(MODULE.gate(suite(seq_row()), self.ledger()), [])

    def test_an_unjudged_pair_missing_from_the_ledger_still_fails(self):
        failures = MODULE.gate(suite(seq_row()), {"divergences": []})
        self.assertTrue(any(f.startswith(f"{SEQ}: write_error") for f in failures), failures)

    def test_a_stale_unjudged_entry_fails(self):
        judged = {"id": SEQ, "components": []}
        judged.update(MODULE.classify(adapter_out(False, [SHACL_REJECTION]), False))
        failures = MODULE.gate(suite(judged), self.ledger())
        self.assertTrue(any(f.startswith("STALE unjudged entry") for f in failures), failures)

    def test_a_changed_error_text_fails(self):
        failures = MODULE.gate(suite(seq_row()), self.ledger(error="cannot verify: something else"))
        self.assertTrue(any(f.startswith(f"{SEQ}: write_error") for f in failures), failures)
        self.assertTrue(any(f.startswith("STALE unjudged entry") for f in failures), failures)

    def test_an_unowned_unjudged_case_fails(self):
        failures = MODULE.gate(suite(seq_row()), self.ledger(owners={}))
        self.assertEqual(failures, [f"UNOWNED ledger case (needs class and item): {SEQ}"])

    def test_an_unjudged_mode_does_not_hide_a_divergence_in_a_judged_mode(self):
        failures = MODULE.gate(suite(seq_row(single=True)), self.ledger())
        self.assertIn(f"NEW divergence: {SEQ} under single", failures)


class ModeInventoryTests(unittest.TestCase):
    """franklin's #707 finding: a lost split must not leave the gate green."""

    def test_deleting_one_mode_from_every_case_fails_against_an_unchanged_ledger(self):
        rows = suite()
        self.assertEqual(MODULE.gate(rows, {"divergences": []}), [])
        for r in rows:
            del r["modes"]["single"]
        failures = MODULE.gate(rows, {"divergences": []})
        self.assertEqual(len(failures), MODULE.EXPECTED_CASES, failures[:3])
        self.assertTrue(all("mode inventory: missing ['single']" in f for f in failures), failures[:3])

    def test_an_empty_mode_map_is_not_classified_as_agreeing(self):
        out = adapter_out(True, [{"accepted": True}])
        out["modes"] = {}
        bad = {"id": "core/x/none#none", "components": []}
        bad.update(MODULE.classify(out, True))
        self.assertEqual(bad["status"], "mode_inventory")
        self.assertTrue(any(f.startswith("core/x/none#none: mode_inventory")
                            for f in MODULE.gate(suite(bad), {"divergences": []})))

    def test_a_split_with_no_writes_for_nonempty_data_is_refused(self):
        out = adapter_out(True, [{"accepted": True}])
        out["modes"]["per_triple"] = {"conforms": True, "writes": []}
        self.assertEqual(MODULE.classify(out, True)["status"], "mode_inventory")

    def test_an_unexpected_extra_mode_fails(self):
        rows = suite()
        rows[5]["modes"]["mystery_split"] = True
        failures = MODULE.gate(rows, {"divergences": []})
        self.assertEqual(len(failures), 1)
        self.assertIn("unexpected ['mystery_split']", failures[0])


if __name__ == "__main__":
    unittest.main()
