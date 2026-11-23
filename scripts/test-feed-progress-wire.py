#!/usr/bin/env python3
"""Private loopback wire qualification against an explicitly selected artifact."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--expect-unsupported', action='store_true')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='feed-progress-') as temporary:
        root = Path(temporary)
        db = str(root / 'fixture.db')
        for graph in ['urn:watched', 'urn:other']:
            subprocess.run([str(args.cli), 'graph', 'create', graph, '--db', db],
                           check=True, capture_output=True)
        seed = root / 'seed.nt'
        seed.write_text('<urn:seed> <urn:p:title> "seed" .\n')
        subprocess.run([str(args.cli), 'ingest', str(seed), '--graph', 'urn:watched',
                        '--timestamp', '2026-11-01T00:00:00Z', '--declare-count', '1',
                        '--declare-sha256', hashlib.sha256(seed.read_bytes()).hexdigest(),
                        '--format', 'nt', '--db', db], check=True, capture_output=True)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        env = {k: v for k, v in os.environ.items() if not k.startswith('QUIPU_')}
        with (root / 'server.log').open('wb') as log:
            child = subprocess.Popen([str(args.server.resolve()), '--db', db, '--bind',
                                      f'127.0.0.1:{port}'], env=env, stdout=log,
                                     stderr=log, cwd=root)
            try:
                for _ in range(100):
                    try:
                        with urllib.request.urlopen(base + '/changes?since=0', timeout=1) as response:
                            initial = json.load(response)
                        break
                    except urllib.error.URLError:
                        if child.poll() is not None:
                            raise RuntimeError('private server exited; retained diagnostic unavailable')
                        time.sleep(.1)
                else:
                    raise RuntimeError('private server not ready')
                scope = {'since': initial['watermark_tx'], 'graph': 'urn:watched',
                         'predicate': 'urn:p:title', 'progress': 'true'}
                url = base + '/changes/stream?' + urllib.parse.urlencode(scope)
                if args.expect_unsupported:
                    try:
                        urllib.request.urlopen(url, timeout=2)
                    except urllib.error.HTTPError as error:
                        assert error.code == 400, error.code
                        print('BASELINE_REFUSES_PROGRESS HTTP400')
                        return
                    raise AssertionError('baseline unexpectedly supports progress')

                def next_frame(response):
                    fields = {}
                    size = 0
                    while True:
                        line = response.readline(1_048_577)
                        if not line:
                            raise AssertionError('stream disconnected')
                        size += len(line)
                        assert size <= 1_048_576, 'frame bound'
                        line = line.rstrip(b'\r\n')
                        if not line:
                            if fields:
                                return fields['event'], json.loads(fields['data'])
                            size = 0
                        elif not line.startswith(b':'):
                            key, _, value = line.partition(b':')
                            fields[key.decode()] = value.lstrip().decode()

                def insert(graph, name):
                    query = f'INSERT DATA {{ GRAPH <{graph}> {{ <urn:{name}> <urn:p:title> "{name}" }} }}'
                    request = urllib.request.Request(base + '/update', data=query.encode(),
                        headers={'Content-Type': 'application/sparql-update'})
                    with urllib.request.urlopen(request, timeout=5) as response:
                        assert response.status in (200, 204)

                with urllib.request.urlopen(url, timeout=75) as response:
                    event, first = next_frame(response)
                    assert event == 'quipu.changes.progress', event
                    assert first['source'] == initial['source']
                    assert first['since'] == first['next_tx'] == initial['watermark_tx']
                    assert first['complete'] and first['records'] == []
                    assert first['scope'] == {'graphs': ['urn:watched'], 'predicate': 'urn:p:title'}
                    # A real quiet shared observation renews freshness without a write.
                    quiet = first
                    while quiet['observed_at'] - first['observed_at'] < 50:
                        event, quiet = next_frame(response)
                        assert event == 'quipu.changes.progress'
                        assert quiet['next_tx'] == first['next_tx']
                        assert quiet['observed_at'] > first['observed_at']
                    insert('urn:other', 'unrelated')
                    event, unrelated = next_frame(response)
                    assert event == 'quipu.changes.progress', event
                    assert unrelated['next_tx'] > quiet['next_tx']
                    insert('urn:watched', 'matching')
                    event, matching = next_frame(response)
                    assert event == 'quipu.changes', event
                    assert 'urn:matching' in json.dumps(matching)
                    event, checkpoint = next_frame(response)
                    assert event == 'quipu.changes.progress', event
                    assert checkpoint['since'] == checkpoint['next_tx'] == matching['next_tx']
                    print('PASS native loopback initial/quiet/unrelated/matching-before-progress; no prod')
            finally:
                child.terminate()
                child.wait(timeout=10)


if __name__ == '__main__':
    main()
