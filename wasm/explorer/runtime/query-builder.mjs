/** A small typed SELECT builder. Execute/save use the real native WASM facade. */
import {Aggregate,Expr,Iri,Literal,Path,Var,expression} from './query-terms.mjs';
export {Aggregate,Call,Compare,Expr,Iri,Literal,Path,Var} from './query-terms.mjs';
export class Pattern { sparql() { throw new TypeError('abstract pattern'); } }
export class Triple extends Pattern {
  constructor(subject,predicate,object) {
    super();
    if (!(subject instanceof Var || subject instanceof Iri) || !(predicate instanceof Var || predicate instanceof Iri || predicate instanceof Path) || !(object instanceof Var || object instanceof Iri || object instanceof Literal)) throw new TypeError('triple requires typed RDF terms');
    Object.assign(this,{subject,predicate,object}); Object.freeze(this);
  }
  sparql() { return `${this.subject.sparql()} ${this.predicate.sparql()} ${this.object.sparql()} .`; }
}
export class Group extends Pattern {
  constructor(patterns) { super(); if (!Array.isArray(patterns) || !patterns.every(p=>p instanceof Pattern)) throw new TypeError('group requires patterns'); this.patterns=Object.freeze([...patterns]); Object.freeze(this); }
  sparql() { return '{ '+this.patterns.map(p=>p.sparql()).join(' ')+' }'; }
}
export class Optional extends Pattern {
  constructor(group) { super(); if (!(group instanceof Group)) throw new TypeError('OPTIONAL requires Group'); this.group=group; Object.freeze(this); }
  sparql() { return 'OPTIONAL '+this.group.sparql(); }
}
export class Union extends Pattern {
  constructor(groups) { super(); if (!Array.isArray(groups) || groups.length<2 || !groups.every(g=>g instanceof Group)) throw new TypeError('UNION requires at least two Groups'); this.groups=Object.freeze([...groups]); Object.freeze(this); }
  sparql() { return this.groups.map(g=>g.sparql()).join(' UNION '); }
}
export class Filter extends Pattern {
  constructor(value) { super(); expression(value); this.expression=value; Object.freeze(this); }
  sparql() { return 'FILTER ('+this.expression.sparql()+')'; }
}
export class Graph extends Pattern {
  constructor(graph,group) { super(); if (!(graph instanceof Iri || graph instanceof Var) || !(group instanceof Group)) throw new TypeError('GRAPH requires Iri/Var and Group'); Object.assign(this,{graph,group}); Object.freeze(this); }
  sparql() { return `GRAPH ${this.graph.sparql()} ${this.group.sparql()}`; }
}
function natural(value) { if (!Number.isSafeInteger(value) || value<0) throw new TypeError('expected non-negative safe integer'); return value; }
function instant(value) {
  if (typeof value!=='string' || value.startsWith('0000') || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z$/.test(value)) throw new TypeError('validAt requires UTC RFC3339 Z');
  const date=new Date(value); if (!Number.isFinite(+date) || date.toISOString().slice(0,19)!==value.slice(0,19)) throw new TypeError('invalid timestamp');
  return value;
}
export class Select {
  constructor(projections,{patterns=[],grouping=[],sources=[],namedSources=[],distinct=false,_limit=null,_validAt=null,_tx=null}={}) {
    if (!Array.isArray(projections) || !projections.length || !projections.every(p=>p instanceof Var || p instanceof Aggregate)) throw new TypeError('SELECT requires Var/Aggregate');
    const names=projections.map(p=>p instanceof Var?p.name:p.alias.name); if (new Set(names).size!==names.length) throw new TypeError('duplicate projection');
    new Group(patterns);
    if (!Array.isArray(grouping) || !grouping.every(v=>v instanceof Var) || !Array.isArray(sources) || !Array.isArray(namedSources) || ![...sources,...namedSources].every(v=>v instanceof Iri)) throw new TypeError('GROUP BY requires Vars; FROM requires Iris');
    if (typeof distinct!=='boolean') throw new TypeError('distinct must be boolean');
    if (_limit!==null) natural(_limit); if (_tx!==null) natural(_tx); if (_validAt!==null) instant(_validAt);
    Object.assign(this,{projections:Object.freeze([...projections]),patterns:Object.freeze([...patterns]),grouping:Object.freeze([...grouping]),sources:Object.freeze([...sources]),namedSources:Object.freeze([...namedSources]),distinct,_limit,_validAt,_tx}); Object.freeze(this);
  }
  _copy(change) { return new Select(this.projections,{...this,...change}); }
  where(...patterns) { return this._copy({patterns:[...this.patterns,...patterns]}); }
  groupBy(...grouping) { return this._copy({grouping}); }
  fromGraph(...sources) { return this._copy({sources}); }
  fromNamed(...namedSources) { return this._copy({namedSources}); }
  limit(count) { return this._copy({_limit:natural(count)}); }
  validAt(at) { return this._copy({_validAt:instant(at)}); }
  asOfTx(tx) { return this._copy({_tx:natural(tx)}); }
  withoutTemporal() { return this._copy({_validAt:null,_tx:null}); }
  toSparql() {
    if (this.grouping.length || this.projections.some(p=>p instanceof Aggregate)) {
      const grouped=new Set(this.grouping.map(v=>v.name)); if (this.projections.some(p=>p instanceof Var && !grouped.has(p.name))) throw new TypeError('non-aggregate projection missing from GROUP BY');
    }
    let out='SELECT '+(this.distinct?'DISTINCT ':'')+this.projections.map(p=>p.sparql()).join(' ');
    for (const [keyword,graphs] of [['FROM',this.sources],['FROM NAMED',this.namedSources]]) for (const graph of graphs) out+=' '+keyword+' '+graph.sparql();
    out+=' WHERE '+new Group(this.patterns).sparql();
    if (this.grouping.length) out+=' GROUP BY '+this.grouping.map(v=>v.sparql()).join(' ');
    if (this._limit!==null) out+=' LIMIT '+this._limit;
    return out;
  }
  toRequest() { const out={query:this.toSparql()}; if (this._validAt!==null) out.valid_at=this._validAt; if (this._tx!==null) out.tx=this._tx; return out; }
  storedDefinition(name,{description='',dataset=null}={}) {
    if (this._validAt!==null || this._tx!==null) throw new TypeError('stored definitions have no temporal fields; use withoutTemporal() explicitly or execute the pinned request');
    if (typeof name!=='string' || !/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(name) || typeof description!=='string' || dataset!==null && typeof dataset!=='string') throw new TypeError('invalid stored-query metadata');
    const out={action:'load',name,description,template:this.toSparql(),params:[]}; if (dataset!==null) out.dataset=dataset; return out;
  }
  /** Return lossless native JSON text; do not coerce native i64 to JS Number. */
  execute(database) { return database.invoke('quipu_query',JSON.stringify(this.toRequest())); }
  save(database,name,options={}) { return database.invoke('quipu_queries',JSON.stringify(this.storedDefinition(name,options))); }
}
