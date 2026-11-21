"""Offline shape comparison; use a SHACL-enabled QUIPU_BIN, never a server."""
import hashlib,json,os,pathlib,subprocess,tempfile
root=pathlib.Path(__file__).resolve().parents[3]
base='a97d2f8c1a74510dd2830f9fddb639157d2a3e7c'
prefix='@prefix a:<http://aegis.gastown.local/ontology/>. @prefix q:<https://w3id.org/intigraph/quechua/ns#>. @prefix rdfs:<http://www.w3.org/2000/01/rdf-schema#>. @prefix xsd:<http://www.w3.org/2001/XMLSchema#>.\n'
shape=root/'shapes/aegis-ontology.shapes.ttl'
print('BASE',base,'candidate shape SHA256',hashlib.sha256(shape.read_bytes()).hexdigest())
with tempfile.TemporaryDirectory() as raw:
 d=pathlib.Path(raw);before=d/'before.ttl';before.write_bytes(subprocess.check_output(['git','show',base+':shapes/aegis-ontology.shapes.ttl'],cwd=root))
 for name,s in [('base',before),('candidate',shape)]:
  for dated in [True,False]:
   data=d/'data.ttl';data.write_text(prefix+'\n'.join(f'a:d{i} a a:Directive;rdfs:label "Decision{i}"'+(';q:traceabilityDueAt "2030-12-05T00:00:00Z"^^xsd:dateTime .' if dated else '.') for i in range(3)))
   p=subprocess.run([os.environ.get('QUIPU_BIN','quipu'),'validate','--shapes',str(s),'--data',str(data),'--format','json'],capture_output=True,text=True,check=False)
   result=json.loads(p.stdout);expected=name=='candidate' and dated
   assert result['conforms']==expected and p.returncode==(0 if expected else 1),(p.stdout,p.stderr)
   assert result['violations']==(3 if name=='candidate' and not dated else 0)
   print(name,'dated' if dated else 'neither','conforms',result['conforms'],'violations',result['violations'],'warnings',result['warnings'])
print('PASS: dated decisions admitted; missing traceability still refused')
