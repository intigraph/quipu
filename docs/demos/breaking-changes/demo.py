#!/usr/bin/env python3
"""Offline fixture demonstration; no production read, migration or release."""
import copy
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from breaking_changes.contract import load, readiness  # noqa: E402
from breaking_changes.doctor import assess  # noqa: E402
from test_breaking_changes import CANDIDATE, FIXTURE, NOW, observation, ready_fixture  # noqa: E402


def main():
    record = ready_fixture()
    digest = hashlib.sha256(json.dumps(record).encode()).hexdigest()
    evidence = observation(record, digest)
    draft, _ = load(FIXTURE)
    print('Offline fixture replay; synthetic versions/receipts, not release authorization.')
    print('Draft release readiness:', 'REFUSED' if readiness(draft) else 'unexpectedly ready')
    cases = [('old-only reader', evidence)]
    dual = copy.deepcopy(evidence)
    dual['reader_terms'] += record['detection']['canonical_terms']
    cases.append(('dual reader, canonical record', dual))
    old = copy.deepcopy(dual)
    old['rows'][0]['predicate'] = record['detection']['legacy_terms'][0]
    cases.append(('dual reader, legacy record', old))
    duplicate = copy.deepcopy(dual)
    duplicate['rows'].append(copy.deepcopy(duplicate['rows'][0]))
    cases.append(('duplicate identity', duplicate))
    missing = copy.deepcopy(dual)
    missing['positive_controls'] = []
    cases.append(('missing dataset control', missing))
    for label, data in cases:
        result = assess(record, digest, data, CANDIDATE, 'a' * 64, NOW)
        print(f"{label}: {result['outcome']}; plans={len(result['plan'])}; release_authorized=false")


if __name__ == '__main__':
    main()
