#!/usr/bin/env python3
"""Replay actual private-fixture reports; does not rerun the resource benchmark."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


def main():
    proof = json.loads(Path(__file__).with_name('proof.json').read_text())
    baseline = json.loads((ROOT / 'benchmark/load-test-baseline.json').read_text())
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory) / 'base.py'
        base.write_bytes(subprocess.check_output(
            ['git', 'show', proof['base'] + ':scripts/quipu-load-test.py'], cwd=ROOT))
        old = module('demo_base_load', base)
        print('Base:', proof['base'])
        print('Before: mixed string classes =', sum(
            old.request_for(i)[0].startswith('string_filter_') for i in range(16)))
    new = module('demo_candidate_load', ROOT / 'scripts/quipu-load-test.py')
    print('Candidate:', subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip())
    print('After: mixed string classes =', sum(
        new.request_for(i, True)[0].startswith('string_filter_') for i in range(16)))
    print('Retained native fixture:', proof['fixture']['triples'], 'triples; same starting bytes')
    for name, expected in [('good', 0), ('mutant', 1)]:
        report = proof['reports'][name]
        failures, unknown = new.evaluate(report, baseline)
        actual = 1 if failures else 2 if unknown else 0
        assert actual == expected == proof[name + '_exit']
        ratio = report['string_filters']['classes']['Q4_subject_contains']['ratio']
        print(f'{name}: replayed exit={actual}, requests={report["requests"]}, errors={report["errors"]}, Q4 ratio={ratio}x')
        for failure in failures:
            print('  RATCHET:', failure)
    print('Private fixture evidence only; no production or deployment claim.')


if __name__ == '__main__':
    main()
