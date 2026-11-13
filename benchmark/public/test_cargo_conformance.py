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

    def test_publisher_actual_condition_refuses_nondefault_branches(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        source = (root / '.github/workflows/publish-conformance-release.yml').read_text()
        folded = ' '.join(source.split('    if: >-\n', 1)[1].split('    runs-on:', 1)[0].split())
        condition = folded.removeprefix('${{').removesuffix('}}').replace('&&', ' and ')
        trusted = {'github.event.workflow_run.conclusion': 'success',
                   'github.event.workflow_run.event': 'workflow_dispatch',
                   'github.event.workflow_run.head_repository.full_name': 'intigraph/quipu',
                   'github.event.workflow_run.head_branch': 'main',
                   'github.event.repository.default_branch': 'main',
                   'github.repository': 'intigraph/quipu'}
        cases = [({}, True), ({'github.event.workflow_run.head_branch': 'feature-unreviewed'}, False),
                 ({'github.event.workflow_run.head_repository.full_name': 'foreign/quipu'}, False),
                 ({'github.event.workflow_run.event': 'pull_request'}, False),
                 ({'github.event.workflow_run.conclusion': 'failure'}, False)]
        for changes, expected in cases:
            with self.subTest(changes=changes):
                expression = condition
                values = dict(trusted, **changes)
                for key in sorted(values, key=len, reverse=True):
                    expression = expression.replace(key, repr(values[key]))
                # Evaluate the checked-in boolean condition, with fixture values
                # quoted as literals and no builtins, rather than a second policy.
                self.assertIs(eval(expression, {'__builtins__': {}}), expected)


if __name__ == "__main__":
    unittest.main()
