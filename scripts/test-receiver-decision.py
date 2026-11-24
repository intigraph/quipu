#!/usr/bin/env python3
"""# arming: ci/library; real disposable child, no native graph or publication."""

from contextlib import redirect_stderr
import importlib.util
import io
import base64
import gzip
import hashlib
import random
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("decision", ROOT / "repository_receiver_decision.py")
M = importlib.util.module_from_spec(spec)
spec.loader.exec_module(M)


class Decision(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="receiver-decision-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)
        self.input = self.root / "input.json"
        self.log = self.root / "commands.jsonl"
        self.receipt = self.root / "retained.json"
        self.child = self.root / "child.py"
        self.child.write_text('''import json,sys
from pathlib import Path
with Path(sys.argv[2]).open("a") as log: log.write(json.dumps(sys.argv[3:])+"\\n")
print(Path(sys.argv[1]).read_text() if "promote" not in sys.argv[3:] else "{}")
''')
        self.good = dict(outcome="staged", share_id="fixture-share",
                         promotion=dict(eligible=True, blockers=[]), triples=dict(accepted=4))

    def run_child(self, args):
        return subprocess.run([sys.executable, str(self.child), str(self.input),
                               str(self.log), *map(str, args[1:])], check=True,
                              capture_output=True, text=True, timeout=10).stdout

    def import_one(self, raw):
        self.input.write_text(raw)
        return M.import_receiver(self.run_child, "fixture-cli", "stage", "receiver.db",
                                 self.receipt, "a" * 40)

    def test_positive_retained_before_one_promote(self):
        with redirect_stderr(io.StringIO()): result = self.import_one(json.dumps(self.good))
        self.assertEqual(result, self.good)
        rows = [json.loads(v) for v in self.log.read_text().splitlines()]
        self.assertEqual([r[:2] for r in rows], [["import", "stage"], ["import", "promote"]])
        self.assertEqual(json.loads(self.receipt.read_text())["decision"], self.good)
        self.assertEqual(self.receipt.stat().st_mode & 0o777, 0o600)

    def test_each_rejection_is_named_private_and_never_promotes(self):
        cases = [(dict(self.good, outcome="quarantined"), "outcome_not_staged"),
                 (dict(self.good, promotion=dict(eligible=False)), "promotion_ineligible"),
                 (dict(self.good, triples=dict(accepted=True)), "accepted_not_integer"),
                 (dict(self.good, triples=dict(accepted=3)), "accepted_too_small")]
        for value, reason in cases:
            with self.subTest(reason=reason):
                self.log.unlink(missing_ok=True)
                output = io.StringIO()
                with redirect_stderr(output), self.assertRaisesRegex(ValueError, reason):
                    self.import_one(json.dumps(value))
                receipt = json.loads(self.receipt.read_text())
                self.assertIn(reason, receipt["reasons"])
                self.assertEqual(receipt["decision"], value)
                self.assertIn(reason, output.getvalue())
                self.assertEqual(len(self.log.read_text().splitlines()), 1)

    def test_arbitrary_private_fields_never_reach_summary(self):
        secret = "ghp_SYNTHETIC_SECRET_NOT_REAL"
        value = dict(self.good, outcome=secret, promotion=dict(eligible=False,
            blockers=["shacl_nonconforming", secret]), validation=dict(blocking=True,
            conforms=False, off_vocabulary=[secret], report=secret), detail=secret)
        output = io.StringIO()
        with redirect_stderr(output), self.assertRaises(ValueError): self.import_one(json.dumps(value))
        self.assertIn(secret, self.receipt.read_text())
        self.assertNotIn(secret, output.getvalue())
        self.assertIn('"off_vocabulary_count": 1', output.getvalue())
        self.assertLess(len(output.getvalue().encode()), 2048)

    def test_malformed_and_duplicate_responses_remain_bounded_refusals(self):
        for raw, reason in [("not-json", "import_json_invalid"),
                            ('{"outcome":"staged","outcome":"other"}', "import_json_invalid"),
                            ("x" * (M.MAX_FULL_DECISION + 1), "import_json_invalid")]:
            with self.subTest(reason=reason), redirect_stderr(io.StringIO()):
                self.log.unlink(missing_ok=True)
                with self.assertRaisesRegex(ValueError, reason): self.import_one(raw)
                self.assertLess(self.receipt.stat().st_size, M.MAX_RECEIPT)
                self.assertEqual(len(self.log.read_text().splitlines()), 1)

    def test_large_valid_response_promotes_once_and_retains_complete_compressed_evidence(self):
        value = dict(self.good, detail="x" * 4_050_729)
        raw = json.dumps(value)
        output = io.StringIO()
        with redirect_stderr(output):
            self.assertEqual(self.import_one(raw), value)
        receipt = json.loads(self.receipt.read_text())
        self.assertTrue(receipt["decision_compacted"])
        self.assertTrue(receipt["response_retained_complete"])
        restored = gzip.decompress(base64.b64decode(receipt["response_gzip_base64"]))
        self.assertEqual(restored, (raw + "\n").encode())
        self.assertEqual(receipt["response_sha256"], hashlib.sha256(restored).hexdigest())
        self.assertLess(self.receipt.stat().st_size, M.MAX_RECEIPT)
        self.assertLess(len(output.getvalue().encode()), 2048)
        commands = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertEqual([row[:2] for row in commands], [["import", "stage"], ["import", "promote"]])

    def test_incompressible_valid_response_does_not_change_admission(self):
        noise = random.Random(0).randbytes(2 * M.MAX_COMPRESSED_RESPONSE)
        value = dict(self.good, detail=base64.b64encode(noise).decode("ascii"))
        raw = json.dumps(value)
        with redirect_stderr(io.StringIO()):
            self.assertEqual(self.import_one(raw), value)
        receipt = json.loads(self.receipt.read_text())
        self.assertTrue(receipt["decision_compacted"])
        self.assertFalse(receipt["response_retained_complete"])
        self.assertNotIn("response_gzip_base64", receipt)
        self.assertEqual(receipt["response_bytes"], len((raw + "\n").encode()))
        self.assertEqual(receipt["response_sha256"], hashlib.sha256((raw + "\n").encode()).hexdigest())
        self.assertEqual(receipt["decision"]["triples"]["accepted"], 4)
        self.assertLess(self.receipt.stat().st_size, M.MAX_RECEIPT)
        self.assertEqual(len(self.log.read_text().splitlines()), 2)

    def test_large_rejections_keep_original_reasons_and_never_promote(self):
        for change, reason in [(dict(outcome="quarantined"), "outcome_not_staged"),
                               (dict(promotion=dict(eligible=False)), "promotion_ineligible"),
                               (dict(triples=dict(accepted=3)), "accepted_too_small")]:
            self.log.unlink(missing_ok=True)
            value = dict(self.good, detail="x" * 4_050_729, **change)
            with redirect_stderr(io.StringIO()), self.assertRaisesRegex(ValueError, reason):
                self.import_one(json.dumps(value))
            receipt = json.loads(self.receipt.read_text())
            self.assertIn(reason, receipt["reasons"])
            self.assertTrue(receipt["response_retained_complete"])
            self.assertEqual(len(self.log.read_text().splitlines()), 1)
            self.assertLess(self.receipt.stat().st_size, M.MAX_RECEIPT)

    def test_receipt_survives_build_cleanup_and_refuses_foreign_custody(self):
        with tempfile.TemporaryDirectory(dir=self.root) as build:
            self.input = Path(build) / "input.json"
            with redirect_stderr(io.StringIO()): self.import_one(json.dumps(self.good))
        self.assertTrue(self.receipt.is_file())
        self.assertEqual(json.loads(self.receipt.read_text())["source_revision"], "a" * 40)
        original = self.receipt.read_bytes()
        self.receipt.chmod(0o644)
        with self.assertRaises(ValueError): M.retain(self.receipt, {})
        self.assertEqual(self.receipt.read_bytes(), original)
        self.receipt.chmod(0o600)
        os.link(self.receipt, self.root / "evidence")
        with self.assertRaises(ValueError): M.retain(self.receipt, {})
        link = self.root / "link"; link.symlink_to(self.receipt)
        with self.assertRaises(ValueError): M.retain(link, {})


if __name__ == "__main__": unittest.main()
