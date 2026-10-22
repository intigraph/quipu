#!/usr/bin/env python3
"""Compare isolated native write-path adapters on duplicate language tags."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--before', required=True, type=Path)
parser.add_argument('--after', required=True, type=Path)
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix='value-set-demo-') as directory:
    root = Path(directory)
    shape = root / 'shapes.ttl'
    data = root / 'data.ttl'
    shape.write_text('''@prefix ex: <https://example.org/value-sets/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:S a sh:NodeShape; sh:targetNode ex:focus;
 sh:property [sh:path ex:left; sh:uniqueLang true].
''')
    data.write_text('''@prefix ex: <https://example.org/value-sets/> .
ex:focus ex:left "first"@en, "second"@en .
''')
    print('Input: one focus, two distinct English literals; sh:uniqueLang true.', flush=True)
    for lane, binary, expected in [('before', args.before, True), ('after', args.after, False)]:
        result = subprocess.run(
            [str(binary.resolve()), '--write-path', '--shapes', str(shape),
             '--data', str(data), '--base', data.as_uri()],
            text=True, capture_output=True, timeout=30, check=True)
        actual = json.loads(result.stdout)
        modes = {name: actual['modes'][name]['conforms']
                 for name in ['per_triple', 'per_triple_reversed']}
        assert actual['batch']['conforms'] is False, actual
        assert actual['control_payload_only']['conforms'] is True, actual
        assert all(value is expected for value in modes.values()), modes
        print(json.dumps({'lane': lane, 'batch_conforms': False,
                          'split_conforms': modes, 'weak_control_conforms': True}), flush=True)
