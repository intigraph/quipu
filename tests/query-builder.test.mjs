import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {Aggregate,Call,Compare,Filter,Group,Iri,Literal,Path,Select,Triple,Var} from '../wasm/explorer/runtime/query-builder.mjs';
import {cases,literalQuery} from './query-builder-cases.mjs';
const fixture=JSON.parse(readFileSync(new URL('../test-fixtures/query-builder/cases.json',import.meta.url),'utf8'));

test('reference serialization, immutable composition and temporal requests',()=>{
  for (const [name,q] of Object.entries(cases())) assert.equal(q.toSparql(),fixture.queries[name].query);
  const base=cases().simple,child=base.limit(3).validAt('2026-01-02T00:00:00Z').asOfTx(42);
  assert.deepEqual(child.toRequest(),{query:base.toSparql()+' LIMIT 3',valid_at:'2026-01-02T00:00:00Z',tx:42});
  assert.deepEqual(base.toRequest(),{query:base.toSparql()});
  assert.throws(()=>base.patterns.push('raw'));
  assert.throws(()=>child.storedDefinition('saved'),/withoutTemporal/);
  assert.equal(child.withoutTemporal().storedDefinition('saved').template,child.toSparql());
});
test('literal serialization roundtrips the shared Unicode/control corpus',()=>{
  for(const value of fixture.literals) {const text=new Literal(value).sparql();assert.equal(JSON.parse(text),value);assert(!text.includes('{param}'));assert(literalQuery(value).toSparql().endsWith(' }'));}
});
test('invalid typed nodes and unsafe numbers refuse',()=>{
  for(const make of [()=>new Var('?x'),()=>new Iri('relative'),()=>new Iri('urn:x>'),()=>new Literal(NaN),()=>new Literal(2**53),()=>new Literal('\ud800'),()=>new Literal('x',{language:'en; bad'}),()=>new Triple('?s',new Iri('urn:p'),new Var('o')),()=>new Filter('raw'),()=>new Path('/',[new Iri('urn:p')]),()=>new Call('RAW',[new Var('x')]),()=>new Compare(new Var('x'),'raw',new Literal(1)),()=>new Select([new Var('x')]).asOfTx(2**53),()=>new Select([new Var('x')]).validAt('2026-02-30T00:00:00Z'),()=>new Select([new Var('x'),new Aggregate('COUNT',null,new Var('n'))]).toSparql()]) assert.throws(make);
});
test('execute/save preserve native JSON text and explicit operation names',()=>{
  const calls=[],database={invoke(op,input){calls.push([op,JSON.parse(input)]);return '{"tx_id":9007199254740993}';}};
  const base=cases().simple,pinned=base.asOfTx(7);
  assert.equal(pinned.execute(database),'{"tx_id":9007199254740993}');
  assert.deepEqual(calls[0],['quipu_query',pinned.toRequest()]);
  assert.equal(base.save(database,'saved'),'{"tx_id":9007199254740993}');
  assert.deepEqual(calls[1],['quipu_queries',base.storedDefinition('saved')]);
  assert.throws(()=>pinned.save(database,'bad'));assert.equal(calls.length,2);
  assert.throws(()=>base.save({invoke(){throw new Error('native refusal');}},'saved'),/native refusal/);
});
