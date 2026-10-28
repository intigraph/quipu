/** Typed SPARQL builder over the WASM Database's native JSON-text facade. */
export abstract class Expr { protected readonly _expression: never; abstract sparql(): string; }
export class Var extends Expr { readonly name: string; constructor(name: string); sparql(): string; }
export class Iri extends Expr { readonly value: string; constructor(value: string); sparql(): string; }
export class Literal extends Expr {
  readonly value: string | number | bigint | boolean;
  constructor(value: string | number | bigint | boolean, options?: {datatype?: Iri | null; language?: string | null});
  sparql(): string;
}
export type Comparison = '=' | '!=' | '<' | '>' | '<=' | '>=' | '&&' | '||' | '+' | '-' | '*' | '/';
export class Compare extends Expr { constructor(left: Expr, op: Comparison, right: Expr); sparql(): string; }
export type FunctionName = 'STR' | 'LANG' | 'DATATYPE' | 'BOUND' | 'CONTAINS' | 'STRSTARTS' | 'STRENDS' | 'LCASE' | 'UCASE' | 'ISIRI' | 'ISLITERAL' | 'REGEX';
export class Call extends Expr { constructor(fn: FunctionName, args: readonly Expr[]); sparql(): string; }
export class Path { constructor(op: '/' | '|' | '^' | '*' | '+' | '?', args: readonly (Iri | Path)[]); sparql(): string; }
export class Aggregate { readonly alias: Var; constructor(fn: 'COUNT' | 'SUM' | 'AVG' | 'MIN' | 'MAX' | 'SAMPLE', value: Expr | null, alias: Var, options?: {distinct?: boolean}); sparql(): string; }
export abstract class Pattern { protected readonly _pattern: never; abstract sparql(): string; }
export class Triple extends Pattern { constructor(subject: Iri | Var, predicate: Iri | Var | Path, object: Iri | Var | Literal); sparql(): string; }
export class Group extends Pattern { readonly patterns: readonly Pattern[]; constructor(patterns: readonly Pattern[]); sparql(): string; }
export class Optional extends Pattern { constructor(group: Group); sparql(): string; }
export class Union extends Pattern { constructor(groups: readonly Group[]); sparql(): string; }
export class Filter extends Pattern { constructor(expression: Expr); sparql(): string; }
export class Graph extends Pattern { constructor(graph: Iri | Var, group: Group); sparql(): string; }
export interface Database { invoke(operation: string, input: string): string; }
export interface StoredOptions { description?: string; dataset?: string | null; }
export interface QueryRequest { query: string; valid_at?: string; tx?: number; }
export interface StoredDefinition { action: 'load'; name: string; description: string; template: string; params: []; dataset?: string; }
export class Select {
  readonly projections: readonly (Var | Aggregate)[];
  readonly patterns: readonly Pattern[];
  constructor(projections: readonly (Var | Aggregate)[], options?: {distinct?: boolean});
  where(...patterns: Pattern[]): Select;
  groupBy(...variables: Var[]): Select;
  fromGraph(...graphs: Iri[]): Select;
  fromNamed(...graphs: Iri[]): Select;
  limit(count: number): Select;
  validAt(instant: string): Select;
  asOfTx(tx: number): Select;
  withoutTemporal(): Select;
  toSparql(): string;
  toRequest(): QueryRequest;
  storedDefinition(name: string, options?: StoredOptions): StoredDefinition;
  execute(database: Database): string;
  save(database: Database, name: string, options?: StoredOptions): string;
}
