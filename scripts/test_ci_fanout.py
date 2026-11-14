#!/usr/bin/env python3
"""Real workflow topology plus fail-closed nightly publication controls.

# arming: ci Pre-commit checks; agent-invoked offline, no external writes.
"""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import yaml

ROOT = Path(__file__).resolve().parents[1]


def module(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts/ci' / file)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


night = module('night', 'nightly-result.py')
paths = module('paths', 'pr-surfaces.py')


def workflow(name):
    return yaml.safe_load((ROOT / '.github/workflows' / name).read_text())


class WorkflowContract(unittest.TestCase):
    def test_required_checks_and_quick_controls_remain(self):
        ci = workflow('ci.yml')['jobs']
        self.assertEqual(ci['fmt']['name'], 'Format')
        for job, prefix in [('clippy', 'Clippy'), ('test', 'Test')]:
            self.assertEqual(ci[job]['name'], prefix + ' (${{ matrix.name }})')
            self.assertEqual([entry['name'] for entry in ci[job]['strategy']['matrix']['include']],
                             ['default', 'shacl'])
        self.assertEqual(ci['build']['name'], 'Build')
        self.assertEqual(ci['release-correctness']['name'], 'Release correctness')
        for job in ['generated-docs', 'source-size', 'shacl-write-path']:
            self.assertNotIn('if', ci[job])
        self.assertIn('generated-docs', ci['release-correctness']['needs'])
        self.assertIn('python3 scripts/test_ci_fanout.py', json.dumps(ci['check']))
        for job in ['query-builders', 'check', 'repository-pendant-source', 'lint-markdown']:
            self.assertNotIn("github.event_name == 'schedule'", ci[job].get('if', ''))
        for file in ['signing-core.yml', 'install.yml', 'pendant-review.yml', 'changelog-check.yml']:
            doc = workflow(file)
            self.assertIn('pull_request', doc.get('on', doc.get(True)))

    def test_heavy_nightly_only_and_single_schedule(self):
        ci = workflow('ci.yml')['jobs']
        condition = "${{ github.event_name == 'schedule' || github.event_name == 'workflow_dispatch' }}"
        for job in ['bundled-runtime', 'extended-features', 'build-full', 'load-test', 'query-perf']:
            self.assertEqual(ci[job]['if'], condition)
        for file in ['python-bindings.yml', 'merge-load.yml', 'conformance.yml']:
            trigger = workflow(file).get('on', workflow(file).get(True))
            self.assertIn('workflow_call', trigger)
            self.assertNotIn('schedule', trigger)
        self.assertEqual(set(ci['nightly-gate']['needs']), night.SUITES)
        self.assertIn('always()', ci['nightly-gate']['if'])
        self.assertEqual(ci['nightly-publish']['needs'], 'nightly-gate')
        self.assertEqual(ci['nightly-publish']['if'],
                         "github.event_name == 'schedule' && github.ref == 'refs/heads/main'")
        self.assertIn('always()', ci['nightly-failure']['if'])
        self.assertEqual(set(ci['nightly-failure']['needs']), {'nightly-gate', 'nightly-publish'})
        self.assertNotIn('pull_request', workflow('merge-load.yml').get(True, {}))
        conformance = workflow('conformance.yml')
        self.assertEqual(conformance['jobs']['re-run-conformance']['if'],
                         "inputs.rederive == true || github.event_name == 'workflow_dispatch'")
        self.assertIn('rederive', ci['conformance']['with'])

    def test_path_specific_checks(self):
        ci = workflow('ci.yml')['jobs']
        for job in ['wasm', 'wasm-explorer']:
            self.assertEqual(ci[job]['needs'], 'pr-paths')
            self.assertIn('!cancelled()', ci[job]['if'])  # nightly selector is skipped
            self.assertIn("needs.pr-paths.outputs.wasm == 'true'", ci[job]['if'])
        for file, included, excluded in [
            ('python-bindings.yml', ['python/**', 'bindings/python/**'], ['src/**', 'Cargo.toml']),
            ('docs.yml', ['docs/**'], ['src/**', 'Cargo.toml']),
        ]:
            triggers = workflow(file).get('on', workflow(file).get(True))
            rules = triggers['pull_request']['paths']
            for item in included:
                self.assertIn(item, rules)
            for item in excluded:
                self.assertNotIn(item, rules)
        self.assertFalse(paths.wasm_changed(['src/query.rs', 'README.md']))
        for file in ['wasm/harness/src/lib.rs', 'wasm/explorer/Cargo.lock', 'Cargo.lock',
                     '.github/actions/wasm-explorer-smoke/action.yml', 'scripts/smoke-wasm.mjs']:
            self.assertTrue(paths.wasm_changed([file]), file)
        result = subprocess.run(['python3', str(ROOT / 'scripts/ci/pr-surfaces.py')],
                                env={'PATH': '/usr/bin:/bin'}, capture_output=True, text=True)
        self.assertEqual(result.stdout, 'wasm=true\n')  # missing evidence keeps coverage
        for error in [OSError('missing git'), subprocess.TimeoutExpired('git', 30),
                      UnicodeError('uncertain path encoding')]:
            with patch.dict(os.environ, {'PR_BASE': 'a' * 40, 'PR_HEAD': 'b' * 40}), \
                    patch.object(paths.subprocess, 'run', side_effect=error), \
                    patch('sys.stdout', new_callable=io.StringIO) as output:
                paths.main()
                self.assertEqual(output.getvalue(), 'wasm=true\n')


class Publication(unittest.TestCase):
    def setUp(self):
        self.env = {'GITHUB_SHA': 'a' * 40, 'GITHUB_REPOSITORY': 'example/repo',
                    'GITHUB_REF': 'refs/heads/main', 'GITHUB_EVENT_NAME': 'schedule',
                    'GITHUB_RUN_ID': '123', 'GATE_RESULT': 'success', 'GATE_SHA': 'a' * 40}

    def archive(self, root, sha=None):
        path = root / night.ASSET
        with tarfile.open(path, 'w:gz') as output:
            for name, data in [('quipu', b'fixture-cli'), ('quipu-server', b'fixture-server'),
                               ('LICENSE', b'fixture-license'), ('FEATURES.txt', b'experimental'),
                               ('SOURCE_SHA', (sha or self.env['GITHUB_SHA']).encode())]:
                member = tarfile.TarInfo('./' + name)
                member.size = len(data)
                output.addfile(member, io.BytesIO(data))
        root.joinpath(night.ASSET + '.sha256').write_text(
            hashlib.sha256(path.read_bytes()).hexdigest() + '  ' + night.ASSET + '\n')

    def test_each_non_green_state_and_missing_suite_refuses(self):
        needs = {name: {'result': 'success'} for name in night.SUITES}
        self.assertTrue(night.green(needs))
        for name in night.SUITES:
            for verdict in ['failure', 'skipped', 'cancelled', '', 'pending']:
                broken = copy.deepcopy(needs)
                broken[name]['result'] = verdict
                self.assertFalse(night.green(broken), (name, verdict))
            broken = copy.deepcopy(needs)
            del broken[name]
            self.assertFalse(night.green(broken))
        self.assertFalse(night.green({}))

    def test_wrong_event_branch_or_sha_never_calls_release(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(night, 'gh') as gh:
            root = Path(directory)
            self.archive(root)
            for field, value in [('GITHUB_EVENT_NAME', 'pull_request'),
                                 ('GITHUB_EVENT_NAME', 'workflow_dispatch'),
                                 ('GITHUB_REF', 'refs/heads/topic'), ('GATE_RESULT', 'skipped'),
                                 ('GATE_RESULT', 'failure'), ('GATE_SHA', 'b' * 40)]:
                with self.assertRaises(ValueError):
                    night.publish(root, {**self.env, field: value})
            self.archive(root, 'b' * 40)
            with self.assertRaises(ValueError):
                night.publish(root, self.env)
            self.archive(root)
            root.joinpath(night.ASSET).write_bytes(b'tampered')
            with self.assertRaises(ValueError):
                night.publish(root, self.env)
            gh.assert_not_called()

    def test_green_exact_package_creates_only_experimental_release(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(night, 'gh') as gh:
            root = Path(directory)
            self.archive(root)
            gh.side_effect = ['[]', '']
            night.publish(root, self.env)
            args = gh.call_args.args
            self.assertEqual(args[:3], ('release', 'create', 'nightly-123-' + 'a' * 40))
            self.assertIn('--prerelease', args)
            self.assertIn('--latest=false', args)
            self.assertEqual(args[args.index('--target') + 1], 'a' * 40)

    def test_existing_release_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(night, 'gh') as gh:
            root = Path(directory)
            self.archive(root)
            gh.return_value = json.dumps([{'tag_name': 'nightly-123-' + 'a' * 40,
                                          'prerelease': False, 'target_commitish': 'b' * 40}])
            with self.assertRaises(ValueError):
                night.publish(root, self.env)
            self.assertEqual(gh.call_count, 1)

    def test_duplicate_trackers_refuse_a_third(self):
        with patch.object(night, 'gh') as gh:
            gh.return_value = json.dumps([{'number': number, 'title': night.TITLE, 'state': 'OPEN'}
                                         for number in [1, 2]])
            with self.assertRaises(ValueError):
                night.track({**self.env, 'NEEDS_JSON': '{}'})
            self.assertEqual(gh.call_count, 1)

    def test_one_tracker_created_or_updated_including_closed(self):
        for existing in [[], [{'number': 7, 'title': night.TITLE, 'state': 'OPEN'}],
                         [{'number': 7, 'title': night.TITLE, 'state': 'CLOSED'}]]:
            with patch.object(night, 'gh') as gh:
                gh.side_effect = [json.dumps(existing)] + [''] * 3
                night.track({**self.env, 'NEEDS_JSON': '{"nightly-gate":{"result":"failure"}}'})
                commands = [call.args[:2] for call in gh.call_args_list]
                self.assertEqual(('issue', 'create') in commands, not existing)
                self.assertEqual(('issue', 'edit') in commands, bool(existing))
                self.assertEqual(('issue', 'reopen') in commands,
                                 bool(existing and existing[0]['state'] == 'CLOSED'))


if __name__ == '__main__':
    unittest.main()
