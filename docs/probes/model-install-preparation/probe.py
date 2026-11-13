import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--binary',required=True)
parser.add_argument('--model-dir',required=True)
args=parser.parse_args()
binary=Path(args.binary).resolve()
fixture=Path(args.model_dir)
expected={'model.onnx':'6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452','tokenizer.json':'be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037'}  # gitleaks:allow -- pinned public model asset SHA-256 digests
for name,digest in expected.items(): assert hashlib.sha256((fixture/name).read_bytes()).hexdigest()==digest
with tempfile.TemporaryDirectory(prefix='sdfo-installed-') as tmp:
 r=Path(tmp)
 env={'HOME':str(r),'PATH':'/usr/bin:/bin','LANG':'C.UTF-8','XDG_CONFIG_HOME':str(r/'config'),'XDG_DATA_HOME':str(r/'data'),'XDG_CACHE_HOME':str(r/'cache')}
 def run(*args):
  print('$ quipu '+' '.join(args),flush=True)
  p=subprocess.run([str(binary),*args],cwd=r,env=env,capture_output=True,text=True,timeout=120)
  if p.returncode: raise RuntimeError((args,p.returncode,p.stderr[-1200:]))
  return p.stdout if args[0] == "knot" else json.loads(p.stdout)
 (r/'sample.ttl').write_text('<https://example.org/cats> <http://www.w3.org/2000/01/rdf-schema#label> "Cats are feline pets that hunt mice and other rodents" .\n<https://example.org/railway> <http://www.w3.org/2000/01/rdf-schema#label> "Railway locomotives pull passenger trains along steel tracks" .\n')
 run('knot','sample.ttl','--db','fresh.db')
 before=run('doctor','search','--db','fresh.db')
 keyword=run('search','Cats','--db','fresh.db')
 cache=r/'data/quipu/models/all-MiniLM-L6-v2-1110a243fdf4706b3f48f1d95db1a4f5529b4d41'
 cache.mkdir(parents=True)
 for name in expected:shutil.copyfile(fixture/name,cache/name)
 installed=run('model','install','--db','fresh.db')
 after=run('doctor','search','--db','fresh.db')
 result=run('search','feline pets hunt rodents','--mode','semantic','--db','fresh.db')
 print('Selected fields from actual JSON responses (temporary paths omitted):')
 print(json.dumps({'keyword':{'ranking':keyword.get('ranking'),'count':keyword.get('count')},'before':{k:before[k] for k in ['semantic_ready','vector_rows']},'model_install':{k:installed[k] for k in ['installed','preparation'] if k in installed},'after':{k:after[k] for k in ['semantic_ready','vector_rows']},'runtime':{k:after['onnx_runtime'][k] for k in ['available','linkage','state']},'semantic':{'ranking':result.get('ranking'),'count':result.get('count'),'first':result.get('results',[{}])[0].get('entity') if result.get('results') else None}},indent=2))
