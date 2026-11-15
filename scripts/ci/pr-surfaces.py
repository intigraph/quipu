#!/usr/bin/env python3
"""Select expensive browser PR checks; uncertain diff evidence retains them."""
import os
import re
import subprocess


def wasm_changed(paths):
    return any(path.startswith(('wasm/', '.github/actions/wasm-explorer-'))
               or path in ('Cargo.toml', 'Cargo.lock', '.github/workflows/ci.yml',
                           'scripts/ci/pr-surfaces.py', 'scripts/test_ci_fanout.py')
               or re.match(r'(scripts|tests)/[^/]*wasm', path)
               for path in paths)


def main():
    base, head = os.environ.get('PR_BASE', ''), os.environ.get('PR_HEAD', '')
    selected = True
    if all(re.fullmatch(r'[0-9a-f]{40}', ref) for ref in (base, head)):
        try:
            result = subprocess.run(['git', 'diff', '--name-only', '-z', base, head],
                                    capture_output=True, timeout=30)
            if result.returncode == 0:
                selected = wasm_changed(result.stdout.decode('utf-8', errors='strict').split('\0'))
        except (OSError, subprocess.TimeoutExpired, UnicodeError):
            selected = True
    print('wasm=' + str(selected).lower())


if __name__ == '__main__':
    main()
