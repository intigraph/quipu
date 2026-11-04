/** Immutable typed SPARQL terms. Strings are values, never source fragments. */
export class Expr { sparql() { throw new TypeError('abstract expression'); } }
export function text(value) {
  if (typeof value !== 'string' || [...value].some(c => c.codePointAt(0) >= 0xd800 && c.codePointAt(0) <= 0xdfff))
    throw new TypeError('expected Unicode scalar text');
  return value;
}
export class Var extends Expr {
  constructor(name) { super(); if (typeof name !== 'string' || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) throw new TypeError('invalid variable name'); this.name=name; Object.freeze(this); }
  sparql() { return '?'+this.name; }
}
export class Iri extends Expr {
  constructor(value) {
    super(); text(value);
    if (!/^[A-Za-z][A-Za-z0-9+.-]*:/.test(value) || /[\s\x00-\x20<>"{}|^`\\]/u.test(value)) throw new TypeError('expected an absolute IRI');
    this.value=value; Object.freeze(this);
  }
  sparql() { return '<'+this.value+'>'; }
}
export class Literal extends Expr {
  constructor(value, {datatype=null, language=null}={}) {
    super();
    if (!(typeof value==='string' || typeof value==='boolean' || typeof value==='bigint' || typeof value==='number' && Number.isSafeInteger(value)))
      throw new TypeError('literal requires text, safe integer, bigint or boolean; use a typed lexical string for other datatypes');
    if (typeof value==='string') text(value);
    if (datatype!==null && !(datatype instanceof Iri)) throw new TypeError('datatype requires Iri');
    if (language!==null && (typeof language!=='string' || !/^[A-Za-z]+(?:-[A-Za-z0-9]+)*$/.test(language))) throw new TypeError('invalid language tag');
    if (datatype!==null && language!==null || typeof value!=='string' && (datatype!==null || language!==null)) throw new TypeError('datatype/language requires lexical string and is mutually exclusive');
    Object.assign(this,{value,datatype,language}); Object.freeze(this);
  }
  sparql() {
    if (typeof this.value!=='string') return String(this.value);
    // Avoid the registry treating a literal {ident} as a template placeholder.
    let out=JSON.stringify(this.value).replaceAll('{','\\u007B').replaceAll('}','\\u007D');
    if (this.datatype!==null) out+='^^'+this.datatype.sparql();
    if (this.language!==null) out+='@'+this.language;
    return out;
  }
}
export function expression(value) { if (!(value instanceof Expr)) throw new TypeError('expected typed expression'); return value; }
export class Compare extends Expr {
  constructor(left,op,right) { super(); expression(left); expression(right); if (!['=','!=','<','>','<=','>=','&&','||','+','-','*','/'].includes(op)) throw new TypeError('unsupported operator'); Object.assign(this,{left,op,right}); Object.freeze(this); }
  sparql() { return `(${this.left.sparql()} ${this.op} ${this.right.sparql()})`; }
}
export class Call extends Expr {
  constructor(fn,args) {
    super(); const arities={STR:[1],LANG:[1],DATATYPE:[1],BOUND:[1],CONTAINS:[2],STRSTARTS:[2],STRENDS:[2],LCASE:[1],UCASE:[1],ISIRI:[1],ISLITERAL:[1],REGEX:[2,3]};
    if (!Object.hasOwn(arities,fn) || !Array.isArray(args) || !arities[fn].includes(args.length)) throw new TypeError('unsupported function or arity');
    args.forEach(expression); if (fn==='BOUND' && !(args[0] instanceof Var)) throw new TypeError('BOUND requires Var');
    this.function=fn; this.args=Object.freeze([...args]); Object.freeze(this);
  }
  sparql() { return this.function+'('+this.args.map(x=>x.sparql()).join(', ')+')'; }
}
export class Path {
  constructor(op,args) {
    if (!['/','|','^','*','+','?'].includes(op) || !Array.isArray(args) || (['/','|'].includes(op)?args.length<2:args.length!==1)) throw new TypeError('invalid path operator or arity');
    if (!args.every(a=>a instanceof Iri || a instanceof Path)) throw new TypeError('paths require Iri/Path');
    this.op=op; this.args=Object.freeze([...args]); Object.freeze(this);
  }
  sparql() { if (['/','|'].includes(this.op)) return '('+this.args.map(a=>a.sparql()).join(this.op)+')'; const inner='('+this.args[0].sparql()+')'; return this.op==='^'?'^'+inner:inner+this.op; }
}
export class Aggregate {
  constructor(fn,value,alias,{distinct=false}={}) {
    if (!['COUNT','SUM','AVG','MIN','MAX','SAMPLE'].includes(fn) || value===null && fn!=='COUNT') throw new TypeError('unsupported aggregate');
    if (value!==null) expression(value);
    if (!(alias instanceof Var) || typeof distinct!=='boolean') throw new TypeError('invalid aggregate alias/distinct');
    Object.assign(this,{function:fn,value,alias,distinct}); Object.freeze(this);
  }
  sparql() { return `(${this.function}(${this.distinct?'DISTINCT ':''}${this.value===null?'*':this.value.sparql()}) AS ${this.alias.sparql()})`; }
}
