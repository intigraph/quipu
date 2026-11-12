"""Cargo-only changes must reach and invalidate the conformance ledger gate."""
import contextlib
import pathlib
import re
import unittest

import test_conformance_report as report_tests


class CargoConformanceTest(unittest.TestCase):
    def test_cargo_changes_invalidate_both_strict_and_pr_provenance(self):
        probe = report_tests.LedgerProvenancePrModeTest()
        for filename in ("Cargo.toml", "Cargo.lock"):
            with self.subTest(filename=filename), contextlib.ExitStack() as stack:
                root, run, base = probe._repo(stack)
                run("checkout", "-qb", "pr")
                (root / filename).write_text('# changed build input\n')
                run("add", "-A")
                run("commit", "-qm", "change Cargo input")
                for pr_base in (None, "main"):
                    code, messages = probe._provenance(root, {"quipu_revision": base}, pr_base)
                    self.assertEqual(code, 1, (filename, pr_base, messages))
                    self.assertIn(filename, " ".join(messages))

    def test_docs_only_change_keeps_provenance_fresh(self):
        probe = report_tests.LedgerProvenancePrModeTest()
        with contextlib.ExitStack() as stack:
            root, run, base = probe._repo(stack)
            (root / "README.md").write_text("documentation\n")
            run("add", "-A")
            run("commit", "-qm", "documentation only")
            for pr_base in (None, base):
                code, messages = probe._provenance(root, {"quipu_revision": base}, pr_base)
                self.assertEqual(code, 0, messages)

    def test_required_ci_calls_blocking_conformance_for_pr_push_and_queue(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        ci = (root / ".github/workflows/ci.yml").read_text()
        for event in ("pull_request", "push", "merge_group"):
            section = re.search(rf"^  {event}:\n(.*?)(?=^  [a-z_]+:|\Z)", ci, re.M | re.S)
            self.assertIsNotNone(section)
            self.assertNotIn("paths:", section.group(1), "all source inputs must reach the gate")
        self.assertIn("uses: ./.github/workflows/conformance.yml", ci)
        required = ci.split("  release-correctness:", 1)[1].split("  extended-correctness:", 1)[0]
        self.assertRegex(required, r"needs: \[[^\]]*conformance")
        workflow = (root / ".github/workflows/conformance.yml").read_text()
        self.assertIn("  workflow_call:", workflow)
        provenance = workflow.split("  ledger-provenance:", 1)[1].split("  re-run-conformance:", 1)[0]
        self.assertIn("--arm provenance --pr-base \"\"", provenance)
        self.assertNotIn("--report-stale", provenance)
        self.assertIn("github.event_name == 'merge_group'", provenance)


if __name__ == "__main__":
    unittest.main()
