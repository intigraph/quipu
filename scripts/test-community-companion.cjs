// Real native producer -> Wasm receiver round trip. Private fixtures only.
// Usage: node scripts/test-community-companion.cjs <quipu> <node-bindings.js>
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const {execFileSync} = require('node:child_process');
const [quipu, bindings] = process.argv.slice(2);
if (!quipu || !bindings) throw Error('native binary and node bindings required');
const {Explorer} = require(path.resolve(bindings));
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'community-companion-'));
const repo = path.resolve(__dirname, '..');
const run = (args) => execFileSync(quipu, args, {encoding:'utf8'});
try {
  const db = path.join(work,'source.db');
  const shapes = path.join(work,'fixture.ttl');
  const data = path.join(work,'data.ttl');
  fs.writeFileSync(shapes,'@prefix sh:<http://www.w3.org/ns/shacl#>. @prefix ex:<http://example.org/>. ex:Shape a sh:NodeShape; sh:targetClass ex:Node.');
  fs.writeFileSync(data,'@prefix ex:<http://example.org/>. @prefix rdfs:<http://www.w3.org/2000/01/rdf-schema#>. ex:a a ex:Node; ex:link ex:b; rdfs:label "CLI: alpha". ex:b a ex:Node; rdfs:label "Beta".');
  run(['shapes','load','fixture',shapes,'--db',db]);
  run(['knot',data,'--db',db]);
  run(['shapes','load','producer-policy',path.join(repo,'docs/knowledge/publication-policy.shapes.ttl'),'--db',db]);
  run(['knot',path.join(repo,'docs/knowledge/publication-policy.ttl'),'--graph','urn:fixture:policy','--db',db]);
  const primary = path.join(work,'primary');
  run(['share','--output',primary,'--db',db]);
  const companion = path.join(work,'companion');
  execFileSync('bash',[path.join(repo,'scripts/build-community-companion.sh'),quipu,primary,repo,companion,path.join(repo,'docs/knowledge/publication-policy.ttl')],{stdio:'pipe'});
  const archive = (directory) => {
    const out = directory+'.tar.gz';
    execFileSync('tar',['-C',directory,'-czf',out,'.']);
    return fs.readFileSync(out);
  };
  const parentBytes = archive(primary);
  const bytes = archive(companion);
  const manifest = JSON.parse(fs.readFileSync(path.join(companion,'manifest.json'),'utf8'));
  const explorer = Explorer.loadPendant(parentBytes,'fixture','2026-01-01T00:00:00Z');
  const root = () => explorer.query('SELECT ?s ?p ?o WHERE { ?s ?p ?o }');
  const before = root();
  const first = explorer.projectCommunities('{}');
  assert.equal(explorer.projectCommunities('{}'),first);
  assert.throws(()=>explorer.projectCommunities('{"persist":true}'));
  assert.throws(()=>explorer.loadCommunityPendant(bytes,manifest.graph_hash,false,'fixture','2026-01-02T00:00:00Z'));
  assert.throws(()=>explorer.loadCommunityPendant(bytes,'0'.repeat(64),true,'fixture','2026-01-02T00:00:00Z'));
  assert.equal(root(),before);
  const imported = JSON.parse(explorer.loadCommunityPendant(bytes,manifest.graph_hash,true,'fixture','2026-01-02T00:00:00Z'));
  assert.ok(imported.derived_graph.startsWith('urn:quipu:'));
  assert.equal(root(),before,'companion must not promote into ROOT');
  const rows = explorer.query(`SELECT ?c ?label WHERE { GRAPH <${imported.derived_graph}> { ?c <http://www.w3.org/2000/01/rdf-schema#label> ?label } }`);
  assert.ok(rows.includes('label') && !rows.includes('\"count\":0'),rows);
  const cold = Explorer.loadPendant(parentBytes,'fixture','2026-01-01T00:00:00Z');
  assert.equal(cold.projectCommunities('{}'),first,'cold load identity and labels');
  const extra = path.join(work,'extra.ttl');
  fs.writeFileSync(extra,'<http://example.org/c> a <http://example.org/Node>.');
  run(['knot',extra,'--db',db]);
  const otherParent = path.join(work,'other-primary');
  run(['share','--output',otherParent,'--db',db]);
  const otherCompanion = path.join(work,'other-companion');
  execFileSync('bash',[path.join(repo,'scripts/build-community-companion.sh'),quipu,otherParent,repo,otherCompanion,path.join(repo,'docs/knowledge/publication-policy.ttl')],{stdio:'pipe'});
  const otherManifest = JSON.parse(fs.readFileSync(path.join(otherCompanion,'manifest.json'),'utf8'));
  assert.throws(()=>explorer.loadCommunityPendant(archive(otherCompanion),otherManifest.graph_hash,true,'fixture','2026-01-02T00:00:00Z'));
  assert.equal(root(),before,'wrong parent must not alter ROOT');
  console.log('PASS: native companion -> verified named Wasm import; adoption/hash refusals; ROOT invariant; cold-load identity');
} finally {
  fs.rmSync(work,{recursive:true,force:true});
}
