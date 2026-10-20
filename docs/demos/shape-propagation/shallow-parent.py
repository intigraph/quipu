"""Reproduce the scheduled depth-one checkout failure using local Git only."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
BASE = '587451b5d63891bccb66edca6d1b36bbfd97f77d'


def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args],
                                   stderr=subprocess.DEVNULL).decode().strip()


def probe(source_root):
    spec = importlib.util.spec_from_file_location('ci', source_root/'scripts/shape-propagation-ci.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory); seed = root/'seed'; seed.mkdir()
        git(seed, 'init'); git(seed, 'config', 'user.name', 'Fixture')
        git(seed, 'config', 'user.email', 'fixture@example.org')
        for content in ('base', 'head'):
            (seed/'fixture').write_text(content)
            git(seed, 'add', '.'); git(seed, 'commit', '-m', content)
        base = git(seed, 'rev-parse', 'HEAD^'); head = git(seed, 'rev-parse', 'HEAD')
        event = root/'event.json'; event.write_text('{}')
        env = {'GITHUB_ACTIONS': 'true', 'GITHUB_SHA': head,
               'GITHUB_EVENT_NAME': 'schedule', 'GITHUB_EVENT_PATH': str(event)}
        results = {}
        for kind in ('full', 'depth-one'):
            checkout = seed
            if kind == 'depth-one':
                checkout = root/'checkout'
                subprocess.run(['git', 'clone', '--quiet', '--depth', '1',
                                seed.as_uri(), str(checkout)], check=True)
            try:
                result = module.revision_range(checkout, env)
                results[kind] = {'exact_range': result == (base, head),
                                 'shallow_after': git(checkout, 'rev-parse', '--is-shallow-repository')}
            except subprocess.CalledProcessError as error:
                results[kind] = {'exact_range': False, 'exit': error.returncode}
        return results


def main():
    with tempfile.TemporaryDirectory() as directory:
        pinned = Path(directory); (pinned/'scripts').mkdir()
        for name in ('shape-propagation-ci.py', 'check-shape-propagation.py'):
            data = subprocess.check_output(['git', '-C', str(ROOT), 'show', BASE+':scripts/'+name])
            (pinned/'scripts'/name).write_bytes(data)
        before = probe(pinned)
    after = probe(ROOT)
    print('BEFORE source commit:', BASE)
    print(json.dumps(before, sort_keys=True))
    print('AFTER script SHA256:', hashlib.sha256((ROOT/'scripts/shape-propagation-ci.py').read_bytes()).hexdigest())
    print(json.dumps(after, sort_keys=True))
    assert before['full']['exact_range'] and before['depth-one']['exit'] == 128
    assert after['full']['exact_range'] and after['depth-one']['exact_range']
    assert after['depth-one']['shallow_after'] == 'false'


if __name__ == '__main__':
    main()
