#!/usr/bin/env python3
"""The declared change-class map and its classifier/gate (aegis-659).

Run directly, and from scripts/test_ci_fanout.py so the CI Pre-commit checks
job exercises it without a workflow edit.
"""
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch
import yaml

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location('change_class', ROOT / 'scripts/ci/change-class.py')
cc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cc)

# Every ci.yml job is classified exactly once: in the map, nightly-only, or an
# aggregate/selector. A new job that is none of these fails here.
NIGHTLY = {'bundled-runtime', 'extended-features', 'build-full', 'load-test', 'query-perf',
           'extended-correctness', 'nightly-wheels', 'nightly-merge-load', 'nightly-gate',
           'nightly-publish', 'nightly-failure'}
AGGREGATES = {'pr-paths', 'release-correctness', 'quipu-required'}
CARGO_BUMP = '--- a/Cargo.toml\n+++ b/Cargo.toml\n@@ -3 +3 @@\n-version = "0.19.5"\n+version = "0.19.6"\n'


def workflow_jobs(stem):
    return yaml.safe_load((ROOT / '.github/workflows' / f'{stem}.yml').read_text())['jobs']


class MapContract(unittest.TestCase):
    def setUp(self):
        self.cmap = cc.load_map()

    def test_every_job_id_exists_in_its_workflow(self):
        for job in self.cmap['jobs']:
            stem, name = job.split(':', 1)
            self.assertIn(name, workflow_jobs(stem), job)

    def test_every_gating_ci_job_is_in_the_map(self):
        mapped = {j.split(':', 1)[1] for j in self.cmap['jobs'] if j.startswith('ci:')}
        unclassified = set(workflow_jobs('ci')) - mapped - NIGHTLY
        self.assertLessEqual(unclassified, AGGREGATES)
        self.assertFalse(mapped & (NIGHTLY | AGGREGATES))
        for name in NIGHTLY:
            self.assertIn("github.event_name == 'schedule'", workflow_jobs('ci')[name]['if'], name)

    def test_bypass_is_the_reasoned_complement_of_required(self):
        jobs = set(self.cmap['jobs'])
        self.assertEqual(self.cmap['classes']['full']['required'], 'all')
        self.assertEqual(self.cmap['classes']['full']['bypass'], {})
        for name, cls in self.cmap['classes'].items():
            if name == 'full':
                continue
            required = set(cls['required'])
            self.assertLessEqual(required, jobs, name)
            self.assertEqual(set(cls['bypass']), jobs - required, name)
            for job, reason in cls['bypass'].items():
                self.assertTrue(reason and reason.strip(), (name, job))
            self.assertIn('ci:check', required, name)  # pre-commit always runs

    def test_rule_classes_are_declared(self):
        for rule in self.cmap['rules']:
            self.assertIn(rule['class'], self.cmap['classes'])
        self.assertEqual(self.cmap['rules'][0]['class'], 'full')


class Classify(unittest.TestCase):
    def setUp(self):
        self.cmap = cc.load_map()

    def classes(self, paths, **kw):
        return cc.classify(self.cmap, paths, **kw)['classes']

    def test_sensitive_and_unmatched_paths_are_full(self):
        for path in ['src/lib.rs', 'crates/x/src/lib.rs', 'Cargo.toml', 'Cargo.lock',
                     'shapes/a.ttl', '.github/workflows/ci.yml', 'scripts/ci/change-class.py',
                     'ci/change-classes.yml', 'build.rs', 'flake.lock', 'justfile',
                     'scripts/anything.sh', 'skills/quipu/SKILL.md', 'src/README.md']:
            self.assertEqual(self.classes([path]), ['full'], path)

    def test_cheap_classes(self):
        self.assertEqual(self.classes(['docs/book/src/a.md', 'README.md']), ['docs'])
        self.assertEqual(self.classes(['CHANGELOG.md']), ['changelog'])
        self.assertEqual(self.classes(['tests/a.rs', 'test-fixtures/x.ttl']), ['test'])
        self.assertEqual(self.classes(['wasm/harness/src/lib.rs']), ['wasm'])

    def test_any_full_path_makes_the_diff_full(self):
        result = cc.classify(self.cmap, ['README.md', 'src/lib.rs'])
        self.assertTrue(result['full'])
        self.assertEqual(result['required'], self.cmap['jobs'])
        self.assertEqual(result['bypassed'], [])

    def test_multi_class_is_the_union(self):
        result = cc.classify(self.cmap, ['tests/a.rs', 'docs/a.md'])
        self.assertEqual(result['classes'], ['docs', 'test'])
        want = set(self.cmap['classes']['docs']['required']) | set(self.cmap['classes']['test']['required'])
        self.assertEqual(set(result['required']), want)

    def test_empty_diff_is_full(self):
        self.assertEqual(self.classes([]), ['full'])
        self.assertEqual(self.classes(['']), ['full'])

    def test_release_needs_branch_paths_and_version_only_manifests(self):
        paths = ['CHANGELOG.md', 'Cargo.toml', 'Cargo.lock']
        ref = 'release-plz-2026-11-26T17-26-44Z'
        self.assertEqual(self.classes(paths, head_ref=ref, manifest_diff=CARGO_BUMP), ['release'])
        self.assertEqual(self.classes(paths, head_ref='feature', manifest_diff=CARGO_BUMP), ['full'])
        self.assertEqual(self.classes(paths + ['src/lib.rs'], head_ref=ref, manifest_diff=CARGO_BUMP),
                         ['full'])
        self.assertEqual(self.classes(paths, head_ref=ref, manifest_diff=None), ['full'])
        for bad in ['-tokio = "1.40.0"\n+tokio = "1.41.0"\n',
                    '-version = "1.0.0"\n+version = "1.0.1"\n-checksum = "aa"\n+checksum = "bb"\n',
                    '+version = "1.0.1"\n',
                    '-serde = { version = "1.0.1" }\n+serde = { version = "1.0.2" }\n']:
            self.assertEqual(self.classes(paths, head_ref=ref, manifest_diff=bad), ['full'], bad)
        inline = '-quipu-ai = { path = "../..", version = "0.19.5" }\n' \
                 '+quipu-ai = { path = "../..", version = "0.19.6" }\n'
        self.assertEqual(self.classes(paths, head_ref=ref, manifest_diff=inline), ['release'])
        # A changelog-only release-plz PR is still release, not merely changelog.
        self.assertEqual(self.classes(['CHANGELOG.md'], head_ref=ref, manifest_diff=''), ['release'])


class Events(unittest.TestCase):
    def test_each_event_defers_only_what_the_other_event_runs(self):
        cmap = cc.load_map()
        full = cc.classify(cmap, ['src/lib.rs'])
        pr = cc.for_event(cmap, full, 'pull_request')['required']
        queue = cc.for_event(cmap, full, 'merge_group')['required']
        self.assertNotIn('ci:wasm', pr)
        self.assertIn('ci:wasm', queue)
        self.assertNotIn('ci:lint-markdown', queue)
        self.assertIn('ci:lint-markdown', pr)
        self.assertEqual(set(pr) | set(queue), set(cmap['jobs']))
        self.assertEqual(cc.for_event(cmap, full, 'push'), full)

    def test_deferred_ids_match_the_workflow_conditions(self):
        ci = workflow_jobs('ci')
        for job in ['wasm', 'wasm-explorer']:
            self.assertIn("github.event_name == 'merge_group'", ci[job]['if'])
            self.assertNotIn('pull_request', ci[job]['if'])
        self.assertIn('merge_group', ci['lint-markdown']['if'])


class Evidence(unittest.TestCase):
    def run_main(self, argv, env=None):
        with patch.dict(os.environ, env or {}, clear=False), \
                patch('sys.stdout', new_callable=io.StringIO) as out:
            code = cc.main(argv)
        return code, out.getvalue()

    def test_missing_or_bad_shas_are_full(self):
        for base, head in [('', ''), ('main', 'HEAD'), ('a' * 40, '')]:
            _, out = self.run_main(['--base', base, '--head', head, '--json'])
            self.assertTrue(json.loads(out)['full'], (base, head))

    def test_git_errors_are_full(self):
        for error in [OSError('missing git'), subprocess.TimeoutExpired('git', 30),
                      UnicodeError('uncertain path encoding')]:
            with patch.object(cc.subprocess, 'run', side_effect=error):
                _, out = self.run_main(['--base', 'a' * 40, '--head', 'b' * 40, '--json'])
            self.assertTrue(json.loads(out)['full'], error)

    def test_unreadable_map_gates_everything(self):
        with patch.object(cc, 'load_map', side_effect=OSError('gone')):
            _, out = self.run_main(['--paths', 'README.md'])
        self.assertIn('full=true\n', out)
        self.assertIn('required=all\n', out)

    def test_github_output_lines(self):
        _, out = self.run_main(['--paths', 'docs/a.md'])
        self.assertIn('classes=docs\n', out)
        self.assertIn('ci_lint_markdown=true\n', out)
        self.assertIn('ci_test=false\n', out)
        self.assertIn('nix_build=false\n', out)

    def test_real_git_diff(self):
        head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True)
        if head.returncode != 0:
            self.skipTest('no git checkout')
        sha = head.stdout.strip()
        _, out = self.run_main(['--base', sha, '--head', sha, '--json'])
        self.assertTrue(json.loads(out)['full'])  # identical refs: empty diff is no free pass


class Gate(unittest.TestCase):
    def needs(self, **results):
        return {name.replace('_', '-'): {'result': r, 'outputs': {}} for name, r in results.items()}

    def test_skipped_and_not_required_is_fine(self):
        needs = self.needs(check='success', test='skipped', wasm='skipped')
        self.assertEqual(cc.gate(needs, 'ci', ['ci:check', 'nix:build']), [])

    def test_skipped_but_required_refuses(self):
        refusals = cc.gate(self.needs(check='success', test='skipped'), 'ci', ['ci:check', 'ci:test'])
        self.assertEqual(len(refusals), 1)
        self.assertIn('test: skipped', refusals[0])

    def test_failure_refuses_even_when_not_required(self):
        refusals = cc.gate(self.needs(check='success', test='failure'), 'ci', ['ci:check'])
        self.assertEqual(len(refusals), 1)
        self.assertIn('did not succeed', refusals[0])
        self.assertTrue(cc.gate(self.needs(check='cancelled'), 'ci', []))

    def test_all_means_every_need(self):
        self.assertTrue(cc.gate(self.needs(check='success', test='skipped'), 'ci', ['all']))
        self.assertEqual(cc.gate(self.needs(check='success'), 'ci', ['all']), [])

    def test_required_job_absent_from_needs_refuses(self):
        self.assertTrue(cc.gate(self.needs(check='success'), 'ci', ['ci:check', 'ci:build']))

    def test_cli_gate(self):
        env = {'NEEDS_JSON': json.dumps(self.needs(check='success', test='skipped'))}
        code, _ = Evidence.run_main(self, ['gate', '--workflow', 'ci', '--required', 'ci:check'], env)
        self.assertEqual(code, 0)
        code, out = Evidence.run_main(self, ['gate', '--workflow', 'ci', '--required', 'all'], env)
        self.assertEqual(code, 1)
        self.assertIn('refused: test', out)
        code, _ = Evidence.run_main(self, ['gate', '--workflow', 'ci', '--required', 'all'],
                                    {'NEEDS_JSON': 'not json'})
        self.assertEqual(code, 1)


class Lane(unittest.TestCase):
    def setUp(self):
        self.cmap = cc.load_map()
        self.prefixes = cc.check_name_prefixes(self.cmap)

    def test_prefixes_are_the_displayed_check_names(self):
        self.assertEqual(self.prefixes['ci:test'], 'Test')
        self.assertEqual(self.prefixes['ci:fmt'], 'Format')
        self.assertEqual(self.prefixes['ci:lint-markdown'], 'Markdown lint')

    def test_docs_pr_skips_are_green_but_required_skips_are_not(self):
        docs = cc.classify(self.cmap, ['docs/a.md'])
        checks = {'Test (default)': 'skipped', 'Wasm (browser harness)': 'skipped',
                  'Pre-commit checks': 'success', 'Markdown lint': 'success'}
        self.assertEqual(cc.lane_skipped_is_green(checks, docs, self.prefixes), [])
        checks['Markdown lint'] = 'skipped'
        self.assertEqual(cc.lane_skipped_is_green(checks, docs, self.prefixes),
                         ['Markdown lint: skipped'])

    def test_full_unknown_and_failed_checks_block(self):
        full = cc.classify(self.cmap, ['src/lib.rs'])
        docs = cc.classify(self.cmap, ['docs/a.md'])
        self.assertTrue(cc.lane_skipped_is_green({'Test (default)': 'skipped'}, full, self.prefixes))
        self.assertTrue(cc.lane_skipped_is_green({'Some new check': 'skipped'}, docs, self.prefixes))
        self.assertTrue(cc.lane_skipped_is_green({'Test (default)': 'failure'}, docs, self.prefixes))


if __name__ == '__main__':
    unittest.main()
