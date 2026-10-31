#!/bin/bash
set -euo pipefail
python3 - <<'PY'
import json,os,pathlib,urllib.request,urllib.error,hashlib
print('BEFORE: deployed Pages at base 46a26acd (released legacy engine)',flush=True)
url='https://intigraph.github.io/quipu/explore/'
with urllib.request.urlopen(url+'default-pack-proof.js',timeout=15) as r:
    proof=r.read().decode().strip()
print('GET default-pack-proof.js:',proof,flush=True)
assert proof=='export default null;'
try:
    urllib.request.urlopen(url+'default-store.db.gz',timeout=15)
    raise AssertionError('baseline changed; re-record')
except urllib.error.HTTPError as e:
    print('GET default-store.db.gz: HTTP',e.code,flush=True)
    assert e.code==404
print('AFTER: isolated published-book fixture, same base + docs workflow patch',flush=True)
p=pathlib.Path(os.environ['EXPLORER_DIR'])
s=(p/'default-pack-proof.js').read_text().split('export default ',1)[1].strip().removesuffix(';')
proof=json.loads(s)
print('default-pack-proof.js schema:',proof['schema'],flush=True)
store=p/'default-store.db.gz'
assert store.stat().st_size>0
print('default-store.db.gz bytes:',store.stat().st_size,flush=True)
print('default-store.db.gz sha256:',hashlib.sha256(store.read_bytes()).hexdigest(),flush=True)
assert proof['schema']=='quipu.explorer-default/v1'
print('PASS: Pages output contains required proof and immutable store.',flush=True)
print('This is candidate build output, not live deployment acceptance.',flush=True)
PY
