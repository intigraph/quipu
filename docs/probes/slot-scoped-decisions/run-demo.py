#!/usr/bin/env python3
"""Compare stale-decision behavior with two local CLI binaries; no service calls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile


def measure(executable, directory):
    root = Path(directory)
    binary = root / 'qp'
    shutil.copy2(executable, binary)
    env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(root / 'config'),
               XDG_DATA_HOME=str(root / 'data'))

    def run(*args, required=True):
        result = subprocess.run([str(binary), *map(str, args)], env=env, cwd=root,
                                capture_output=True, text=True, timeout=30)
        if required and result.returncode:
            raise RuntimeError(result.stderr or result.stdout)
        return result

    def put(db, text):
        data = root / 'input.ttl'
        data.write_text(text)
        run('knot', data, '--db', db)

    def replace(db, value):
        run('retract', 'urn:test:s', '--predicate', 'urn:test:status', '--db', db)
        put(db, f'<urn:test:s> <urn:test:status> "{value}" .')

    def snapshot(db):
        return run('export', '--format', 'ntriples', '--db', db).stdout

    def clone(source, target):
        with sqlite3.connect(source) as original, sqlite3.connect(target) as copied:
            original.backup(copied)

    source = root / 'source.db'
    put(source, '<urn:test:s> <urn:test:status> "open" .')
    shapes = root / 'shapes.ttl'
    shapes.write_text('@prefix sh: <http://www.w3.org/ns/shacl#> . '
                      '[] sh:path <urn:test:status>; sh:maxCount 1 .')
    run('shapes', 'load', 'merge', shapes, '--db', source)
    base = root / 'base'
    run('share', '--output', base, '--shapes', 'merge', '--destination', 'internal',
        '--db', source)
    base_id = json.loads((base / 'manifest.json').read_text())['share_id']
    local = root / 'local.db'
    run('import', base, '--destination', 'internal', '--db', local)
    replace(source, 'closed')
    incoming = root / 'incoming'
    run('share', '--output', incoming, '--parent-share', base_id, '--shapes', 'merge',
        '--destination', 'internal', '--db', source)
    replace(local, 'blocked')
    decisions = root / 'decisions.json'
    run('merge', incoming, '--emit-decisions', decisions, '--db', local)
    file = json.loads(decisions.read_text())
    assert len(file['rows']) == 1
    for row in file['rows']:
        row['decision'] = {'choose': 'theirs'}
    decisions.write_text(json.dumps(file))
    rows = []
    for n in range(6):
        db = root / f'unrelated-{n}.db'
        clone(local, db)
        for index in range(n):
            put(db, f'<urn:test:other{index}> <urn:test:p> "{index}" .')
        before = snapshot(db)
        result = run('merge', incoming, '--decisions', decisions, '--reviewer',
                     'fixture-reviewer', '--db', db, required=False)
        after = snapshot(db)
        if result.returncode:
            assert 'stale' in result.stderr and before == after
        else:
            assert '<urn:test:s> <urn:test:status> "closed" .' in after
            for index in range(n):
                assert f'<urn:test:other{index}> <urn:test:p> "{index}" .' in after
        rows.append({'unrelated_writes': n, 'synthetic_elapsed_s': n * 120,
                     'accepted': result.returncode == 0,
                     'refused_without_graph_change': bool(result.returncode and before == after)})
    # Negative controls: reviewed slot changed, and a different valid incoming share.
    db = root / 'changed-slot.db'
    clone(local, db)
    replace(db, 'changed-after-review')
    before = snapshot(db)
    changed = run('merge', incoming, '--decisions', decisions, '--reviewer',
                  'fixture-reviewer', '--db', db, required=False)
    assert changed.returncode and before == snapshot(db)
    put(source, '<urn:test:new> <urn:test:p> "incoming changed" .')
    new_incoming = root / 'new-incoming'
    run('share', '--output', new_incoming, '--parent-share', base_id, '--shapes',
        'merge', '--destination', 'internal', '--db', source)
    db = root / 'changed-incoming.db'
    clone(local, db)
    before = snapshot(db)
    changed_incoming = run('merge', new_incoming, '--decisions', decisions,
                           '--reviewer', 'fixture-reviewer', '--db', db, required=False)
    assert changed_incoming.returncode and before == snapshot(db)
    return {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'schema': file['schema'], 'arms': rows,
            'changed_slot_refused_without_graph_change': True,
            'changed_incoming_refused_without_graph_change': True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--candidate', required=True, type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    results = {}
    for name, binary in [('baseline', args.baseline), ('candidate', args.candidate)]:
        with tempfile.TemporaryDirectory(prefix=f'decision-{name}-') as directory:
            results[name] = measure(binary.resolve(), directory)
    assert [r['accepted'] for r in results['baseline']['arms']] == [True] + [False] * 5
    assert all(r['accepted'] for r in results['candidate']['arms'])
    text = json.dumps(results, indent=2)
    if args.output:
        args.output.write_text(text + '\n')
    print(text)


if __name__ == '__main__':
    main()
