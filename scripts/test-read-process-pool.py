#!/usr/bin/env python3
"""Actual scratch workers: parity, writer isolation, failure and parent lifetime."""
import concurrent.futures
import ctypes
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request


def interrupted(signum, _frame):
    raise SystemExit(128 + signum)


for shutdown_signal in (signal.SIGTERM, signal.SIGHUP):
    signal.signal(shutdown_signal, interrupted)

_libc = ctypes.CDLL(None) if sys.platform.startswith('linux') else None
_parent = os.getpid()


def die_with_parent():
    if _libc is not None:
        _libc.prctl(1, signal.SIGTERM, 0, 0, 0)
        if os.getppid() != _parent:
            os.kill(os.getpid(), signal.SIGTERM)


def port_range():
    # Hold the complete range while choosing it; the server still refuses any
    # conflicting listener at startup and verifies the expected child PID.
    for _ in range(20):
        sockets = []
        try:
            first = socket.socket()
            first.bind(('127.0.0.1', 0))
            base = first.getsockname()[1]
            sockets.append(first)
            if base > 65532:
                continue
            for port in (base + 1, base + 2):
                member = socket.socket()
                sockets.append(member)
                member.bind(('127.0.0.1', port))
            return base
        except OSError:
            pass
        finally:
            for member in sockets:
                member.close()
    raise AssertionError('no private port range available')


def request(base, path, payload=None, token=False):
    headers = {'Content-Type': 'application/json', 'X-Quipu-Client': 'test',
               'X-Quipu-Agent': 'private-process-test',
               'X-Quipu-Read-Worker-Peer': '198.51.100.99:1234'}
    if token:
        headers['Authorization'] = 'Bearer ' + 'fixture-token-' * 4
    req = urllib.request.Request(base + path, headers=headers,
                                 data=None if payload is None else json.dumps(payload).encode())
    try:
        with urllib.request.urlopen(req, timeout=8) as response:
            return response.status, response.read(), response.headers
    except urllib.error.HTTPError as error:
        return error.code, error.read(), error.headers


def live(pid):
    try:
        # A reaped child may be a zombie briefly; it is no longer executing.
        return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z'
    except FileNotFoundError:
        return False


def run(server, cli):
    with tempfile.TemporaryDirectory(prefix='quipu-read-process-') as directory:
        root = Path(directory)
        env = {k: v for k, v in os.environ.items() if not k.startswith(('QUIPU_', 'BOBBIN_'))}
        env['HOME'] = str(root / 'home')
        Path(env['HOME']).mkdir()
        db = root / 'scratch.db'
        seed = root / 'seed.ttl'
        seed.write_text('<urn:positive> <urn:predicate> "present" .\n')
        subprocess.run([cli, 'knot', str(seed), '--db', str(db)], cwd=root,
                       env=env, capture_output=True, check=True, timeout=30)
        (root / '.bobbin').mkdir()
        port = port_range()
        (root / '.bobbin/config.toml').write_text(
            '[quipu.server]\nread_processes=2\nmemory_budget_mib=256\n'
            f'read_process_base_port={port + 1}\nauth_token="' + 'fixture-token-' * 4 + '"\n'
            '[quipu.search]\nrequest_timeout_ms=3000\n')
        base = f'http://127.0.0.1:{port}'
        query = {'query': 'ASK { <urn:positive> <urn:predicate> "present" }'}
        workers = set()
        stop_samples = threading.Event()
        sampler = None
        memory = []
        memory_errors = []
        with socket.socket() as occupied:
            occupied.bind(('127.0.0.1', port + 1))
            occupied.listen()
            occupied.settimeout(.2)
            collision = subprocess.run([server, '--db', str(db), '--bind', f'127.0.0.1:{port}'],
                                       cwd=root, env=env, capture_output=True, timeout=20)
            assert collision.returncode != 0, 'occupied worker port must refuse startup'
            try:
                connection, _ = occupied.accept()
                connection.close()
                raise AssertionError('worker credential sent to unrelated listener')
            except socket.timeout:
                pass
        with (root / 'server.log').open('w+') as log:
            process = subprocess.Popen([server, '--db', str(db), '--bind',
                                        f'127.0.0.1:{port}'], cwd=root, env=env,
                                       stdout=log, stderr=log, start_new_session=True,
                                       preexec_fn=die_with_parent)
            try:
                deadline = time.monotonic() + 30
                while True:
                    if process.poll() is not None:
                        log.seek(0)
                        raise AssertionError(log.read())
                    try:
                        if request(base, '/health')[0] == 200:
                            break
                    except OSError:
                        pass
                    assert time.monotonic() < deadline, 'primary never became ready'
                    time.sleep(.1)
                def ingress_count():
                    status, body, _ = request(base, '/metrics')
                    assert status == 200
                    return int(next(line.split()[1] for line in body.decode().splitlines()
                                    if line.startswith('quipu_http_requests_started_total ')))
                before_reads = ingress_count()
                for _ in range(2):
                    status, body, headers = request(base, '/query', query)
                    assert status == 200 and json.loads(body)['result'] is True
                    workers.add(int(headers['X-Quipu-Read-Worker-Pid']))
                assert len(workers) == 2 and process.pid not in workers
                assert ingress_count() - before_reads == 3, 'forwarded reads bypass primary ingress metrics'
                def sample_memory():
                    while not stop_samples.is_set():
                        try:
                            total = 0
                            for pid in workers | {process.pid}:
                                lines = Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()
                                total += int(next(line for line in lines if line.startswith('Pss:')).split()[1]) * 1024
                            memory.append(total)
                        except (OSError, StopIteration, ValueError) as error:
                            memory_errors.append(type(error).__name__)
                        stop_samples.wait(.05)
                sampler = threading.Thread(target=sample_memory, daemon=True)
                sampler.start()
                assert request(base, '/health')[2]['X-Quipu-Read-Processes-Alive'] == '2'
                assert request(base, '/query', {'query': ' ' * (4 * 1024 * 1024)})[0] == 413
                assert request(base, '/query', {'query': 'DELETE WHERE { ?s ?p ?o }'})[0] not in (200, 201)
                assert json.loads(request(base, '/query', query)[1])['result'] is True
                assert request(base, '/knot', {'turtle': '<urn:no> <urn:p> "refused" .'})[0] == 401
                # Direct worker access cannot bypass the private-channel guard.
                for member in (port + 1, port + 2):
                    assert request(f'http://127.0.0.1:{member}', '/query', query)[0] == 403
                # After two requests round-robin returns to the first worker.
                _, _, headers = request(base, '/query', query)
                first = int(headers['X-Quipu-Read-Worker-Pid'])
                other = (workers - {first}).pop()
                os.kill(other, signal.SIGSTOP)
                with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                    blocked = executor.submit(request, base, '/query', query)
                    time.sleep(.3)
                    assert not blocked.done(), 'slow-worker control did not block'
                    started = time.monotonic()
                    assert request(base, '/knot', {'turtle': '<urn:written> <urn:predicate> "new" .'}, token=True)[0] == 200
                    status, body, headers = request(base, '/query', {'query': 'ASK { <urn:written> <urn:predicate> "new" }'})
                    isolated_seconds = time.monotonic() - started
                    assert status == 200 and json.loads(body)['result'] is True
                    assert int(headers['X-Quipu-Read-Worker-Pid']) == first
                    assert isolated_seconds < 2, isolated_seconds
                    assert blocked.result(timeout=7)[0] == 503
                os.kill(other, signal.SIGCONT)
                stop_samples.set()
                sampler.join(timeout=2)
                assert memory and not memory_errors, memory_errors
                assert max(memory) < 256 * 1024 * 1024, max(memory)
                os.kill(other, signal.SIGKILL)
                assert request(base, '/query', query)[0] == 503, 'must not fall back to writer'
                assert request(base, '/health')[0] == 503, 'worker loss must be observable'
                assert request(base, '/knot', {'turtle': '<urn:still> <urn:predicate> "writer" .'}, token=True)[0] == 200
                assert live(first), 'parent-death control needs a live child'
                process.kill()  # Parent destructors cannot run; the child must notice.
                deadline = time.monotonic() + 3
                while any(live(pid) for pid in workers) and time.monotonic() < deadline:
                    time.sleep(.1)
                assert not any(live(pid) for pid in workers)
                process.wait(timeout=5)
                log.flush()
                log.seek(0)
                records = log.read()
                assert 'private-process-test' in records, 'declared caller provenance lost'
                assert '198.51.100.99' not in records, 'untrusted caller spoofed socket peer'
                print(json.dumps({'result': 'PASS', 'distinct_workers': 2,
                                  'slow_reader_writer_and_other_reader_seconds': isolated_seconds,
                                  'worker_loss': '503_no_writer_fallback', 'writes': 'primary_only',
                                  'parent_death': 'live_child_stopped',
                                  'occupied_port': 'refused_without_contact',
                                  'sampled_running_pss_peak_bytes': max(memory),
                                  'memory_samples': len(memory),
                                  'scope': 'scratch_only'}))
            finally:
                stop_samples.set()
                if sampler is not None:
                    sampler.join(timeout=2)
                for pid in workers:
                    if live(pid):
                        os.kill(pid, signal.SIGKILL)
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)


if __name__ == '__main__':
    run(str(Path(sys.argv[1]).resolve()), str(Path(sys.argv[2]).resolve()))
