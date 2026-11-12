"""Replay the atomic-document contract using two local conformance adapters."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

SHAPES = '''@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/atomic/> .
ex:S a sh:NodeShape; sh:targetClass ex:P;
 sh:property [sh:path ex:name; sh:minCount 1; sh:maxCount 1].
'''
DATA = '''<http://example.org/atomic/a> a <http://example.org/atomic/P> .
<http://example.org/atomic/a> <http://example.org/atomic/name> "a" .
'''


def run(binary, atomic, data):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / 'shapes.ttl').write_text(SHAPES)
        (root / 'data.ttl').write_text(data)
        result = subprocess.run(
            [binary, '--write-path', *(['--atomic-document'] if atomic else []),
             '--shapes', str(root / 'shapes.ttl'), '--data', str(root / 'data.ttl')],
            capture_output=True, text=True, timeout=60, check=True)
        return json.loads(result.stdout)


print('Input: a type and its required name, split into two chunks.')
before = run(os.environ['BEFORE_ADAPTER'], False, DATA)
print('BEFORE independent per-triple write verdict:', before['modes']['per_triple']['conforms'])
assert before['batch']['conforms'] is True
assert before['modes']['per_triple']['conforms'] is False

after = run(os.environ['AFTER_ADAPTER'], True, DATA)
print('AFTER independent per-triple verdict (strict gate retained):',
      after['modes']['per_triple']['conforms'])
print('AFTER stage both chunks, then commit once:', after['modes']['per_triple']['atomic'])
assert after['modes']['per_triple']['conforms'] is False
assert after['modes']['per_triple']['atomic'] == {'accepted': True, 'count': 2}

bad = run(os.environ['AFTER_ADAPTER'], True,
          DATA + '<http://example.org/atomic/a> <http://example.org/atomic/name> "b" .\n')
print('CONTROL completed document with two names accepted:',
      bad['modes']['per_triple']['atomic']['accepted'])
assert bad['modes']['per_triple']['atomic']['accepted'] is False
print('PASS: stage together-only data; invalid atomic documents still refuse.')
