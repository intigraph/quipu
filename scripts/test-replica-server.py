#!/usr/bin/env python3
"""Private real-server acceptance: no production endpoint or credentials."""
import ctypes
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


for sig in (signal.SIGTERM, signal.SIGHUP):
    signal.signal(sig, interrupted)


# Match the native MCP harness: SIGKILL of the test must not orphan a server.
_libc = ctypes.CDLL(None) if sys.platform.startswith('linux') else None


def die_with_parent():
    if _libc is not None:
        _libc.prctl(1, signal.SIGTERM, 0, 0, 0)  # PR_SET_PDEATHSIG


def request(url, path, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(url + path, data=data,
                                 headers={'Content-Type': 'application/json',
                                          'X-Quipu-Client': 'test'})
    try:
        with urllib.request.urlopen(req, timeout=3) as response:
            return response.status, response.read(), response.headers
    except urllib.error.HTTPError as error:
        return error.code, error.read(), error.headers


def run(server_binary, cli_binary):
    with tempfile.TemporaryDirectory(prefix='quipu-replica-') as directory:
        root = Path(directory)
        home = root / 'home'
        home.mkdir()
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(('QUIPU_', 'BOBBIN_'))}
        env['HOME'] = str(home)
        db = root / 'follower.db'
        seed = root / 'seed.ttl'
        seed.write_text('<urn:positive> <urn:predicate> "present" .\n')
        subprocess.run([cli_binary, 'knot', str(seed), '--db', str(db)],
                       env=env, cwd=root, check=True, capture_output=True)
        conn = sqlite3.connect(db)
        identity = conn.execute('SELECT store_id FROM store_identity').fetchone()[0]
        tx = conn.execute('SELECT MAX(id) FROM transactions').fetchone()[0]
        assert conn.execute('SELECT COUNT(*) FROM events').fetchone()[0] > 0, 'pending push trap has no events'
        observed = {'failed': False, 'posts': 0, 'gets': 0, 'poll_times': []}

        class Primary(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                observed['gets'] += 1
                observed['poll_times'].append(time.monotonic())
                assert self.path == '/watermark'
                assert self.headers['X-Quipu-Client'] == 'replica-metadata'
                self.send_response(500 if observed['failed'] else 200)
                self.end_headers()
                self.wfile.write(json.dumps({'store_id': identity,
                                             'watermark_tx': tx + 7}).encode())

            def do_POST(self):
                observed['posts'] += 1
                self.send_response(200)
                self.end_headers()

        primary = ThreadingHTTPServer(('127.0.0.1', 0), Primary)
        thread = threading.Thread(target=primary.serve_forever, daemon=True)
        thread.start()
        primary_url = f'http://127.0.0.1:{primary.server_port}'
        # A pending subscription is a positive control for forbidden background
        # delivery: the old primary worker would POST and advance its cursor.
        conn.execute('INSERT INTO subscriptions '
                     '(consumer_id,mode,webhook_url,batch_size,batch_window_s,created_at) '
                     'VALUES (?,?,?,?,?,?)',
                     ('replica-trap', 'realtime', primary_url + '/trap', 1, 0,
                      '2000-01-01T00:00:00Z'))
        conn.commit()
        conn.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        conn.close()

        def snapshot():
            data = db.read_bytes()
            return {'counter': int.from_bytes(data[24:28], 'big'),
                    'mtime_ns': db.stat().st_mtime_ns,
                    'sha256': hashlib.sha256(data).hexdigest()}

        before = snapshot()
        (root / '.bobbin').mkdir()
        (root / '.bobbin/config.toml').write_text(
            '[quipu.server.replica]\nprimary_url = ' + json.dumps(primary_url)
            + '\n[quipu.events]\nretention_days = 0\n')
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        url = f'http://127.0.0.1:{port}'
        with (root / 'server.log').open('w+') as log:
            process = subprocess.Popen([server_binary, '--read-only', '--db', str(db), '--bind',
                                        f'127.0.0.1:{port}'],
                                       env=env, cwd=root, stdout=log, stderr=log,
                                       preexec_fn=die_with_parent)
            try:
                deadline = time.monotonic() + 10
                while True:
                    if process.poll() is not None:
                        log.seek(0)
                        raise AssertionError(log.read())
                    try:
                        status, _, headers = request(url, '/health')
                        if headers.get('X-Quipu-Replica-Lag-Tx') == '7':
                            break
                    except OSError:
                        pass
                    assert time.monotonic() < deadline, 'no fresh lag observation'
                    time.sleep(.1)
                for _ in range(100):
                    status, body, headers = request(url, '/query', {
                        'query': 'ASK { <urn:positive> <urn:predicate> "present" }'})
                    assert status == 200 and json.loads(body)['result'] is True
                    assert headers['X-Quipu-Replica-Lag-Tx'] == '7'
                for path in ('/knot', '/episode', '/set', '/update', '/retract',
                             '/events/commit', '/embed_backfill', '/project'):
                    status, body, _ = request(url, path, {})
                    assert status == 503, (path, status)
                    assert json.loads(body)['primary'] == primary_url
                status, body, _ = request(url, '/metrics')
                assert status == 200
                assert b'quipu_replica_lag_transactions 7\n' in body
                observed['failed'] = True
                deadline = time.monotonic() + 20
                while request(url, '/health')[2]['X-Quipu-Replica-Lag-Tx'] != 'unknown':
                    assert time.monotonic() < deadline, 'failure retained a false lag'
                    time.sleep(.2)
                assert b'quipu_replica_lag_transactions -1\n' in request(url, '/metrics')[1]
            finally:
                process.terminate()
                process.wait(timeout=10)
                primary.shutdown()
                primary.server_close()
                thread.join(timeout=3)
        # Malformed replica configuration must never become a default writer.
        for invalid in ('[quipu.server.replica]\n',
                        '[quipu.server.replica]\nprimary_url = 123\n'):
            (root / '.bobbin/config.toml').write_text(invalid)
            result = subprocess.run([server_binary, '--db', str(db), '--bind',
                                     f'127.0.0.1:{port}'], env=env, cwd=root,
                                    capture_output=True, timeout=3,
                                    preexec_fn=die_with_parent)
            assert result.returncode != 0, 'invalid config fell back to a writer'
        (root / '.bobbin/config.toml').unlink()
        result = subprocess.run([server_binary, '--read-only', '--db', str(db),
                                 '--bind', f'127.0.0.1:{port}'], env=env, cwd=root,
                                capture_output=True, timeout=3,
                                preexec_fn=die_with_parent)
        assert result.returncode != 0, 'lost config downgraded the read-only role'
        after = snapshot()
        assert before == after, (before, after)
        assert observed['gets'] >= 2, observed
        assert all(b - a >= 14.9 for a, b in zip(
            observed['poll_times'], observed['poll_times'][1:])), 'fast retry loop'
        assert observed['posts'] == 0, 'replica ran the primary push-delivery job'
        print(json.dumps({'positive_reads': 100, 'before': before, 'after': after,
                          'write_status': 503, 'lag': '7 -> unknown on failure',
                          'primary_reads': observed['gets'], 'forbidden_pushes': 0,
                          'invalid_replica_configs_refused': 2}))


if __name__ == '__main__':
    run(str(Path(sys.argv[1]).resolve()), str(Path(sys.argv[2]).resolve()))
