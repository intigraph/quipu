"""Execute the queue validator on real synthetic Git combined trees."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("check-merge-group-ledger.py")
ROOT = SCRIPT.resolve().parents[2]


class MergeGroupLedgerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "user.name", "Fixture")
        self.write("src/lib.rs", "// measured\n")
        self.write("benchmark/public/conformance_report.py", 'CODE_PATHS = ("src",)\n')
        self.measured = self.commit("measured code")
        self.ledger(self.measured)
        self.base = self.commit("publish measured ledger")
        self.git("switch", "-qc", "entry")
        self.write("README.md", "queue entry\n")
        self.commit("entry")
        self.git("switch", "-q", "main")
        self.write("docs.txt", "another entry ahead\n")
        self.commit("queue base")
        self.git("merge", "--no-ff", "-qm", "synthetic group", "entry")
        self.head = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root,
                                       text=True, stderr=subprocess.PIPE).strip()

    def write(self, path, text):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)

    def commit(self, title):
        self.git("add", ".")
        self.git("commit", "-qm", title)
        return self.git("rev-parse", "HEAD")

    def ledger(self, revision):
        self.write("benchmark/public/results/fixture.json",
                   json.dumps({"quipu_revision": revision}))

    def check(self, head=None, event_name="merge_group"):
        event = self.root / "event.json"
        event.write_text(json.dumps({"merge_group": {
            "head_sha": head or self.git("rev-parse", "HEAD"),
            "base_sha": self.base}}))
        return subprocess.run([sys.executable, str(SCRIPT)], cwd=self.root,
                              env={**os.environ, "GITHUB_EVENT_NAME": event_name,
                                   "GITHUB_EVENT_PATH": str(event)},
                              text=True, capture_output=True)

    def test_correct_combined_tree_passes(self):
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(self.head, result.stdout)

    def test_missing_wrong_short_and_ambiguous_ledgers_refuse(self):
        for revision in ("", "0" * 40, "abc123"):
            with self.subTest(revision=revision):
                self.ledger(revision)
                self.commit("invalid stamp")
                self.assertNotEqual(self.check().returncode, 0)
        self.ledger(self.measured)
        self.write("benchmark/public/results/other.json",
                   json.dumps({"quipu_revision": self.base}))
        self.commit("ambiguous ledger stamps")
        self.assertIn("exactly one", self.check().stdout)

    def test_absent_ledger_directory_refuses(self):
        self.git("rm", "benchmark/public/results/fixture.json")
        self.commit("missing ledger")
        self.assertIn("exactly one", self.check().stdout)

    def test_code_from_another_queue_entry_stales_ledger(self):
        self.write("src/ahead.rs", "// entry ahead changed code\n")
        self.commit("combined code drift")
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("src/ahead.rs", result.stdout)

    def test_narrowing_path_contract_cannot_hide_drift(self):
        self.write("src/lib.rs", "// changed\n")
        self.write("benchmark/public/conformance_report.py", 'CODE_PATHS = ("other",)\n')
        self.commit("narrow paths")
        self.assertIn("src/lib.rs", self.check().stdout)

    def test_wrong_checkout_or_event_refuses(self):
        self.assertIn("not the merge group", self.check(head=self.base).stdout)
        self.assertIn("bound merge_group", self.check(event_name="pull_request").stdout)

    def test_workflow_required_check_binding(self):
        # Parse this small job block without an extra runtime YAML dependency.
        text = (ROOT / ".github/workflows/merge-group-ledger.yml").read_text()
        job = text.split("  merge-group-ledger-provenance:\n", 1)[1]
        for required in ("name: Merge-group ledger provenance",
                         "if: github.event_name == 'merge_group'",
                         "ref: ${{ github.event.merge_group.head_sha }}",
                         "fetch-depth: 0", "contents: read",
                         "python3 scripts/ci/check-merge-group-ledger.py"):
            self.assertIn(required, job)
        self.assertNotIn("continue-on-error", job)
        self.assertNotIn("--report-stale", job)


if __name__ == "__main__":
    unittest.main()
