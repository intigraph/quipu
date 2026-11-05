#!/usr/bin/env python3
"""Scratch-only reader startup failures must not terminate the primary."""
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def request(base, path, payload=None, authorized=False):
    headers = {'Content-Type': 'application/json', 'X-Quipu-Client': 'agent-adhoc'}
    if authorized:
        headers['Authorization'] = 'Bearer synthetic-startup-only'
    req = urllib.request.Request(base + path, headers=headers,
                                 data=None if payload is None else json.dumps(payload).encode())
    try:
        with urllib.request.urlopen(req, timeout=3) as response:
            return response.status, json.load(response), response.headers
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read()), error.headers


def run(server, cli, mode):
    with tempfile.TemporaryDirectory(prefix='quipu-startup-') as directory:
        root = Path(directory)
        (root / 'home').mkdir()
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(('QUIPU_', 'BOBBIN_', 'SEEDS_', 'XDG_'))}
        env.update(HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'home/.config'))
        db = root / 'scratch.db'
        seed = root / 'seed.ttl'
        seed.write_text('<urn:positive> <urn:predicate> "present" .\n')
        subprocess.run([cli, 'load', str(seed), '--db', str(db)], cwd=root,
                       env=env, capture_output=True, check=True, timeout=30)
        (root / '.bobbin').mkdir()
        with socket.socket() as reader, socket.socket() as primary:
            reader.bind(('127.0.0.1', 0))
            worker_port = reader.getsockname()[1]
            primary.bind(('127.0.0.1', 0))
            port = primary.getsockname()[1]
            primary.close()
            if mode == 'occupied':
                reader.listen()
                reader.settimeout(.2)
            else:
                reader.close()
            (root / '.bobbin/config.toml').write_text(
                '[quipu.server]\nread_processes=1\nmemory_budget_mib=256\n'
                f'read_process_base_port={worker_port}\nauth_token="synthetic-startup-only"\n'
                '[quipu.embedding]\nauto_embed=false\n')
            command = [server, '--db', str(db), '--bind', f'127.0.0.1:{port}']
            executable_fd = None
            if mode == 'spawn-failure':
                # Execute an owned, already-unlinked ELF through an inherited FD.
                # The primary runs, but its current_exe path cannot launch a child.
                # No production executable or process is changed.
                copy = root / 'owned-server'
                shutil.copyfile(server, copy)
                copy.chmod(0o700)
                executable_fd = os.open(copy, os.O_RDONLY)
                copy.unlink()
                command[0] = f'/proc/self/fd/{executable_fd}'
            with (root / 'server.log').open('w+') as log:
                process = subprocess.Popen(command, cwd=root, env=env, stdout=log, stderr=log,
                                           pass_fds=(() if executable_fd is None else (executable_fd,)),
                                           start_new_session=True)
                if executable_fd is not None:
                    os.close(executable_fd)
                base = f'http://127.0.0.1:{port}'
                try:
                    deadline = time.monotonic() + 20
                    while process.poll() is None and time.monotonic() < deadline:
                        try:
                            status, health, _ = request(base, '/health')
                            break
                        except OSError:
                            time.sleep(.05)
                    else:
                        log.seek(0)
                        raise AssertionError(f'{mode}: primary unavailable: {log.read()}')
                    query = {'query': 'ASK { <urn:positive> <urn:predicate> "present" }'}
                    if mode == 'free':
                        assert status == 200, health
                        code, body, headers = request(base, '/query', query)
                        assert code == 200 and body['result'] is True
                        assert int(headers['X-Quipu-Read-Worker-Pid']) != process.pid
                        assert request(f'http://127.0.0.1:{worker_port}', '/query', query)[0] == 403
                    else:
                        assert status == 503 and health['reason'] == 'read_process_unavailable', health
                        assert health['read_processes'] == 1 and health['read_processes_alive'] == 0
                        assert request(base, '/query', query)[0] == 503, 'writer read fallback'
                    write = {'turtle': '<urn:still> <urn:predicate> "writer" .'}
                    assert request(base, '/knot', write)[0] == 401
                    assert request(base, '/knot', write, authorized=True)[0] == 200
                    if mode == 'occupied':
                        try:
                            connection, _ = reader.accept()
                            connection.close()
                            raise AssertionError('private worker channel contacted a foreign listener')
                        except socket.timeout:
                            pass
                    elif mode == 'spawn-failure':
                        with socket.socket() as released:
                            released.bind(('127.0.0.1', worker_port))
                    time.sleep(.3)
                    assert process.poll() is None
                    log.flush()
                    log.seek(0)
                    records = log.read()
                    if mode != 'free':
                        assert records.count('primary retained') == 1, records
                    print(f'PASS {mode}: primary write/auth, pool health/read admission, no retry')
                finally:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait(timeout=10)


if __name__ == '__main__':
    server, cli = map(lambda p: str(Path(p).resolve()), sys.argv[1:3])
    for mode in ('free', 'occupied', 'spawn-failure'):
        run(server, cli, mode)
