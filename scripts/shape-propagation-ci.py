#!/usr/bin/env python3
"""Existing pre-commit CI entrypoint. Public source reads; no service writes."""
from __future__ import annotations
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PINS = {'quechua': '41a1c2e05359c3b9cfb0091e6daf469604acc293',
        'camayoc': '769a333ec5a654449720d7b1d84d85c0ba49178f'}
spec = importlib.util.spec_from_file_location('propagation', ROOT/'scripts/check-shape-propagation.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def revision_range(repo, env):
    """Exact checkout head; event base when present. Never substitute an absent base."""
    head = audit.git(repo, 'rev-parse', 'HEAD').decode().strip()
    if env.get('GITHUB_ACTIONS') == 'true':
        if env.get('GITHUB_SHA') != head:
            raise ValueError('CI checkout differs from exact event SHA')
        event = json.loads(Path(env['GITHUB_EVENT_PATH']).read_text())
        base = event.get('pull_request', {}).get('base', {}).get('sha') or event.get('before')
        if base and base != '0'*40 and not re.fullmatch('[0-9a-f]{40}', base):
            raise ValueError('invalid event base')
        # actions/checkout uses depth=1. This is an explicit source-history
        # fetch in the existing CI checkout, never a production operation.
        # Scheduled/manual events have no base: their parent is unavailable
        # until this fetch completes. Keep fetch errors fatal, not warning-only.
        if audit.git(repo, 'rev-parse', '--is-shallow-repository').strip() == b'true':
            subprocess.run(['git', '-C', str(repo), 'fetch', '--unshallow', '--no-tags', 'origin'],
                           check=True, timeout=120)
        if not base or base == '0'*40:
            base = audit.git(repo, 'rev-parse', head+'^').decode().strip()
        if not re.fullmatch('[0-9a-f]{40}', base):
            raise ValueError('invalid event base')
    else:
        base = audit.git(repo, 'rev-parse', head+'^').decode().strip()
    audit.git(repo, 'cat-file', '-e', base+'^{commit}')
    return base, head


def source_file(repository, path, *, fetch=None):
    if repository not in PINS:
        raise ValueError('unknown public source owner')
    relative = PurePosixPath(path)
    if (relative.is_absolute() or '..' in relative.parts or '\\' in path
            or not path or '?' in path or '#' in path):
        raise ValueError('unsafe source path')
    url = 'https://raw.githubusercontent.com/scbrown/'+repository+'/'+PINS[repository]+'/'+path
    local_source = os.environ.get('QUIPU_PROPAGATION_'+repository.upper()+'_REPO')
    if fetch is None and local_source:
        # Explicit local source mode reads immutable Git objects, not possibly
        # dirty working files. No fetch, credential lookup or fallback on error.
        data = audit.git(Path(local_source), 'show', PINS[repository]+':'+path)
    elif fetch is None:
        with urllib.request.urlopen(url, timeout=15) as response:
            data = response.read(2*1024*1024+1)
    else:
        data = fetch(url)
    if len(data) > 2*1024*1024:
        raise ValueError('public source exceeds read budget')
    return data


def report(repo, base, head, catalogue, *, fetch=None):
    old_sha, old = audit.snapshot(repo, base)
    new_sha, new = audit.snapshot(repo, head)
    terms_bytes = source_file('quechua', 'ns.ttl', fetch=fetch)
    inputs = [{'repository': 'scbrown/quechua', 'commit': PINS['quechua'], 'path': 'ns.ttl',
               'sha256': hashlib.sha256(terms_bytes).hexdigest()}]
    with tempfile.TemporaryDirectory(prefix='shape-propagation-') as directory:
        cq_root = Path(directory)
        paths = sorted({row['cq']['path'] for row in catalogue.get('entries', []) if row.get('cq')})
        if len(paths) > 100:
            raise ValueError('CQ source read budget exceeded')
        for path in paths:
            data = source_file('camayoc', path, fetch=fetch)
            target = (cq_root/path).resolve()
            if not target.is_relative_to(cq_root):
                raise ValueError('CQ destination escapes private staging')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            inputs.append({'repository': 'scbrown/camayoc', 'commit': PINS['camayoc'],
                           'path': path, 'sha256': hashlib.sha256(data).hexdigest()})
        rows = audit.audit(audit.changes(old, new), catalogue,
                           audit.Graph().parse(data=terms_bytes.decode(), format='turtle'), cq_root)
    return {'schema': 'shape-propagation/v1', 'base': old_sha, 'head': new_sha, 'mode': 'WARN',
            'changes': rows, 'gaps': sum(bool(row['gaps']) for row in rows), 'inputs': inputs}


def main():
    # Run all counterexamples under the same pinned hook environment.
    for name in ('test-shape-propagation.py', 'test-shape-propagation-ci.py'):
        subprocess.run([sys.executable, str(ROOT/'scripts'/name)], check=True, timeout=60)
    base, head = revision_range(ROOT, os.environ)
    result = report(ROOT, base, head, json.loads((ROOT/'shapes/propagation-catalogue.json').read_text()))
    # Complete report is retained in the existing CI job log. This replaces
    # the proposed separate upload-artifact step; no workflow file is changed.
    print('SHAPE_PROPAGATION_REPORT_BEGIN')
    print(json.dumps(result, sort_keys=True, indent=2))
    print('SHAPE_PROPAGATION_REPORT_END')
    print('WARN: unresolved shape propagation entities:', result['gaps'])
    return 0  # Evidence gaps warn; execution/read failures still fail the hook.


if __name__ == '__main__':
    raise SystemExit(main())
