#!/usr/bin/env python3
"""Finite synthetic demo: temporary database, loopback only, no existing store."""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server', required=True)
    parser.add_argument('--preparer', required=True)
    parser.add_argument('--sdk')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='substring-demo-') as temporary:
        root = Path(temporary)
        db = root / 'owned.db'
        token = secrets.token_hex(24)
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(('QUIPU_', 'SEEDS_', 'XDG_'))}
        env.update(HOME=str(root), XDG_CONFIG_HOME=str(root / 'config'),
                   QUIPU_AUTH_TOKEN=token, RAYON_NUM_THREADS='1')
        process = None
        log = (root / 'server.log').open('wb')
        base = ''

        def stop():
            nonlocal process
            if process is not None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
                process = None

        def start():
            nonlocal process, base
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0))
                port = sock.getsockname()[1]
            base = f'http://127.0.0.1:{port}'
            process = subprocess.Popen([args.server, '--db', str(db), '--bind',
                                        f'127.0.0.1:{port}'], env=env,
                                       stdout=log, stderr=log)
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError('demo server exited before readiness')
                try:
                    with urllib.request.urlopen(base + '/health', timeout=1):
                        return
                except OSError:
                    time.sleep(.1)
            raise RuntimeError('demo readiness deadline')

        def request(path, payload, contract=False):
            headers = {'Content-Type': 'application/x-www-form-urlencoded',
                       'X-Quipu-Client': 'agent-adhoc',
                       'Authorization': 'Bearer ' + token}
            if contract:
                headers['X-Quipu-Require-Substring'] = 'v6'
            req = urllib.request.Request(base + path,
                data=urllib.parse.urlencode(payload).encode(), headers=headers)
            try:
                with urllib.request.urlopen(req, timeout=10) as response:
                    return response.status, response.headers, response.read()
            except urllib.error.HTTPError as error:
                return error.code, error.headers, error.read()

        try:
            start()
            graph = 'https://example.org/project/demo'
            subject = 'https://seeds.local/item/demo-needle'
            facts = (f'INSERT DATA {{ GRAPH <{graph}> {{ <{subject}> '
                'a <https://schema.org/Action> ; '
                '<https://schema.org/identifier> "demo-needle" ; '
                '<https://schema.org/name> "synthetic title" ; '
                '<https://schema.org/description> "positive needle description" ; '
                '<https://seeds.local/ontology/status> "open" . } }')
            assert request('/update', {'update': facts})[0] == 200
            query = (f'SELECT ?s WHERE {{ GRAPH <{graph}> {{ ?s '
                '<https://schema.org/description> ?text . '
                'FILTER(isLiteral(?text) && sameTerm(?text, STR(?text)) && '
                'CONTAINS(LCASE(STR(?text)), "needle")) } }')
            code, _, _ = request('/query', {'query': query}, True)
            assert code == 400, code
            print('BEFORE: required index absent -> HTTP 400 (no scan fallback)', flush=True)
            stop()
            subprocess.run([args.preparer, str(db), '--provision'], env=env,
                           check=True, timeout=30, capture_output=True)
            start()
            code, headers, raw = request('/query', {'query': query}, True)
            assert code == 200 and headers.get('X-Quipu-Substring-Index') == 'v6'
            body = json.loads(raw)
            rows = body.get('results', {}).get('bindings', [])
            assert len(rows) == 1 and rows[0]['s']['value'] == subject, body
            print('AFTER: prepared index -> HTTP 200, v6 acknowledgement, 1 synthetic hit', flush=True)
            if args.sdk:
                result = subprocess.run([args.sdk, '--quipu', base, '--graph', graph,
                    '--json', 'search', 'needle', '--full'], env=env,
                    check=True, capture_output=True, text=True, timeout=15)
                assert 'demo-needle' in result.stdout
                print('SDK: --full finds the description-only synthetic hit', flush=True)
        finally:
            stop()
            log.close()


if __name__ == '__main__':
    main()
