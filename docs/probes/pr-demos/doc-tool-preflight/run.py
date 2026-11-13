"""Compare missing-Mermaid failure ordering using isolated executable fixtures."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[4]
BASE = 'd649bcd1bb356175b97804b30e3f3e4609cd267f'
FILES = ('justfile', 'scripts/precommit-mdbook.sh', 'scripts/preflight-doc-tools.sh')


def run(label, base):
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        binaries = root / 'bin'
        binaries.mkdir()
        for name in ('bash', 'sh', 'cat', 'just'):
            shutil.copy2(shutil.which(name), binaries / name)
        for name, body in {
            'mdbook': 'exit 0',
            'pre-commit': 'echo "FULL HOOK SUITE STARTED (fixture)"; exec ./scripts/precommit-mdbook.sh',
        }.items():
            target = binaries / name
            target.write_text('#!/bin/bash\n' + body + '\n')
            target.chmod(0o755)
        for name in FILES:
            if base and name.endswith('preflight-doc-tools.sh'):
                continue
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            data = subprocess.check_output(['git', 'show', f'{BASE}:{name}'], cwd=ROOT) if base else (ROOT / name).read_bytes()
            target.write_bytes(data)
            target.chmod(0o755)
        print(f'\n{label}: just check; mdbook present, mdbook-mermaid absent', flush=True)
        result = subprocess.run(['just', 'check'], cwd=root,
                                env=dict(os.environ, PATH=str(binaries)),
                                text=True, capture_output=True, timeout=30)
        print(result.stdout + result.stderr, end='', flush=True)
        print(f'Observed exit: {result.returncode}', flush=True)
        assert result.returncode != 0
        assert 'mdbook-mermaid not found' in result.stderr
        assert ('FULL HOOK SUITE STARTED' in result.stdout) == base


print('Source fixture demonstration; no Cargo, package install or production service.', flush=True)
print(f'BASE {BASE}', flush=True)
run('BEFORE', True)
run('AFTER (current checkout)', False)
print('\nPASS: same missing dependency; candidate refuses before the full hooks.', flush=True)
