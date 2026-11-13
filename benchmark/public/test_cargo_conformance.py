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

    def test_required_ci_runs_conformance_on_pr_main_and_queue(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/conformance.yml").read_text()
        ci = (root / ".github/workflows/ci.yml").read_text()
        self.assertIn('  workflow_call:', workflow)
        for event in ('pull_request', 'push', 'merge_group'):
            self.assertRegex(ci, rf'(?m)^  {event}:')
        self.assertIn('    uses: ./.github/workflows/conformance.yml', ci)
        gate = re.search(r'^  release-correctness:\n(.*?)(?=^  [a-z_-]+:|\Z)', ci, re.M | re.S)
        self.assertIsNotNone(gate)
        self.assertRegex(gate.group(1), r'needs: \[[^\]]*\bconformance\b')

    def test_release_uses_explicit_dispatch_and_separate_publication(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        release = (root / '.github/workflows/release.yml').read_text()
        conformance = (root / '.github/workflows/conformance.yml').read_text()
        publisher = (root / '.github/workflows/publish-conformance-release.yml').read_text()
        self.assertIn('gh workflow run conformance.yml --ref main -f "release_tag=$TAG"', release)
        self.assertIn('TAG: ${{ needs.artifacts.outputs.tag }}', release)
        self.assertNotRegex(conformance, r'(?m)^  release:')
        self.assertIn('ref: ${{ inputs.release_tag || github.sha }}', conformance)
        self.assertNotIn('contents: write', conformance)
        self.assertIn('run-id: ${{ github.event.workflow_run.id }}', publisher)
        self.assertIn("github.event.workflow_run.event == 'workflow_dispatch'", publisher)
        self.assertIn('gh api "repos/$GH_REPO/commits/$REF"', publisher)


if __name__ == "__main__":
    unittest.main()
