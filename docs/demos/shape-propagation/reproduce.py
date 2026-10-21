"""A missing downstream contract becomes explicit WARN evidence, then resolves."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
from rdflib import Graph
ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('audit', ROOT/'scripts/check-shape-propagation.py')
m = importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
prefix = '@prefix x: <https://example.org/> . @prefix sh: <http://www.w3.org/ns/shacl#> . '
old = Graph().parse(data=prefix, format='turtle')
new = Graph().parse(data=prefix+'x:S a sh:NodeShape; sh:targetClass x:C; sh:property [sh:path x:p; sh:minCount 1] .', format='turtle')
delta = m.changes(old,new)
with tempfile.TemporaryDirectory() as directory:
 root=Path(directory);query=root/'question.json'
 query.write_text('{"query":"SELECT ?entity WHERE { ?entity a <https://example.org/C> }"}')
 before=m.audit(delta,{'entries':[]},Graph(),root)
 entries=[{'iri':r['iri'],'definition_sha256':r['after']['sha256'],
           'disposition':'quipu-local','reason':'synthetic engine fixture only',
           'cq':{'path':query.name,'sha256':hashlib.sha256(query.read_bytes()).hexdigest()}} for r in delta]
 after=m.audit(delta,{'entries':entries},Graph(),root)
 print(json.dumps({'mode':'WARN','before_unresolved':sum(bool(r['gaps']) for r in before),
                   'after_unresolved':sum(bool(r['gaps']) for r in after),
                   'writes':0,'enforcement_enabled':False},sort_keys=True))
