import {Aggregate,Filter,Group,Iri,Literal,Select,Triple,Var,type Database} from '../wasm/explorer/runtime/query-builder.mjs';
const s=new Var('s'), v=new Var('v');
const database:Database={invoke:(_operation,_input)=>'{"count":0}'};
const query=new Select([s,v]).where(new Triple(s,new Iri('urn:p'),v)).asOfTx(1);
const raw:string=query.execute(database);
const template:string=query.withoutTemporal().storedDefinition('saved').template;
void raw; void template;
new Literal(9007199254740993n);
// @ts-expect-error subjects must be typed terms, never source strings
new Triple('?s',new Iri('urn:p'),v);
// @ts-expect-error patterns are not expressions
new Filter(new Group([]));
// @ts-expect-error transaction pins use exact safe JS numbers
query.asOfTx(1n);
// @ts-expect-error aggregate names have a finite typed vocabulary
new Aggregate('UPDATE',null,new Var('count'));
