"""Local synthetic merge-group recording; no hosted queue or settings writes."""
import importlib.util
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location(
    'queue_controls', root / 'scripts/ci/test-merge-group-ledger.py')
controls = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controls)
base = 'bc8d91fb46b87ab54c8aad32d5987c5dfc402300'
workflow = subprocess.check_output(
    ['git', 'show', f'{base}:.github/workflows/ci.yml'], cwd=root, text=True)
assert '  merge-group-ledger-provenance:' not in workflow
print(f'BASE {base}: merge_group trigger exists, strict ledger job absent', flush=True)
fixture = controls.MergeGroupLedgerTests()
fixture.setUp()
try:
    print('CANDIDATE: correct ledger on combined two-parent Git group', flush=True)
    result = fixture.check()
    print(result.stdout.strip(), flush=True)
    assert result.returncode == 0, result.stderr
    fixture.write('src/ahead.rs', '// code from another queue entry\n')
    fixture.commit('combined code drift')
    print('CANDIDATE: same ledger after another queue entry changes code', flush=True)
    result = fixture.check()
    print(result.stdout.strip(), flush=True)
    assert result.returncode != 0 and 'src/ahead.rs' in result.stdout
    print('Expected stale-ledger refusal observed. No live queue tested.', flush=True)
finally:
    fixture.doCleanups()
