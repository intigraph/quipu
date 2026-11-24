"""Compare the old predicate fragment and the new helper on one real child."""
import ast
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

root=Path(__file__).resolve().parents[3]
base='bb28bb63a3d1420adcf16a8dd96007f90710f455'
text=subprocess.check_output(['git','show',base+':scripts/build-repository-share.py'],cwd=root,text=True)
build=next(n for n in ast.parse(text).body if isinstance(n,ast.FunctionDef) and n.name=='build')
index=next(i for i,n in enumerate(build.body) if isinstance(n,ast.Assign)
           and any(isinstance(t,ast.Name) and t.id=='imported' for t in n.targets))
fragment=ast.Module(body=build.body[index:index+3],type_ignores=[])
raw=json.dumps(dict(outcome='quarantined',share_id='synthetic',promotion=dict(eligible=False,
    blockers=['off_vocabulary']),triples=dict(accepted=0),validation=dict(blocking=True,
    conforms=False,off_vocabulary=['private fixture entity'])))
calls=[]
def run(args):
    calls.append(args[1:3])
    return subprocess.run([sys.executable,'-c','import sys;print(sys.stdin.read())'],
        input=raw,text=True,capture_output=True,check=True,timeout=5).stdout
print('Synthetic quarantined decision, child exits zero; no production build/import.')
try:
    exec(compile(fragment,'baseline-build-predicate','exec'),dict(json=json,run=run,
        binary='fixture',stage='stage',receiver='receiver'))
except ValueError as error:print('BEFORE:',error)
else:raise AssertionError('baseline admitted quarantine')
assert calls==[['import','stage']]
print('BEFORE: import decision not retained by the predicate fragment')
spec=importlib.util.spec_from_file_location('decision',root/'scripts/repository_receiver_decision.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
with tempfile.TemporaryDirectory() as folder:
    receipt=Path(folder)/'private.json';calls.clear()
    try:m.import_receiver(run,'fixture','stage','receiver',receipt,'a'*40)
    except ValueError as error:print('AFTER:',error)
    else:raise AssertionError('candidate admitted quarantine')
    record=json.loads(receipt.read_text())
    assert record['decision']==json.loads(raw) and calls==[['import','stage']]
    print('AFTER: full decision retained; mode='+f'{receipt.stat().st_mode&0o777:04o}'+'; no promote')
