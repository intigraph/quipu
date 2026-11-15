"""Replay historical registry conflicts and private release-byte derivation."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('knowledge', ROOT / 'scripts/build-repository-knowledge.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
registry = json.loads((ROOT / 'docs/knowledge/repository-knowledge.json').read_text())
modes = {r['source']: r['source_hash_mode'] for r in registry['records'] if 'source_hash_mode' in r}
cases = json.loads((ROOT / 'scripts/fixtures/repository-knowledge-merges.json').read_text())


def merge(rows):
    with tempfile.TemporaryDirectory() as directory:
        paths = []
        for i in (1, 0, 2):
            path = Path(directory) / str(i)
            path.write_text(json.dumps(rows[i], indent=2) + '\n')
            paths.append(str(path))
        return subprocess.run(['git', 'merge-file', '-p', *paths], capture_output=True)


print('Synthetic replay of exact historical conflicting registry records:')
for case in cases:
    before = merge(case['records'])
    after_rows = copy.deepcopy(case['records'])
    for record in after_rows:
        record.pop('source_sha256')
        record['source_hash_mode'] = modes[record['source']]
    after = merge(after_rows)
    print(case['merge'][:8], case['records'][0]['source'],
          f'before exit={before.returncode}, after exit={after.returncode}')
    assert before.returncode == 1 and after.returncode == 0
    after_rows[1]['summary'] = 'Left summary'
    after_rows[2]['summary'] = 'Right summary'
    assert merge(after_rows).returncode == 1
print('Different summary edits still conflict.')

with tempfile.TemporaryDirectory() as directory:
    source = Path(directory)
    for record in registry['records']:
        target = source / record['source']
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((ROOT / record['source']).read_bytes())
    target = source / 'docs/knowledge/repository-knowledge.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(registry))
    original = target.read_bytes()
    for path in modes:
        before = module.source_hash(source, next(r for r in registry['records'] if r['source'] == path))
        witness = source / path
        witness.write_bytes(witness.read_bytes() + b'\nprivate demonstration edit\n')
        after = module.source_hash(source, next(r for r in registry['records'] if r['source'] == path))
        text = module.render(source, '1' * 40, lambda _: 'synthetic indexed label')
        assert before != after and after in text
        print(path, 'source edit changes emitted sourceHash:', before != after)
    assert target.read_bytes() == original
    print('Registry and review dates unchanged; index labels are fixture controls, not live coverage.')
