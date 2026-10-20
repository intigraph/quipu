"""Counterexamples for the existing-target CI integration, without network."""
import importlib.util
import json
import hashlib
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('ci', Path(__file__).with_name('shape-propagation-ci.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ExistingTarget(unittest.TestCase):
    def test_actual_git_delta_retains_full_warning_and_exact_cq_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], stderr=subprocess.DEVNULL).decode().strip()
            git('init'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.org')
            (repo/'shapes').mkdir()
            shape = repo/'shapes/test.ttl'
            prefix = '@prefix x: <https://example.org/> . @prefix sh: <http://www.w3.org/ns/shacl#> . '
            shape.write_text(prefix)
            git('add', '.'); git('commit', '-m', 'base'); base = git('rev-parse', 'HEAD')
            shape.write_text(prefix+'x:S a sh:NodeShape; sh:targetClass x:C .')
            git('add', '.'); git('commit', '-m', 'new shape'); head = git('rev-parse', 'HEAD')
            query = b'{"query":"ASK { ?s ?p ?o }"}'
            def fetch(url):
                return b'@prefix x: <https://example.org/> . x:Term a <http://www.w3.org/2000/01/rdf-schema#Class> .' if url.endswith('/ns.ttl') else query
            warning = m.report(repo, base, head, {'entries': []}, fetch=fetch)
            self.assertEqual((warning['base'], warning['head'], warning['gaps']), (base, head, 2))
            rows = [{'iri': row['iri'], 'definition_sha256': row['after']['sha256'],
                     'disposition': 'mapped', 'term': 'https://example.org/Term',
                     'cq': {'path': 'competency/query.json', 'sha256': hashlib.sha256(query).hexdigest()}}
                    for row in warning['changes']]
            resolved = m.report(repo, base, head, {'entries': rows}, fetch=fetch)
            self.assertEqual(resolved['gaps'], 0)
            self.assertEqual(len(resolved['inputs']), 2)
            self.assertEqual(resolved['mode'], 'WARN')
            self.assertFalse(any('schema' in str(p) for p in repo.iterdir()))

    def test_source_is_immutable_and_unsafe_paths_never_fetch(self):
        urls = []
        self.assertEqual(m.source_file('quechua', 'ns.ttl', fetch=lambda u: urls.append(u) or b'ok'), b'ok')
        self.assertIn(m.PINS['quechua'], urls[0])
        for repo, path in [('other', 'ns.ttl'), ('camayoc', '../escape'), ('camayoc', '/absolute'),
                           ('camayoc', 'a?token=x'), ('camayoc', 'a\\b')]:
            with self.assertRaises(ValueError):
                m.source_file(repo, path, fetch=lambda _: self.fail('unexpected fetch'))

    def test_oversize_or_unreadable_input_is_not_a_warning_success(self):
        with self.assertRaises(ValueError):
            m.source_file('quechua', 'ns.ttl', fetch=lambda _: b'x'*(2*1024*1024+1))
        def refused(_): raise OSError('refused')
        with self.assertRaises(OSError): m.source_file('quechua', 'ns.ttl', fetch=refused)

    def test_explicit_local_source_uses_pinned_objects_and_never_falls_back(self):
        with patch.dict(m.os.environ, {'QUIPU_PROPAGATION_QUECHUA_REPO': '/fixture/source'}), \
             patch.object(m.audit, 'git', return_value=b'pinned') as read, \
             patch.object(m.urllib.request, 'urlopen', side_effect=AssertionError('network')):
            self.assertEqual(m.source_file('quechua', 'ns.ttl'), b'pinned')
            self.assertEqual(read.call_args.args[1:], ('show', m.PINS['quechua']+':ns.ttl'))
            read.side_effect = OSError('missing immutable object')
            with self.assertRaises(OSError): m.source_file('quechua', 'ns.ttl')

    def test_wrong_event_head_and_invalid_base_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)/'event.json'; p.write_text(json.dumps({'before': 'not-a-sha'}))
            env = {'GITHUB_ACTIONS': 'true', 'GITHUB_SHA': 'b'*40, 'GITHUB_EVENT_PATH': str(p)}
            with patch.object(m.audit, 'git', return_value=('a'*40).encode()):
                with self.assertRaises(ValueError): m.revision_range(Path('.'), env)
                env['GITHUB_SHA'] = 'a'*40
                with self.assertRaises(ValueError): m.revision_range(Path('.'), env)

    def test_shallow_source_is_fetched_only_in_ci_and_failure_propagates(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)/'event.json'; p.write_text(json.dumps({'before': 'b'*40}))
            env = {'GITHUB_ACTIONS': 'true', 'GITHUB_SHA': 'a'*40, 'GITHUB_EVENT_PATH': str(p)}
            with patch.object(m.audit, 'git', side_effect=[b'a'*40, b'true']), \
                 patch.object(m.subprocess, 'run', side_effect=OSError('fetch refused')):
                with self.assertRaises(OSError): m.revision_range(Path('.'), env)


if __name__ == '__main__': unittest.main()
