import importlib.util,json,subprocess,sys,types
from pathlib import Path
root=Path(sys.argv[1]).resolve(); suite=Path(sys.argv[2]).resolve()
quipu=Path(sys.argv[3]).resolve(); server=Path(sys.argv[4]).resolve()
path=root/'benchmark/public/sparql11_federated.py';sys.path.insert(0,str(path.parent))
base='0f23ef3be3cc6ed32d62dacd4cf7c0986fc803bb'
for label in ['before','after']:
    m=types.ModuleType('federated_demo_'+label);m.__file__=str(path)
    source=subprocess.check_output(['git','show',base+':benchmark/public/sparql11_federated.py'],cwd=root,text=True) if label=='before' else path.read_text()
    exec(compile(source,str(path),'exec'),m.__dict__)
    case,data=next(x for x in m.discover(suite/'service/manifest.ttl') if x[0].identifier==':service2')
    real=m.EVAL.reserve_port;first=real();calls=[first,first]
    m.EVAL.reserve_port=lambda: calls.pop(0) if calls else real()
    print(label+': real local SERVICE fixture, first two port reservations identical',flush=True)
    result=m.run_case(case,data,quipu,server)
    print(json.dumps(result,sort_keys=True),flush=True)
    assert result['status']==('failed' if label=='before' else 'passed'),result
