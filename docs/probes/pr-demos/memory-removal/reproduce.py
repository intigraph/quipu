"""Compare command admission only; never open a store or contact a server."""
import argparse
import subprocess

p = argparse.ArgumentParser()
p.add_argument('--before', required=True)
p.add_argument('--after', required=True)
a = p.parse_args()
for label, binary in [('Before', a.before), ('After', a.after)]:
    result = subprocess.run([binary, 'memory', '--help'], capture_output=True, text=True, timeout=15)
    accepted = result.returncode == 0
    print(f'{label}: memory command accepted={accepted}; exit={result.returncode}')
    assert accepted is (label == 'Before')
    if label == 'After':
        assert 'unknown command: memory' in result.stderr
        print('After: explicit unknown-command refusal; no store operation requested')
print('Retained-record query/export is covered separately by the persistence regression.')
print('Source CLI demonstration only; no production deployment or data migration.')
