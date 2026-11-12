"""The committed measurement is stable, but outcome drift cannot disappear."""

import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest

import ledger_snapshot as ledger


class LedgerSnapshotTests(unittest.TestCase):
    def run_tool(self, *args):
        with contextlib.redirect_stdout(io.StringIO()):
            return ledger.main(list(map(str, args)))

    def fixture(self, root):
        source = root / "raw"
        source.mkdir()
        data = {"quipu_revision": "measured", "suite_revision": "pinned",
                "generated_at": "first", "generated_by": "run-1",
                "results": [{"id": "one", "status": "passed", "duration_ms": 1.2}]}
        for name in ledger.LEDGERS:
            (source / name).write_text(json.dumps(data))
        return source, data

    def test_repeated_measurements_keep_bytes_and_revision_but_not_telemetry(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, data = self.fixture(root)
            first, second = root / "first", root / "second"
            self.assertEqual(self.run_tool("--source", source, "--output", first), 0)
            data.update(generated_at="later", generated_by="run-2")
            data["results"][0]["duration_ms"] = 9000
            for name in ledger.LEDGERS:
                (source / name).write_text(json.dumps(data))
            self.assertEqual(self.run_tool("--source", source, "--output", second), 0)
            for name in ledger.LEDGERS:
                self.assertEqual((first / name).read_bytes(), (second / name).read_bytes())
                self.assertEqual(json.loads((second / name).read_text())["quipu_revision"], "measured")
            self.assertIn("generated_at", data)  # raw evidence is not mutated

    def test_queue_commit_identity_is_separate_from_outcome_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, data = self.fixture(root)
            stable = root / "stable"
            self.assertEqual(self.run_tool("--source", source, "--output", stable), 0)
            data["quipu_revision"] = "queue-candidate"
            path = source / ledger.LEDGERS[0]
            path.write_text(json.dumps(data))
            self.assertEqual(self.run_tool("--source", source, "--check", stable), 0)
            for field, value in (("status", "failed"), ("id", "missing-original-case")):
                changed = copy.deepcopy(data)
                changed["results"][0][field] = value
                path.write_text(json.dumps(changed))
                self.assertEqual(self.run_tool("--source", source, "--check", stable), 1)

    def test_incomplete_measurement_fails_without_partial_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, _ = self.fixture(root)
            (source / ledger.LEDGERS[-1]).unlink()
            output = root / "output"
            self.assertEqual(self.run_tool("--source", source, "--output", output), 2)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
