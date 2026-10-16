#!/usr/bin/env python3
"""Offline controls: failed naive queries must not masquerade as speedups."""
import importlib.util
import json
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('query_perf', ROOT / 'scripts/quipu-query-perf.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
budget = json.loads((ROOT / 'benchmark/query-perf-budget.json').read_text())
rows = [{'s': 'http://example.org/perf/test', 'l': 'index.example'}]


def arm(name, response=None, *, no_speedup=False):
    defeated_calls = 0

    def query(url, sparql, timeout):
        nonlocal defeated_calls
        if '|| false' in sparql:
            defeated_calls += 1
            # Differential remains valid; only timed defeated calls fail.
            if defeated_calls > 1 and response:
                if name != 'partial-error' or defeated_calls == 3:
                    return response
            return (.001 if no_speedup else .010, 'ok', rows)
        if 'COUNT' in sparql:
            return (.001, 'ok', [{'n': '1'}])
        return (.001, 'ok', rows)

    with patch.object(module, 'run_query', side_effect=query):
        report, failures = module.gate('http://127.0.0.1:1', budget, 3, 30, ['Q3_label_regex'])
    result = report['classes']['Q3_label_regex']
    if name == 'healthy':
        assert not failures, failures
        assert result['ratio'] == 10
    else:
        assert failures, f'{name}: incorrectly accepted as green'
        if not no_speedup:
            assert result['ratio'] is None, f'{name}: invalid timing used in ratio'
    print(f'{name}: {"PASS valid" if not failures else "PASS rejected"}')


arm('healthy')
arm('timeout', (30, '408', None))
arm('malformed-null', (.001, 'ok', None))
arm('malformed-object', (.001, 'ok', {'error': 'invalid response'}))
arm('malformed-row', (.001, 'ok', ['not a binding']))
arm('empty', (.001, 'ok', []))
arm('partial-error', (30, '408', None))
arm('no-speedup', no_speedup=True)

# Validate the real HTTP decoder, not just the controlled query adapter.
class Response:
    def __init__(self, data):
        self.data = data
    def __enter__(self):
        return self
    def __exit__(self, *args):
        return False
    def read(self):
        return json.dumps(self.data).encode()

for payload, expected in [
    ({'rows': rows}, 'ok'),
    ({'rows': None}, 'malformed'),
    ({'rows': {'s': 'invalid'}}, 'malformed'),
    ({'rows': ['invalid']}, 'malformed'),
    ({'rows': rows, 'error': 'query failed'}, 'malformed'),
]:
    with patch.object(module.urllib.request, 'urlopen', return_value=Response(payload)):
        _, status, _ = module.run_query('http://127.0.0.1:1', 'SELECT ?s WHERE { ?s ?p ?o }', 1)
    assert status == expected, (payload, status)
print('HTTP decoder: 5 controls PASS')

# Served replay must fail before querying more classes after degraded reads.
# It never sends the defeated form, even for the known-slow graph class.
def smoke_arm(name, response, expected, profile=None):
    calls = []
    def query(url, sparql, timeout):
        assert '|| false' not in sparql
        calls.append(sparql)
        return response(len(calls)) if callable(response) else response
    with patch.object(module, 'run_query', side_effect=query):
        report = module.smoke('http://127.0.0.1:1', 3, 1, profile)
    assert bool(report['failures']) == expected, (name, report)
    assert set(report['classes']) == set(module.CLASSES)
    if expected:
        assert len(calls) <= 3, (name, len(calls))
        assert sum(c['status'] == 'not_run' for c in report['classes'].values()) == 9
    else:
        assert len(calls) == 30
        assert all(c['status'] == 'passed' for c in report['classes'].values())
    print(f'smoke {name}: PASS')

healthy = (.001, 'ok', [{'n': '2', 's': 'example'}])
smoke_arm('healthy', healthy, False)
smoke_arm('timeout', (30, '408', None), True)
smoke_arm('empty', (.001, 'ok', []), True)
smoke_arm('zero-count', (.001, 'ok', [{'n': '0'}]), True)
smoke_arm('missing-count', (.001, 'ok', [{'s': 'example'}]), True)
smoke_arm('malformed', (.001, 'ok', ['invalid']), True)
smoke_arm('partial-error', lambda n: healthy if n == 1 else (30, '408', None), True)
smoke_arm('partial-empty', lambda n: healthy if n == 1 else (.001, 'ok', []), True)
profile = {
    'bindings': {'namespace': 'http://example.org/served/', 'knowledge_graph': 'urn:served:knowledge',
                 'subject_needle': 'known-entity', 'label_regex': 'index\\.example|case-9ka',
                 'literal_needle': 'drain', 'comment_needle': 'Dolt', 'label_needle': 'quipu',
                 'graph_needle': 'seeds'},
    'max_p95_ms': {name: 100 for name in module.CLASSES},
}
classes, _ = module.smoke_classes(profile)
assert set(classes) == set(module.CLASSES)
for name, cls in classes.items():
    rendered = module.render(cls, False, True)
    assert 'http://example.org/perf/' not in rendered, (name, rendered)
    assert 'urn:perf:graph:' not in rendered
assert 'urn:served:knowledge' in module.render(classes['Q2_graph_literal_substr'], False, True)
assert 'http://example.org/served/known-entity' in classes['Q9_subject_strstarts']['f']
smoke_arm('served-bindings', healthy, False, profile)
smoke_arm('slow-sample', (.101, 'ok', [{'n': '2'}]), True, profile)
for invalid in [dict(profile, bindings={}), dict(profile, max_p95_ms={}),
                dict(profile, max_p95_ms={n: float('nan') for n in module.CLASSES}),
                dict(profile, bindings=dict(profile['bindings'], namespace='http://example.org/>'))]:
    with patch.object(module, 'run_query') as query:
        try:
            module.smoke('http://127.0.0.1:1', 3, 1, invalid)
            raise AssertionError('invalid profile accepted')
        except ValueError:
            pass
        query.assert_not_called()
print('smoke profile validation: 4 controls PASS')

# Exercise the CLI exit code and retained report together.
import contextlib
import io
import sys
import tempfile
with tempfile.TemporaryDirectory() as directory:
    output = Path(directory) / 'report.json'
    for response, expected in [(healthy, 0), ((30, '408', None), 1), ((.001, 'ok', []), 1)]:
        with patch.object(module, 'run_query', return_value=response), patch.object(
            sys, 'argv', ['query-perf', 'smoke', '--reps', '1', '--output', str(output)]), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            assert module.main() == expected
        assert bool(json.loads(output.read_text())['failures']) == bool(expected)
print('smoke CLI exit/report: 3 controls PASS')

# Known-slow is not an error waiver; later classes stay explicitly unrun.
with patch.object(module, 'run_query', side_effect=lambda url, query, timeout:
                  (30, '408', None) if 'GRAPH ?g' in query else healthy):
    report = module.smoke('http://127.0.0.1:1', 1, 1)
assert report['classes']['Q7_graph_var_contains']['status'] == 'failed'
assert report['classes']['Q8_label_lcase_contains']['status'] == 'not_run'
assert report['classes']['Q9_subject_strstarts']['status'] == 'not_run'
print('smoke known-slow error abort: PASS')
for flags in [('--reps', '0'), ('--reps', '-1'), ('--timeout', 'nan'), ('--timeout', '0')]:
    with patch.object(sys, 'argv', ['query-perf', 'smoke', *flags]), \
            patch.object(module, 'run_query') as query, contextlib.redirect_stderr(io.StringIO()):
        try:
            module.main()
            raise AssertionError('invalid sampling accepted')
        except SystemExit as error:
            assert error.code == 2
        query.assert_not_called()
print('smoke CLI sampling validation: 4 controls PASS')

# The actual event command binds its report to the deployed revision.
with tempfile.TemporaryDirectory() as directory:
    profile_path = Path(directory) / 'profile.json'
    output = Path(directory) / 'report.json'
    profile_path.write_text(json.dumps(profile))
    expected = 'a' * 40
    for versions, failed, queried in [([expected, expected], False, True),
                                     (['b' * 40], True, False),
                                     ([OSError('unavailable')], True, False),
                                     ([expected, 'b' * 40], True, True)]:
        with patch.object(module, 'served_revision', side_effect=versions), \
                patch.object(module, 'run_query', return_value=healthy) as query, \
                patch.object(sys, 'argv', ['query-perf', 'smoke', '--profile', str(profile_path),
                                          '--expected-revision', expected, '--reps', '1',
                                          '--output', str(output)]), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            assert module.main() == int(failed)
            assert query.called == queried
        report = json.loads(output.read_text())
        assert bool(report['failures']) == failed
print('served revision event boundary: 4 controls PASS')
