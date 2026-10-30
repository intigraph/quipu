"""Record actual offline fixture metrics from two owned profile executables."""
import hashlib
import re
from pathlib import Path
import subprocess
import sys

base, candidate, query = sys.argv[1:]
print('Offline: 7,500 synthetic closed items across two named graphs.', flush=True)
print('Engine base: ed79bf881ec99e100d7fe3b047cb08d315bbdb59', flush=True)
print('Engine candidate: eb738e7b61bbe0cfdfd5cfac8d543cb7a271f1e8', flush=True)
answers, statements = [], []
for label, executable in [('BASE', base), ('CANDIDATE', candidate)]:
    print(f'\n$ profile {label} query.rq 6000', flush=True)
    output = subprocess.run([str(Path(executable).resolve(strict=True)), query, '6000'], check=True,
                            capture_output=True, text=True).stdout.splitlines()
    measurement = next(line for line in output if line.startswith('MEASURE '))
    answer = next(line for line in output if line.startswith('ANSWER '))
    print(measurement, flush=True)
    assert 'rows=Ok(39)' in measurement, 'nonempty coverage and event control'
    statements.append(int(re.search(r'statements=(\d+)', measurement).group(1)))
    answers.append(answer)
assert answers[0] == answers[1], 'the optimization must preserve every result row'
assert statements[0] > 20 * statements[1], 'the base must demonstrate excessive work'
print('\nExact sorted answers: identical', flush=True)
print('Answer SHA256:', hashlib.sha256(answers[0].encode()).hexdigest(), flush=True)
print(f'SQL statements: {statements[0]} -> {statements[1]}', flush=True)
