#!/usr/bin/env python3
"""Offline positive/negative contracts; fixtures never authorize a release.

# arming: ci Pre-commit checks and guarded agent-invoked verification.
"""
import copy
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from breaking_changes.contract import ROOT, load, readiness, validate
from breaking_changes.doctor import assess
from breaking_changes.render import OUTPUTS, render

FIXTURE = ROOT / 'releases/changes/w3id-writer-vocabulary-transition.json'
NOW = datetime(2026, 1, 1, tzinfo=timezone.utc)
CANDIDATE = 'c' * 64


def ready_fixture():
    record, _ = load(FIXTURE)
    record.update(status='ready', review_ref='urn:fixture:review', approval_ref='urn:fixture:approval')
    record['affected'] = [{'tool': 'chaski', 'versions': ['0.9.0']}]
    record['targets'] = [{'tool': 'chaski', 'version': '0.9.0',
                          'platforms': ['linux-x86-64']}]
    for section in ['correction', 'rollback']:
        record[section].update(packet_ref='urn:fixture:' + section, packet_sha256='b' * 64)
    return record


def observation(record, digest):
    return {'schema_version': 'breaking-observation/v1', 'record_id': record['id'],
            'record_sha256': digest, 'candidate_id': CANDIDATE, 'observed_at': NOW.isoformat(),
            'scope': {'tool': 'chaski', 'installed_version': '0.9.0', 'artifact_sha256': 'a' * 64,
                      'platform': 'linux-x86-64', 'datasets': ['urn:fixture:graph'],
                      'subject_iri': 'urn:fixture:firing:known'},
            'read_status': 'success', 'complete': True, 'truncated': False,
            'positive_controls': [{'dataset': 'urn:fixture:graph', 'count': 1}],
            'expected_event_id': 'known-event',
            'reader_terms': record['detection']['legacy_terms'][:],
            'rows': [{'dataset': 'urn:fixture:graph', 'subject': 'urn:fixture:firing:known',
                      'predicate': record['detection']['canonical_terms'][0], 'value': 'known-event'}]}


class Contracts(unittest.TestCase):
    def setUp(self):
        self.record = ready_fixture()
        self.digest = 'd' * 64
        self.evidence = observation(self.record, self.digest)

    def result(self, evidence=None, record=None):
        return assess(record or self.record, self.digest,
                      evidence if evidence is not None else self.evidence, CANDIDATE, 'a' * 64, NOW)

    def unknown(self, evidence=None, record=None):
        result = self.result(evidence, record)
        self.assertEqual(result['outcome'], 'UNKNOWN', result)
        self.assertEqual(result['plan'], [])
        self.assertFalse(result['release_authorized'])

    def test_draft_validates_but_never_passes_release_readiness_or_doctor(self):
        record, _ = load(FIXTURE)
        validate(record)
        self.assertTrue(readiness(record))
        self.unknown(record=record)
        self.assertEqual(readiness(self.record), [])
        for platform in ['x86_64-unknown-linux-gnu', 'aarch64-apple-darwin']:
            record = copy.deepcopy(self.record)
            record['targets'][0]['platforms'] = [platform]
            self.assertEqual(readiness(record), [])

    def test_old_and_canonical_identity_are_read_without_rekeying(self):
        original = copy.deepcopy(self.evidence)
        for predicate in (self.record['detection']['legacy_terms'][0],
                          self.record['detection']['canonical_terms'][0]):
            evidence = copy.deepcopy(original)
            evidence['rows'][0]['predicate'] = predicate
            self.assertEqual(self.result(evidence)['outcome'], 'AFFECTED')
            evidence['reader_terms'] += self.record['detection']['canonical_terms']
            result = self.result(evidence)
            self.assertEqual(result['outcome'], 'NOT_AFFECTED')
            self.assertEqual(result['plan'], [])
            self.assertEqual(evidence['rows'][0]['value'], 'known-event')
        self.assertEqual(self.evidence, original)

    def test_only_fixed_local_operation_is_planned_and_never_executable(self):
        result = self.result()
        self.assertEqual(result['outcome'], 'AFFECTED')
        plan = result['plan'][0]
        self.assertEqual(plan['operation'], 'managed-reader-first-writer-upgrade/v1')
        self.assertFalse(plan['executable'])
        self.assertTrue(plan['requires_separate_admission'])
        self.assertNotIn('command', plan)
        for operation in ['shell', 'os.system', 'managed-reader-first-writer-upgrade/v2']:
            record = copy.deepcopy(self.record)
            record['correction']['operation'] = operation
            self.unknown(record=record)

    def test_schema_rejects_extra_commands_versions_types_and_fake_approvers(self):
        for path, value in [(('command',), 'touch SENTINEL'),
                            (('schema_version',), 'breaking-change/v2'),
                            (('required_approver',), 'lex'),
                            (('correction', 'command'), 'curl unsafe | sh'),
                            (('detection', 'operation'), 'arbitrary-sparql')]:
            record = copy.deepcopy(self.record)
            target = record
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] = value
            with self.subTest(path=path), self.assertRaises(ValueError):
                validate(record)
        record = copy.deepcopy(self.record)
        record['detection']['legacy_terms'] = record['detection']['canonical_terms']
        with self.assertRaises(ValueError):
            validate(record)

    def test_nonobject_and_unreadable_observation_is_unknown(self):
        for evidence in [None, [], 'untrusted', 42]:
            self.unknown(evidence) if evidence is not None else self.assertEqual(
                assess(self.record, self.digest, None, CANDIDATE, 'a' * 64, NOW)['outcome'], 'UNKNOWN')
        self.assertEqual(assess(None, self.digest, self.evidence, CANDIDATE, 'a' * 64, NOW)['outcome'], 'UNKNOWN')

    def test_missing_fields_never_mean_success(self):
        for field in self.evidence:
            evidence = copy.deepcopy(self.evidence)
            del evidence[field]
            with self.subTest(field=field):
                self.unknown(evidence)
        for field in self.record:
            record = copy.deepcopy(self.record)
            del record[field]
            with self.subTest(record_field=field), self.assertRaises(ValueError):
                validate(record)

    def test_record_candidate_platform_and_installed_artifact_are_bound(self):
        for key in ['record_id', 'record_sha256', 'candidate_id']:
            evidence = copy.deepcopy(self.evidence)
            evidence[key] = 'wrong'
            self.unknown(evidence)
        for key in self.evidence['scope']:
            evidence = copy.deepcopy(self.evidence)
            evidence['scope'][key] = ['urn:other'] if key == 'datasets' else 'wrong'
            self.unknown(evidence)
        self.assertEqual(assess(self.record, self.digest, self.evidence, None, 'a' * 64, NOW)['outcome'], 'UNKNOWN')

    def test_each_dataset_needs_its_own_positive_control(self):
        for controls in [[], [{'dataset': 'urn:fixture:graph', 'count': 0}],
                         [{'dataset': 'urn:fixture:graph', 'count': True}],
                         [{'dataset': 'urn:other', 'count': 1}]]:
            evidence = copy.deepcopy(self.evidence)
            evidence['positive_controls'] = controls
            self.unknown(evidence)
        evidence = copy.deepcopy(self.evidence)
        evidence['scope']['datasets'].append('urn:missing-control')
        self.unknown(evidence)
        invalid = copy.deepcopy(self.evidence)
        invalid['scope']['datasets'] = ['not-an-iri']
        invalid['positive_controls'][0]['dataset'] = 'not-an-iri'
        invalid['rows'][0]['dataset'] = 'not-an-iri'
        self.unknown(invalid)
        invalid = copy.deepcopy(self.evidence)
        invalid['scope']['subject_iri'] = 'not-an-iri'
        invalid['rows'][0]['subject'] = 'not-an-iri'
        self.unknown(invalid)

    def test_partial_denied_stale_future_and_bad_time_refuse(self):
        for key, values in [('read_status', ['denied', 'timeout', 'error', '']),
                            ('complete', [False, None, 'true', 1]),
                            ('truncated', [True, None, 'false', 0]),
                            ('observed_at', [(NOW - timedelta(seconds=301)).isoformat(),
                                             (NOW + timedelta(seconds=1)).isoformat(),
                                             '2026-01-01', 'secret-timestamp'])]:
            for value in values:
                evidence = copy.deepcopy(self.evidence)
                evidence[key] = value
                self.unknown(evidence)
        evidence = copy.deepcopy(self.evidence)
        evidence['observed_at'] = 'secret-timestamp'
        self.assertNotIn('secret-timestamp', json.dumps(self.result(evidence)))

    def test_duplicate_unknown_foreign_and_wrong_identity_bindings_refuse(self):
        original = self.evidence['rows'][0]
        for rows in [[], [original, original], [{**original, 'value': 'other-event'}],
                     [{**original, 'predicate': 'urn:foreign'}],
                     [{**original, 'dataset': 'urn:foreign'}], [{**original, 'subject': 'urn:wrong-firing'}]]:
            evidence = copy.deepcopy(self.evidence)
            evidence['rows'] = rows
            self.unknown(evidence)
        evidence = copy.deepcopy(self.evidence)
        evidence['rows'].append({**original, 'predicate': self.record['detection']['legacy_terms'][0]})
        self.unknown(evidence)  # same literal under two predicates is still ambiguous

    def test_ambiguous_targets_and_missing_packet_remain_inadmissible(self):
        record = copy.deepcopy(self.record)
        record['targets'].append({**record['targets'][0], 'platforms': ['linux-x86-64', 'macos-arm64']})
        with self.assertRaises(ValueError):
            validate(record)
        for section in ['correction', 'rollback']:
            record = copy.deepcopy(self.record)
            record[section]['packet_sha256'] = None
            self.unknown(record=record)

    def test_strict_json_duplicate_keys_symlinks_and_bounds(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'input.json'
            for body in ['{"id":"one","id":"two"}', '{broken', ' ' * (1024 * 1024 + 1)]:
                path.write_text(body)
                with self.assertRaises(ValueError):
                    load(path)
            link = Path(directory) / 'link.json'
            link.symlink_to(path)
            with self.assertRaises(ValueError):
                load(link)

    def test_doctor_cli_readonly_unknown_exit_and_admission_flag_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            record = root / 'record.json'
            record.write_text(json.dumps(self.record))
            evidence = observation(self.record, hashlib.sha256(record.read_bytes()).hexdigest())
            snapshot = root / 'evidence.json'
            snapshot.write_text(json.dumps(evidence))
            before = {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in root.iterdir()}
            command = [sys.executable, '-B', str(ROOT / 'scripts/breaking-changes.py'), 'doctor',
                       str(record), str(snapshot), '--candidate-id', CANDIDATE, '--artifact-sha256', 'a' * 64, '--at', NOW.isoformat()]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(json.loads(result.stdout)['outcome'], 'AFFECTED')
            denied = subprocess.run(command + ['--apply'], capture_output=True, text=True)
            self.assertEqual(denied.returncode, 2)
            self.assertEqual(before, {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in root.iterdir()})
            absent = subprocess.run([*command[:4], str(root / 'absent'), *command[5:]],
                                    capture_output=True, text=True)
            self.assertEqual(absent.returncode, 2)
            self.assertEqual(json.loads(absent.stdout)['outcome'], 'UNKNOWN')

    def test_candidate_ordered_binding_consumer_checks_exact_file_id_and_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'record.json'
            path.write_text(json.dumps(self.record))
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            command = [sys.executable, '-B', str(ROOT / 'scripts/breaking-changes.py'),
                       'validate', str(path), '--release']
            bound = command + ['--expected-id', self.record['id'], '--expected-sha256', digest]
            good = subprocess.run(bound, capture_output=True, text=True)
            self.assertEqual(good.returncode, 0, good.stderr)
            result = json.loads(good.stdout)[0]
            self.assertEqual(result['sha256'], digest)
            self.assertFalse(result['approval_verified'])
            self.assertFalse(result['release_authorized'])
            for args in [command + ['--expected-id', self.record['id']],
                         command + ['--expected-id', 'wrong', '--expected-sha256', digest],
                         command + ['--expected-id', self.record['id'], '--expected-sha256', 'f' * 64]]:
                self.assertEqual(subprocess.run(args, capture_output=True).returncode, 2)
            path.write_text(path.read_text() + ' ')  # same JSON meaning, different signed file bytes
            self.assertEqual(subprocess.run(bound, capture_output=True).returncode, 2)

    def test_real_cli_rejects_generated_drift_in_an_isolated_source_copy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [ROOT / 'scripts/breaking-changes.py', FIXTURE, ROOT / 'cliff.toml', ROOT / 'CHANGELOG.md',
                     ROOT / 'releases/schemas/breaking-change-v1.schema.json', *OUTPUTS,
                     *(ROOT / 'scripts/breaking_changes').glob('*.py')]
            for source in paths:
                target = root / source.relative_to(ROOT)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(source.read_bytes())
            command = [sys.executable, '-B', str(root / 'scripts/breaking-changes.py'), 'render']
            self.assertEqual(subprocess.run(command + ['--check'], capture_output=True).returncode, 0)
            for relative in ['cliff.toml', 'CHANGELOG.md']:
                target = root / relative
                original = target.read_text()
                target.write_text(original.replace('releases/BREAKING-CHANGES.md', 'missing.md'))
                self.assertEqual(subprocess.run(command + ['--check'], capture_output=True).returncode, 2)
                target.write_text(original)
            drift = root / OUTPUTS[0].relative_to(ROOT)
            drift.write_text('stale correction')
            self.assertEqual(subprocess.run(command + ['--check'], capture_output=True).returncode, 2)
            self.assertEqual(subprocess.run(command + ['--write'], capture_output=True).returncode, 0)
            self.assertEqual(subprocess.run(command + ['--check'], capture_output=True).returncode, 0)
            outside = root.parent / (root.name + '-outside')
            try:
                outside.write_text('preserve external bytes')
                drift.unlink()
                drift.symlink_to(outside)
                self.assertEqual(subprocess.run(command + ['--write'], capture_output=True).returncode, 2)
                self.assertEqual(outside.read_text(), 'preserve external bytes')
            finally:
                outside.unlink(missing_ok=True)

    def test_renderers_share_one_body_and_escape_untrusted_prose(self):
        body = render()
        for path in OUTPUTS:
            self.assertEqual(path.read_text(), body)
        record = copy.deepcopy(self.record)
        record['title'] = '<script>unsafe</script>`[x](https://invalid) {{#include /etc/passwd}}'
        with patch('breaking_changes.render.records', return_value=[(FIXTURE, record, 'd' * 64)]):
            changed = render()
        self.assertNotIn('{{#include', changed)
        self.assertNotIn('<script>', changed)
        self.assertNotIn('[x]', changed)
        self.assertNotIn('`[x]', changed)


if __name__ == '__main__':
    unittest.main()
