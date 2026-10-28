import {Aggregate,Compare,Filter,Group,Iri,Literal,Optional,Path,Select,Triple,Union,Var} from '../wasm/explorer/runtime/query-builder.mjs';
import {readFileSync} from 'node:fs';
export function cases() {
  const [s,label,n,target,kind]=['s','label','n','target','type'].map(x=>new Var(x));
  return {
    simple:new Select([s,label]).where(new Triple(s,new Iri('urn:label'),label)),
    optional:new Select([s,new Var('next')]).where(new Triple(s,new Iri('urn:type'),new Iri('urn:Thing')),new Optional(new Group([new Triple(s,new Iri('urn:link'),new Var('next'))]))),
    union:new Select([s]).where(new Union([new Group([new Triple(s,new Iri('urn:label'),new Literal('Alpha'))]),new Group([new Triple(s,new Iri('urn:label'),new Literal('Beta'))])])),
    filter:new Select([s]).where(new Triple(s,new Iri('urn:score'),n),new Filter(new Compare(n,'>',new Literal(1)))),
    path:new Select([s,target]).where(new Triple(s,new Path('/',[new Iri('urn:link'),new Path('*',[new Iri('urn:link')])]),target)),
    aggregate:new Select([kind,new Aggregate('COUNT',s,new Var('total'))]).where(new Triple(s,new Iri('urn:type'),kind)).groupBy(kind),
    scoped:new Select([s]).where(new Triple(s,new Iri('urn:type'),new Iri('urn:Thing'))).fromGraph(new Iri('urn:missing')),
  };
}
export function literalQuery(value) {
  const [s,v]=['s','value'].map(x=>new Var(x));
  return new Select([v]).where(new Triple(s,new Iri('urn:value'),v),new Filter(new Compare(v,'=',new Literal(value))));
}
if (process.argv.includes('--emit')) console.log(JSON.stringify(Object.fromEntries(Object.entries(cases()).map(([name,q])=>[name,q.toRequest()]))));
if (process.argv.includes('--literal')) console.log(JSON.stringify(literalQuery(JSON.parse(readFileSync(0,'utf8'))).toRequest()));
