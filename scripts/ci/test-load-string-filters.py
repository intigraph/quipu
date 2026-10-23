#!/usr/bin/env python3
"""Offline coverage and fail-closed controls for mixed tl2q4j queries."""
import importlib.util
import json
from pathlib import Path
import sys
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("mixed_load", ROOT / "scripts/quipu-load-test.py")
load = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = load
spec.loader.exec_module(load)
s = load.strings


class Controls(unittest.TestCase):
    def test_all_shared_shapes_and_existing_operations_are_exercised(self):
        operations = [load.request_for(i, True) for i in range(48)]
        for name in s.NAMES:
            wanted = "string_filter_" + name
            selected = [payload for op, path, payload in operations if op == wanted]
            self.assertEqual(len(selected), 3)
            self.assertEqual(selected[0]["query"], s.perf.render(s.perf.CLASSES[name], False, True))
        for i in (0, 1, 2, 4, 5):
            self.assertEqual(load.request_for(i), load.request_for(i, True))
        self.assertEqual(load.request_for(3, True)[2]['graph'], 'urn:quipu:graph:root')
        for token in ("CONTAINS", "STRSTARTS", "REGEX", "LCASE", "GRAPH ?g"):
            self.assertTrue(any(token in payload.get("query", "") for _, _, payload in operations))

    def test_http200_malformed_error_and_empty_are_not_success(self):
        self.assertTrue(s.validate_response(b'{"rows":[{"s":"fixture"}]}'))
        for raw in (b'null', b'{}', b'{"rows":[]}', b'{"rows":[null]}',
                    b'{"error":"timeout","rows":[{"s":"fixture"}]}', b'not-json'):
            self.assertFalse(s.validate_response(raw), raw)

    def test_budget_requires_every_class_and_enough_samples(self):
        budget = json.loads((ROOT / 'benchmark/query-perf-budget.json').read_text())
        report = {'operations': {"string_filter_"+name:
                  {'requests': 3, 'p99_ms': 1, 'statuses': {'ok': 3}} for name in s.NAMES}}
        self.assertEqual(s.evaluate(report, budget), [])
        op = 'string_filter_Q4_subject_contains'
        report['operations'][op]['requests'] = 0
        self.assertTrue(any(op in e for e in s.evaluate(report, budget)))
        report['operations'][op] = {'requests': 3, 'p99_ms': 9000, 'statuses': {'http_408': 3}}
        self.assertEqual(len(s.evaluate(report, budget)), 2)
        del report['operations'][op]
        self.assertTrue(any(op in e for e in s.evaluate(report, budget)))

    def test_large_profile_preserves_errors_progress_and_string_failures(self):
        baseline = json.loads((ROOT / 'benchmark/load-test-baseline.json').read_text())
        report = dict(profile='string-filters', requests=144, errors={},
                      read_progress_during_writes=1,
                      string_filters=dict(classes={n: {} for n in s.NAMES},
                                          failures=[], mixed_failures=[]))
        self.assertEqual(load.evaluate(report, baseline), ([], []))
        report['string_filters']['failures'] = ['narrowing disabled']
        self.assertIn('narrowing disabled', load.evaluate(report, baseline)[0])
        report['errors'] = {'http_408': 1}
        report['read_progress_during_writes'] = 0
        del report['string_filters']
        self.assertEqual(len(load.evaluate(report, baseline)[0]), 3)

    def test_cli_cannot_silently_skip_grading_or_shape_coverage(self):
        binary = str(ROOT / 'scripts/quipu-load-test.py')
        budget = str(ROOT / 'benchmark/query-perf-budget.json')
        for extra in ([], ['--baseline', str(ROOT / 'benchmark/load-test-baseline.json'),
                           '--requests-per-level', '1']):
            result = subprocess.run([sys.executable, binary, '--string-filter-budget', budget,
                                     *extra], capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertIn(b'string-filter mode requires', result.stderr)

    def test_differential_reuses_original_gate_and_excludes_brief_classes(self):
        with patch.object(s.perf, 'gate', return_value=({'failures': ['disabled pushdown']}, ['disabled pushdown'])) as gate:
            result = s.differential('http://127.0.0.1:1', {'classes': {}}, 40)
        self.assertEqual(result['failures'], ['disabled pushdown'])
        self.assertEqual(gate.call_args.args[2:4], (5, 40))
        self.assertEqual(gate.call_args.args[4], list(s.NAMES))
        self.assertEqual(len(s.NAMES), 10)


if __name__ == '__main__':
    unittest.main()
