#!/usr/bin/env python3
"""Same CLI probe against retained base bytes and the candidate; no network."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest.mock import patch

root = Path(__file__).resolve().parents[4]
base = '65db3e3c49f4e16b188b7c6b1e895f1428187cac'
source = root / 'scripts/quipu-query-perf.py'
base_bytes = subprocess.check_output(['git', 'show', base + ':scripts/quipu-query-perf.py'], cwd=root)
print('Target: source CLI, controlled query responses; no service requests.')
with tempfile.TemporaryDirectory() as directory:
    old = Path(directory) / 'before.py'
    old.write_bytes(base_bytes)
    for label, path in [('BASE ' + base, old), ('CANDIDATE source SHA256 ' + hashlib.sha256(source.read_bytes()).hexdigest(), source)]:
        spec = importlib.util.spec_from_file_location('query_perf', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        result = {}
        for name, response in [('healthy', (.001, 'ok', [{'n': '1', 's': 'example'}])),
                               ('timeout', (30, '408', None)), ('empty', (.001, 'ok', []))]:
            with patch.object(module, 'run_query', return_value=response), patch.object(
                sys, 'argv', ['query-perf', 'smoke', '--url', 'http://127.0.0.1:1', '--reps', '1']), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                result[name] = module.main()
        print(label)
        print('  smoke CLI exit codes:', json.dumps(result, sort_keys=True))
        assert result == ({'healthy': 0, 'timeout': 0, 'empty': 0} if path == old else
                          {'healthy': 0, 'timeout': 1, 'empty': 1})
print('PASS: same failures accepted before, rejected after; healthy control preserved.')
