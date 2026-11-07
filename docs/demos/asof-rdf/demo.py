"""Exercise a qualified candidate binary against a tiny private retained cut."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('--binary', default=os.environ.get('DEMO_BINARY'), required=not os.environ.get('DEMO_BINARY'))
binary = str(Path(parser.parse_args().binary).resolve(strict=True))
print('Candidate binary SHA256:', hashlib.sha256(Path(binary).read_bytes()).hexdigest(), flush=True)
print('Offline fixture only: one retained record, no network, no configured database.', flush=True)
record = dict(entity='urn:item:1', attribute='urn:predicate:label', value='offline',
              graph='urn:board', op='assert', tx=1, timestamp='2026-11-04T00:00:00Z',
              valid_from='2026-11-04T00:00:00Z', valid_to=None)
digest = hashlib.sha256(json.dumps(record, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
manifest = dict(schema='quipu-asof-share/v1', source=dict(store_id='test', log_epoch='epoch'),
                watermark_tx=3, source_head_tx=3, scope=['urn:board'], context_graphs=[],
                scope_digest='sha256:scope', policy_digest='sha256:policy', shapes_hash='sha256:profile',
                completeness=dict(valid_time=True, bytes='lossless', graphs={
                    'urn:board': dict(records=1, digest=digest, frozen=None)}),
                producer=dict(version='test', git_sha='test', git_dirty=False))
binding = dict(store_id='test', log_epoch='epoch', watermark_tx=3, graph='urn:board',
               shapes_hash='sha256:profile', max_input_bytes=100000, max_frames=20,
               max_disk_bytes=67108864, wall_seconds=30)
with tempfile.TemporaryDirectory(prefix='asof-rdf-demo-') as temporary:
    root = Path(temporary)
    env = {k: v for k, v in os.environ.items() if k not in ('QUIPU_CONFIG', 'QUIPU_DB')}
    env.update(HOME=str(root), XDG_CONFIG_HOME=str(root/'config'))
    path = root/'binding.json'
    path.write_text(json.dumps(binding)); path.chmod(0o600)
    for case, expected in [('corrupt-digest', False), ('complete-cut', True)]:
        cut = json.loads(json.dumps(manifest))
        if not expected:
            cut['completeness']['graphs']['urn:board']['digest'] = '0'*64
        frames, scratch = root/(case+'.jsonl'), root/case
        frames.write_text(json.dumps(dict(event='quipu.asof', data=json.dumps(
            dict(manifest=cut, records=[record]), separators=(',', ':')))))
        frames.chmod(0o600); scratch.mkdir(mode=0o700)
        result = subprocess.run([binary, 'asof-rdf', '--binding', str(path), '--frames',
            str(frames), '--scratch', str(scratch)], cwd=root,
            env={**env, 'SQLITE_TMPDIR': str(scratch)}, capture_output=True, text=True, timeout=40)
        complete = scratch/'complete.json'
        print(case, 'exit=', result.returncode, 'completion=', complete.exists(), flush=True)
        assert (result.returncode == 0) is expected, result.stderr
        assert complete.exists() is expected
        if expected:
            receipt = json.loads(complete.read_text())['receipt']
            print('Observed receipt:', json.dumps(receipt, sort_keys=True), flush=True)
            assert receipt['source_tx'] == 3 and receipt['chunks'] == 1
            assert 'offline' in (scratch/'chunk-00000000.ttl').read_text()
            print('RDF chunk contains the retained label: offline', flush=True)
    assert not (root/'quipu.db').exists()
    print('PASS: corrupt input refused; complete cut converted; no ambient database created.', flush=True)
