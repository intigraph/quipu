#!/usr/bin/env python3
"""Run identical HTTP preflight probes against the old allowlist and candidate.

The old-policy control removes only the two added header names in this isolated
checkout. The policy file is restored even after failure or an interrupt. Do not
run concurrently with edits to src/server/cors.rs. No service or network needed.
"""
import os
from pathlib import Path
import signal
import subprocess

root = Path(__file__).resolve().parents[3]
policy = root / 'src/server/cors.rs'
original = policy.read_bytes()
old = original.decode()
for header in ('x-quipu-client', 'x-quipu-task'):
    line = f'            axum::http::HeaderName::from_static("{header}"),\n'
    assert old.count(line) == 1, 'unexpected policy; refusal preserves source'
    old = old.replace(line, '')

def interrupted(_signal, _frame):
    raise SystemExit('interrupted; restoring policy')

signal.signal(signal.SIGTERM, interrupted)
command = [os.environ.get('CARGO', 'cargo'), 'test', '--features', 'full',
           '--bin', 'quipu-server', 'cors::tests', '--', '--nocapture']

def probe(label, expected):
    print(label, flush=True)
    print('$ cargo test --features full --bin quipu-server cors::tests -- --nocapture', flush=True)
    result = subprocess.run(command, cwd=root, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    # Retain actual relevant output while excluding local cache/build paths.
    for line in result.stdout.splitlines():
        if line.startswith(('Access-Control-Allow-Headers:', 'test cors::tests::',
                            'test result:', 'running 2 tests')):
            print(line, flush=True)
    print(f'exit={result.returncode}; expected={expected}', flush=True)
    if result.returncode != expected:
        raise SystemExit('unexpected probe result; inspect the local build independently')

try:
    policy.write_text(old)
    probe('BEFORE: missing attribution headers (old allowlist control)', 101)
finally:
    policy.write_bytes(original)
probe('AFTER: candidate policy, identical preflight input', 0)
assert policy.read_bytes() == original
print('Policy restored byte-for-byte; no service contacted.', flush=True)
