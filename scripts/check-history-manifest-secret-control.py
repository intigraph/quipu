#!/usr/bin/env python3
"""Check the exact manifest exception and a new secret in that same file.

Requires git and gitleaks. All mutations occur in a disposable local clone.
No fixture value or scanner report is printed.
"""
from pathlib import Path
import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
COMMIT = '4391747653c30a45991e6e19e901b6fd78cf5f8a'
MANIFEST = 'docs/demos/qualified-main-history/source.json'


def run(*args, cwd, expected=0):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if result.returncode != expected:
        raise SystemExit(f'{args[0]} returned {result.returncode}; expected {expected} (output withheld)')
    return result


def scan(tree, report, revision, expected):
    run('gitleaks', 'git', '--no-banner', '--redact', '--report-format', 'json',
        '--report-path', str(report), '--log-opts', revision, str(tree),
        cwd=tree, expected=expected)
    return json.loads(report.read_text())


def hook_control(hook, tree, expected):
    head = run('git', 'rev-parse', 'HEAD', cwd=tree).stdout.strip()
    base = run('git', 'rev-parse', 'HEAD^', cwd=tree).stdout.strip()
    refs = f'refs/heads/control {head} refs/heads/control {base}\n'
    result = subprocess.run(['bash', str(hook), 'origin', 'https://example.org/source.git'],
                            input=refs, cwd=tree, capture_output=True, text=True)
    if result.returncode != expected:
        raise SystemExit(f'Hook returned {result.returncode}, expected {expected} (output withheld)')
    if expected == 1 and 'a credential in the commits' not in result.stderr:
        raise SystemExit('Hook did not report an actual credential finding (output withheld)')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hook', type=Path, help='Also exercise an installed pre-push gitleaks hook; no push occurs')
    args = parser.parse_args()
    hook = args.hook.resolve(strict=True) if args.hook else None
    with tempfile.TemporaryDirectory(prefix='manifest-secret-control-') as scratch:
        scratch = Path(scratch)
        tree = scratch / 'repo'
        run('git', 'clone', '--shared', '--no-checkout', str(ROOT), str(tree), cwd=scratch)
        run('git', 'checkout', '--detach', COMMIT, cwd=tree)
        # Discriminate the exact exception in this disposable clone first.
        fingerprint = f'{COMMIT}:{MANIFEST}:generic-api-key:12'
        proposed = (ROOT / '.gitleaksignore').read_text()
        (tree / '.gitleaksignore').write_text('\n'.join(
            line for line in proposed.splitlines() if line != fingerprint) + '\n')
        before = scan(tree, scratch / 'before.json', f'{COMMIT}^..{COMMIT}', 1)
        if not any(f.get('Fingerprint') == fingerprint for f in before):
            raise SystemExit('Withheld exception did not reproduce the exact finding')
        if hook:
            hook_control(hook, tree, 1)
        # Use the proposed exception, rather than the historical checkout's file.
        shutil.copyfile(ROOT / '.gitleaksignore', tree / '.gitleaksignore')
        findings = scan(tree, scratch / 'digest.json', f'{COMMIT}^..{COMMIT}', 0)
        if findings:
            raise SystemExit('Known file digest was not excluded')
        if hook:
            hook_control(hook, tree, 0)
        # Public synthetic pattern; generated to avoid embedding token-like text in source.
        fake = 'gh' + 'p_' + 'Ab3Cd4Ef5Gh6Ij7Kl8Mn9Op0Qr1St2Uv3Wx4'
        path = tree / MANIFEST
        data = json.loads(path.read_text())
        data['api_key'] = fake
        path.write_text(json.dumps(data, indent=2) + '\n')
        run('git', 'add', MANIFEST, cwd=tree)
        run('git', '-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Fixture',
            '-c', 'user.email=fixture@example.test', 'commit', '-m', 'Synthetic scanner control', cwd=tree)
        findings = scan(tree, scratch / 'positive.json', 'HEAD^..HEAD', 1)
        if not any(f.get('File') == MANIFEST and f.get('RuleID') == 'github-pat' for f in findings):
            raise SystemExit('Same-file synthetic secret was not detected')
        if hook:
            hook_control(hook, tree, 1)
        data['api_key'] = hashlib.sha256(b'public synthetic generic API key fixture').hexdigest()
        path.write_text(json.dumps(data, indent=2) + '\n')
        run('git', 'add', MANIFEST, cwd=tree)
        run('git', '-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Fixture',
            '-c', 'user.email=fixture@example.test', 'commit', '-m', 'Synthetic generic API key control', cwd=tree)
        findings = scan(tree, scratch / 'generic.json', 'HEAD^..HEAD', 1)
        if not any(f.get('File') == MANIFEST and f.get('RuleID') == 'generic-api-key' for f in findings):
            raise SystemExit('Same-file generic API key was not detected')
        if hook:
            hook_control(hook, tree, 1)
        print('PASS: exact digest excluded; same-file PAT and generic API key detected; hook=' + str(bool(hook)))


if __name__ == '__main__':
    main()
