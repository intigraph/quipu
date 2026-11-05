#!/usr/bin/env python3
"""Exercise real emit in isolated test processes, without a running service."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
from contextlib import closing

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--test-binary', type=Path, required=True)
args = parser.parse_args()
for mode in ('false', 'true', 'invalid-directory'):
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        counter, journal = root / 'counter', root / 'journal'
        counter.mkdir(mode=0o700)
        journal.mkdir(mode=0o700)
        env = dict(os.environ, QUIPU_COUNTER_TEST_CHILD='1',
                   QUIPU_SEARCH_COUNTERS_ONLY_ENABLED='false' if mode == 'false' else 'true',
                   QUIPU_SEARCH_COUNTERS_ONLY_DIR=str(counter if mode != 'invalid-directory' else counter / 'missing'),
                   QUIPU_SEARCH_JOURNAL_DIR=str(journal))
        result = subprocess.run([str(args.test_binary.resolve()), '--exact',
            'search_outcome::counters_only::tests::child_privacy_boundary', '--nocapture'],
            env=env, capture_output=True, text=True, check=True)
        files = list(journal.iterdir())
        evidence = any('private-query-sentinel' in f.read_text() for f in files)
        requests = None
        database = counter / 'counts.sqlite'
        if database.exists():
            with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as conn:
                requests = conn.execute("SELECT sum(value) FROM counts WHERE series LIKE 'quipu_search_requests_total%'").fetchone()[0]
            assert b'sentinel' not in database.read_bytes()
        unknown = 'capture UNKNOWN' in result.stderr
        assert (len(files), evidence, requests, unknown) == {
            'false': (1, True, None, False),
            'true': (0, False, 1, False),
            'invalid-directory': (0, False, None, True),
        }[mode], (mode, len(files), evidence, requests, unknown, result.stdout)
        print(json.dumps(dict(mode=mode, detailed_files=len(files),
            query_evidence_persisted=evidence, aggregate_requests=requests,
            capture_unknown=unknown, caller_exit=result.returncode)))
