// Isolated, real compiled-WASM demonstration. No service or private data.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { featureFixture } from '../../../../scripts/wasm-feature-fixture.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
const bundle = process.argv[2];
assert.ok(bundle, 'usage: node demo.mjs <node-bundle-directory> [--expect-missing]');
const before = process.argv.includes('--expect-missing');
const work = mkdtempSync(join(tmpdir(), 'wasm-store-ops-demo-'));
try {
  const binary = process.env.QUIPU_BIN ?? join(repo, 'target/release/quipu');
  const fixture = featureFixture(repo, binary, work);
  const wasm = createRequire(import.meta.url)(join(resolve(bundle), 'quipu_wasm_explorer.js'));
  console.log('Engine:', wasm.explorerVersion());
  const ex = wasm.Explorer.loadPendant(new Uint8Array(readFileSync(fixture.pack)), 'binding-demo', new Date().toISOString());
  const positive = JSON.parse(ex.query('SELECT ?s WHERE { ?s a <http://example.org/Widget> }'));
  assert.equal(positive.rows.length, 2);
  console.log('Positive control: native pack loaded; Widget rows =', positive.rows.length);
  if (before) {
    for (const method of ['shapes', 'knot', 'entityHistory']) {
      assert.equal(typeof ex[method], 'undefined');
      console.log(`${method}: unavailable on the compiled base API`);
    }
  } else {
    const shapes = `@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:demo:RecordShape> a sh:NodeShape; sh:targetClass <urn:demo:Record>;
 sh:property [sh:path <urn:demo:title>; sh:minCount 1] .`;
    console.log('shapes(load):', ex.shapes(JSON.stringify({ action: 'load', name: 'demo', turtle: shapes })));
    console.log('knot(valid):', ex.knot(JSON.stringify({
      turtle: '<urn:demo:good> a <urn:demo:Record>; <urn:demo:title> "Before" .', source: 'binding-demo',
    })));
    const edits = JSON.parse(ex.editLog()).length;
    let rejected = false;
    try {
      const result = JSON.parse(ex.knot(JSON.stringify({ turtle: '<urn:demo:bad> a <urn:demo:Record> .' })));
      rejected = result.conforms === false;
      console.log('knot(missing title):', JSON.stringify(result));
    } catch (error) {
      rejected = /SHACL|validation/i.test(String(error));
      console.log('knot(missing title) rejected:', error.message ?? String(error));
    }
    assert.equal(rejected, true);
    const leaked = JSON.parse(ex.query('SELECT ?p WHERE { <urn:demo:bad> ?p ?v }')).rows.length;
    assert.equal(leaked, 0);
    assert.equal(JSON.parse(ex.editLog()).length, edits);
    console.log('Rejected write: visible facts = 0; edit log unchanged');
    ex.set('urn:demo:good', 'urn:demo:title', JSON.stringify({ str: 'After' }));
    const history = JSON.parse(ex.entityHistory('urn:demo:good'));
    assert.ok(history.history.some((fact) => fact.value === 'Before'));
    assert.ok(history.history.some((fact) => fact.value === 'After'));
    console.log('entityHistory:', JSON.stringify(history));
  }
  console.log(before ? 'BASE CONTROL COMPLETE' : 'CANDIDATE DEMONSTRATION COMPLETE');
} finally {
  rmSync(work, { recursive: true, force: true });
}
