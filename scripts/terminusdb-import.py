#!/usr/bin/env python3
"""Import a TerminusDB database into Quipu: schema -> SHACL, instance -> one episode.

TerminusDB's community build serves each graph as Turtle at
``GET /api/triples/<org>/<db>/local/{branch/<b>|commit/<id>}/{schema|instance}``
and the commit history as JSON at ``GET /api/log/<org>/<db>/local/branch/<b>``.
The schema graph is triples in TerminusDB's ``sys:`` vocabulary
(``http://terminusdb.com/schema/sys#``), produced by ``json_schema_triple`` in
``src/core/document/json.pl``. This script reads that vocabulary and writes:

* ``shapes.ttl``   one ``sh:NodeShape`` per ``sys:Class`` (``sh:targetClass``),
                   with cardinality from Optional/Set/Cardinality/mandatory,
                   ``sh:in`` for Enum ranges, ``rdfs:subClassOf`` for
                   ``sys:inherits`` (inherited properties are copied onto the
                   child shape, since TerminusDB instances carry only their most
                   specific ``rdf:type``);
* ``instance.nt``  the instance graph as N-Triples, IRIs kept (see "IRIs" below);
* ``import.json``  a report: origin commit, counts, anything not translated.

Then, optionally, it loads them into Quipu as ONE episode whose transaction
``source`` names the origin commit (``terminusdb:<org>/<db>/local/commit/<id>``),
either over REST (``--quipu-url``, ``POST /shapes`` + ``POST /knot``) or into a
local store (``--quipu-db``, ``quipu shapes load`` + ``quipu ingest --source``).
``--replay-log`` (REST only) instead replays the branch history oldest first:
each commit's instance graph is posted as a ``replace_snapshot`` knot whose
transaction and valid time are the commit time.

IRIs: TerminusDB's own IRIs are kept verbatim by default, so links survive the
move. A ``@key: Lexical`` document is named
``<data base><Class>/<v1>+<v2>...`` where each key value is URI-segment encoded
and ``+`` inside a value becomes ``%2B`` (``idgen_lexical`` in json.pl).
``lexical_iri()`` reproduces that rule for minting new records. ``--rebase-data``
and ``--rebase-schema`` swap the ``terminusdb:///data/`` and
``terminusdb:///schema#`` prefixes for your own namespaces.

Stdlib only. The Turtle reader covers what TerminusDB's dump emits (no blank
nodes are minted by TerminusDB; lists are explicit ``rdf:List`` cons cells).
"""
import argparse
import base64
import datetime as dt
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path

SYS = 'http://terminusdb.com/schema/sys#'
RDF = 'http://www.w3.org/1999/02/22-rdf-syntax-ns#'
RDFS = 'http://www.w3.org/2000/01/rdf-schema#'
XSD = 'http://www.w3.org/2001/XMLSchema#'
SH = 'http://www.w3.org/ns/shacl#'
TYPE = RDF + 'type'
DEFAULT_SCHEMA = 'terminusdb:///schema#'
DEFAULT_DATA = 'terminusdb:///data/'


# --------------------------------------------------------------------------
# Terms: ('iri', value) | ('bnode', label) | ('lit', lexical, datatype, lang)

def iri(v):
    return ('iri', v)


def lit(v, dtype=XSD + 'string', lang=None):
    return ('lit', v, None if lang else dtype, lang)


class TurtleError(ValueError):
    pass


class Turtle:
    """A small Turtle 1.1 reader: prefixes, base, IRIs, prefixed names, ``a``,
    literals (all quote forms, escapes, ^^, @lang, numbers, booleans), ``;``
    ``,`` ``.``, blank nodes, ``[ ]`` property lists and ``( )`` collections."""

    TOKEN = re.compile(r'''
        (?P<ws>\s+|\#[^\n]*)
      | (?P<iri><[^<>"{}|^`\\\s]*>)
      | (?P<str>"""(?:[^"\\]|\\.|"(?!""))*"""|'\'\'(?:[^'\\]|\\.|'(?!''))*'\'\'|"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*')
      | (?P<lang>@[A-Za-z]+(?:-[A-Za-z0-9]+)*)
      | (?P<dt>\^\^)
      | (?P<num>[+-]?(?:\d+\.\d*(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|\d+[eE][+-]?\d+|\d+))
      | (?P<bnode>_:[A-Za-z0-9_][A-Za-z0-9_.\-]*)
      | (?P<pname>[A-Za-z][\w.\-]*?:(?:[\w\-%:]|\\.|\.(?=[\w\-%:]))*|:(?:[\w\-%:]|\\.|\.(?=[\w\-%:]))*)
      | (?P<word>[A-Za-z]+)
      | (?P<punct>[.;,\[\]()])
    ''', re.X)
    ESC = {'t': '\t', 'b': '\b', 'n': '\n', 'r': '\r', 'f': '\f', '"': '"', "'": "'", '\\': '\\'}

    def __init__(self, text, base=''):
        self.toks = []
        pos = 0
        while pos < len(text):
            m = self.TOKEN.match(text, pos)
            if not m:
                raise TurtleError(f'cannot tokenize at {text[pos:pos + 40]!r}')
            if m.lastgroup != 'ws':
                self.toks.append((m.lastgroup, m.group()))
            pos = m.end()
        self.i, self.base, self.prefixes, self.triples, self.bn = 0, base, {}, [], 0

    def peek(self):
        return self.toks[self.i] if self.i < len(self.toks) else (None, None)

    def take(self, want=None):
        tok = self.peek()
        if tok[0] is None or (want and tok[1] != want):
            raise TurtleError(f'expected {want or "token"}, got {tok[1]!r}')
        self.i += 1
        return tok

    def unescape(self, s):
        out, k = [], 0
        while k < len(s):
            c = s[k]
            if c == '\\':
                n = s[k + 1]
                if n in 'uU':
                    width = 4 if n == 'u' else 8
                    out.append(chr(int(s[k + 2:k + 2 + width], 16)))
                    k += 2 + width
                    continue
                out.append(self.ESC.get(n, n))
                k += 2
                continue
            out.append(c)
            k += 1
        return ''.join(out)

    def resolve(self, ref):
        ref = self.unescape(ref)
        if re.match(r'^[A-Za-z][A-Za-z0-9+.\-]*:', ref) or not self.base:
            return ref
        return urllib.parse.urljoin(self.base, ref)

    def parse(self):
        while self.peek()[0] is not None:
            kind, val = self.peek()
            if val in ('@prefix', '@base') or (kind == 'word' and val.upper() in ('PREFIX', 'BASE')):
                self.directive()
            else:
                self.statement()
        return self.triples

    def directive(self):
        _, val = self.take()
        sparql = not val.startswith('@')
        if val.lstrip('@').upper() == 'PREFIX':
            _, name = self.take()
            if not name.endswith(':'):
                raise TurtleError(f'bad prefix name {name!r}')
            _, ref = self.take()
            self.prefixes[name[:-1]] = self.resolve(ref[1:-1])
        else:
            _, ref = self.take()
            self.base = self.resolve(ref[1:-1])
        if not sparql:
            self.take('.')

    def fresh(self):
        self.bn += 1
        return ('bnode', f'b{self.bn}')

    def statement(self):
        subj = self.term(subject=True)
        if self.peek()[1] != '.':
            self.predicate_objects(subj)
        self.take('.')

    def predicate_objects(self, subj):
        while True:
            pred = self.term(predicate=True)
            while True:
                self.triples.append((subj, pred, self.term()))
                if self.peek()[1] != ',':
                    break
                self.take(',')
            if self.peek()[1] != ';':
                return
            while self.peek()[1] == ';':
                self.take(';')
            if self.peek()[1] in ('.', ']'):
                return

    def term(self, subject=False, predicate=False):
        kind, val = self.take()
        if kind == 'iri':
            return iri(self.resolve(val[1:-1]))
        if kind == 'pname':
            prefix, _, local = val.partition(':')
            if prefix not in self.prefixes:
                raise TurtleError(f'undeclared prefix {prefix!r}')
            return iri(self.prefixes[prefix] + re.sub(r'\\(.)', r'\1', local))
        if kind == 'word' and val == 'a' and predicate:
            return iri(TYPE)
        if predicate:
            raise TurtleError(f'bad predicate {val!r}')
        if kind == 'bnode':
            return ('bnode', val[2:])
        if val == '[':
            node = self.fresh()
            if self.peek()[1] != ']':
                self.predicate_objects(node)
            self.take(']')
            return node
        if val == '(':
            items = []
            while self.peek()[1] != ')':
                items.append(self.term())
            self.take(')')
            head = iri(RDF + 'nil')
            for item in reversed(items):
                cell = self.fresh()
                self.triples.append((cell, iri(RDF + 'first'), item))
                self.triples.append((cell, iri(RDF + 'rest'), head))
                head = cell
            return head
        if subject:
            raise TurtleError(f'bad subject {val!r}')
        if kind == 'str':
            q = 3 if val[:3] in ('"""', "'''") else 1
            text = self.unescape(val[q:-q])
            if self.peek()[0] == 'lang':
                return lit(text, lang=self.take()[1][1:])
            if self.peek()[0] == 'dt':
                self.take()
                return lit(text, self.term()[1])
            return lit(text)
        if kind == 'num':
            dtype = 'double' if 'e' in val.lower() else 'decimal' if '.' in val else 'integer'
            return lit(val, XSD + dtype)
        if kind == 'word' and val in ('true', 'false'):
            return lit(val, XSD + 'boolean')
        raise TurtleError(f'unexpected {val!r}')


def parse_turtle(text, base=''):
    return Turtle(text, base).parse()


def nt_term(t):
    if t[0] == 'iri':
        return '<' + ''.join(c if c not in '<>"{}|^`\\' and ord(c) > 0x20 else f'\\u{ord(c):04X}' for c in t[1]) + '>'
    if t[0] == 'bnode':
        return '_:' + t[1]
    text = t[1].replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n').replace('\r', '\\r')
    if t[3]:
        return f'"{text}"@{t[3]}'
    if t[2] and t[2] != XSD + 'string':
        return f'"{text}"^^<{t[2]}>'
    return f'"{text}"'


def to_ntriples(triples):
    return ''.join(f'{nt_term(s)} {nt_term(p)} {nt_term(o)} .\n' for s, p, o in triples)


# --------------------------------------------------------------------------
# IRIs

def encode_fragment(value):
    """One key value as TerminusDB encodes it (encode_id_fragment, json.pl)."""
    return urllib.parse.quote(str(value), safe="!$&'()*,;=:@-._~").replace('+', '%2B')


def lexical_iri(data_base, cls, values):
    """The IRI TerminusDB mints for a ``@key: Lexical`` document."""
    return f'{data_base}{cls}/' + '+'.join(encode_fragment(v) for v in values)


def rebaser(pairs):
    pairs = [(old, new) for old, new in pairs if new]

    def fix(t):
        if t[0] == 'iri':
            for old, new in pairs:
                if t[1].startswith(old):
                    return iri(new + t[1][len(old):])
        if t[0] == 'lit' and t[2]:
            for old, new in pairs:
                if t[2].startswith(old):
                    return ('lit', t[1], new + t[2][len(old):], t[3])
        return t
    return lambda triples: [(fix(s), fix(p), fix(o)) for s, p, o in triples]


# --------------------------------------------------------------------------
# Schema -> SHACL

class Schema:
    def __init__(self, triples):
        self.out = {}
        for s, p, o in triples:
            self.out.setdefault(s, {}).setdefault(p, []).append(o)

    def objs(self, s, p):
        return self.out.get(s, {}).get(iri(p) if isinstance(p, str) else p, [])

    def one(self, s, p):
        v = self.objs(s, p)
        return v[0] if v else None

    def types(self, s):
        return {o[1] for o in self.objs(s, TYPE)}

    def rdf_list(self, head):
        items, seen = [], set()
        while head and head != iri(RDF + 'nil') and head not in seen:
            seen.add(head)
            first = self.one(head, RDF + 'first')
            if first is not None:
                items.append(first)
            head = self.one(head, RDF + 'rest')
        return items

    def int_of(self, s, p):
        v = self.one(s, SYS + p)
        return int(float(v[1])) if v and v[0] == 'lit' else None


SKIP_PREDICATES = {TYPE} | {SYS + k for k in (
    'key', 'inherits', 'documentation', 'abstract', 'subdocument', 'unfoldable',
    'base', 'value', 'metadata', 'oneOf', 'shared', 'unfold')}
DATATYPE_NAMESPACES = (XSD, RDF + 'langString', SYS + 'JSON')


def property_constraints(schema, range_term, classes, enums, notes, where):
    """Map one property's range node to (min, max, value-constraint dict)."""
    kinds = schema.types(range_term)
    family = next((k[len(SYS):] for k in kinds if k.startswith(SYS) and k[len(SYS):] in (
        'Optional', 'Set', 'Cardinality', 'List', 'Array', 'Table', 'Foreign')), None)
    lo, hi, target = 1, 1, range_term
    if family:
        target = schema.one(range_term, SYS + 'class')
        mn, mx = schema.int_of(range_term, 'min_cardinality'), schema.int_of(range_term, 'max_cardinality')
        if family == 'Optional':
            lo, hi = 0, 1
        elif family in ('Set', 'Cardinality'):
            lo, hi = mn or 0, mx
        elif family == 'Foreign':
            lo, hi = 0, None
            notes.append(f'{where}: Foreign range kept as an untyped reference')
            return lo, hi, {'nodeKind': SH + 'IRI'}
        else:
            # List and Array values are cons/array cells, not the element itself.
            notes.append(f'{where}: {family} of <{target[1] if target else "?"}> left without an element constraint')
            return (1 if family == 'List' else 0), (1 if family == 'List' else None), {}
    if not target or target[0] != 'iri':
        notes.append(f'{where}: unreadable range')
        return lo, hi, {}
    t = target[1]
    if t in enums:
        return lo, hi, {'in': enums[t]}
    if t in classes:
        return lo, hi, {'class': t}
    if t.startswith(DATATYPE_NAMESPACES):
        return lo, hi, {'datatype': t}
    notes.append(f'{where}: range <{t}> is not a known class or datatype; emitted as sh:class')
    return lo, hi, {'class': t}


def translate_schema(triples):
    """Return (classes, enums, notes) from TerminusDB schema triples.

    classes: {class_iri: {'parents': [...], 'props': {pred: (min, max, constraint)},
                          'key': (kind, [fields]) | None, 'abstract': bool,
                          'subdocument': bool, 'union': bool}}
    """
    schema = Schema(triples)
    notes = []
    class_nodes = [s for s in schema.out if s[0] == 'iri' and schema.types(s) & {SYS + 'Class', SYS + 'TaggedUnion'}]
    enum_nodes = [s for s in schema.out if s[0] == 'iri' and SYS + 'Enum' in schema.types(s)]
    enums = {e[1]: [v[1] for v in schema.rdf_list(schema.one(e, SYS + 'value'))] for e in enum_nodes}
    classes = {}
    for c in class_nodes:
        classes[c[1]] = {'parents': [o[1] for o in schema.objs(c, SYS + 'inherits')], 'props': {},
                         'key': None, 'abstract': bool(schema.objs(c, SYS + 'abstract')),
                         'subdocument': bool(schema.objs(c, SYS + 'subdocument')),
                         'union': SYS + 'TaggedUnion' in schema.types(c)}
    for c in class_nodes:
        info = classes[c[1]]
        for pred, values in schema.out[c].items():
            if pred[1] in SKIP_PREDICATES:
                if pred[1] == SYS + 'oneOf':
                    notes.append(f'<{c[1]}>: @oneOf not translated')
                continue
            info['props'][pred[1]] = property_constraints(schema, values[0], classes, enums, notes,
                                                          f'<{c[1]}> <{pred[1]}>')
        key = schema.one(c, SYS + 'key')
        if key:
            kind = next((k[len(SYS):] for k in schema.types(key) if k.startswith(SYS)), 'Random')
            info['key'] = (kind, [f[1] for f in schema.rdf_list(schema.one(key, SYS + 'fields'))])
    # Flatten inheritance: a child shape carries every ancestor property.
    def ancestors(name, seen=()):
        for parent in classes.get(name, {}).get('parents', []):
            if parent not in seen and parent in classes:
                yield parent
                yield from ancestors(parent, seen + (name,))
    for name, info in classes.items():
        inherited = {}
        for anc in reversed(list(ancestors(name))):
            inherited.update(classes[anc]['props'])
        info['all_props'] = {**inherited, **info['props']}
        if not info['key']:
            info['key'] = next((classes[a]['key'] for a in ancestors(name) if classes[a]['key']), None)
    return classes, enums, notes


def shape_iri(cls):
    return cls + '/Shape'


def ttl_iri(v):
    return nt_term(iri(v))


def write_shapes(classes, enums):
    lines = [f'@prefix sh: <{SH}> .', f'@prefix rdfs: <{RDFS}> .', f'@prefix xsd: <{XSD}> .', '']
    for name in sorted(classes):
        info = classes[name]
        for parent in info['parents']:
            lines.append(f'{ttl_iri(name)} rdfs:subClassOf {ttl_iri(parent)} .')
        body = [f'{ttl_iri(shape_iri(name))} a sh:NodeShape', f'    sh:targetClass {ttl_iri(name)}']
        if info['key'] and info['key'][0] == 'Lexical':
            fields = ', '.join(f.rsplit('#', 1)[-1].rsplit('/', 1)[-1] for f in info['key'][1])
            body.append(f'    rdfs:comment {json.dumps(f"TerminusDB @key Lexical on ({fields}): IRI = <data base>Class/value+value, values URI-segment encoded")}')
        for pred in sorted(info['all_props']):
            lo, hi, cons = info['all_props'][pred]
            if info['union']:
                lo = 0  # exactly-one is enforced by sh:xone below
            parts = [f'sh:path {ttl_iri(pred)}']
            if lo:
                parts.append(f'sh:minCount {lo}')
            if hi is not None:
                parts.append(f'sh:maxCount {hi}')
            if 'datatype' in cons:
                parts.append(f'sh:datatype {ttl_iri(cons["datatype"])}')
            if 'class' in cons:
                parts.append(f'sh:class {ttl_iri(cons["class"])}')
            if 'nodeKind' in cons:
                parts.append(f'sh:nodeKind {ttl_iri(cons["nodeKind"])}')
            if 'in' in cons:
                parts.append('sh:in ( ' + ' '.join(ttl_iri(v) for v in cons['in']) + ' )')
            body.append('    sh:property [ ' + ' ; '.join(parts) + ' ]')
        if info['union'] and info['props']:
            alts = ' '.join(f'[ sh:path {ttl_iri(p)} ; sh:minCount 1 ]' for p in sorted(info['props']))
            body.append(f'    sh:xone ( {alts} )')
        lines.append(' ;\n'.join(body) + ' .')
        lines.append('')
    # TerminusDB stores List values as rdf:List cons cells and Array values as
    # sys:Array cells. Quipu refuses an rdf:type no loaded shape sanctions, so the
    # cell types get shapes too (which also checks the cells are well formed).
    lines.append(CONTAINER_SHAPES)
    return '\n'.join(lines)


CONTAINER_SHAPES = f'''<urn:quipu:terminusdb:ListCellShape> a sh:NodeShape ;
    sh:targetClass <{RDF}List> ;
    sh:property [ sh:path <{RDF}first> ; sh:minCount 1 ; sh:maxCount 1 ] ;
    sh:property [ sh:path <{RDF}rest> ; sh:minCount 1 ; sh:maxCount 1 ] .

<urn:quipu:terminusdb:ArrayCellShape> a sh:NodeShape ;
    sh:targetClass <{SYS}Array> ;
    sh:property [ sh:path <{SYS}value> ; sh:maxCount 1 ] ;
    sh:property [ sh:path <{SYS}index> ; sh:minCount 1 ; sh:maxCount 1 ] .
'''


# --------------------------------------------------------------------------
# TerminusDB client

class TerminusDB:
    def __init__(self, server, timeout=60):
        self.server, self.timeout = server.rstrip('/'), timeout
        token = os.environ.get('TERMINUSDB_TOKEN')
        user, password = os.environ.get('TERMINUSDB_USER'), os.environ.get('TERMINUSDB_PASSWORD')
        if token:
            self.auth = 'Bearer ' + token
        elif user and password is not None:
            self.auth = 'Basic ' + base64.b64encode(f'{user}:{password}'.encode()).decode()
        else:
            self.auth = None

    def get(self, path, accept):
        req = urllib.request.Request(self.server + path, headers={'Accept': accept})
        if self.auth:
            req.add_header('Authorization', self.auth)
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            return resp.read().decode('utf-8')

    def triples(self, resource, graph):
        body = self.get(f'/api/triples/{resource}/{graph}', 'text/turtle')
        # Without a text/turtle match the handler replies with the Turtle as a JSON string.
        return json.loads(body) if body.lstrip().startswith('"') else body

    def log(self, db_path, branch, count=None):
        q = f'?count={count}' if count else ''
        return json.loads(self.get(f'/api/log/{db_path}/local/branch/{branch}{q}', 'application/json'))


def commit_source(db_path, commit_id):
    return f'terminusdb:{db_path}/local/commit/{commit_id}'


def iso(ts):
    return dt.datetime.fromtimestamp(float(ts), dt.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


# --------------------------------------------------------------------------
# Quipu loaders

def quipu_post(url, path, payload, token_env):
    req = urllib.request.Request(url.rstrip('/') + path, data=json.dumps(payload).encode(),
                                 headers={'Content-Type': 'application/json'})
    token = os.environ.get(token_env) if token_env else None
    if token:
        req.add_header('Authorization', 'Bearer ' + token)
    try:
        with urllib.request.urlopen(req, timeout=300) as resp:
            body = json.loads(resp.read() or b'{}')
    except urllib.error.HTTPError as e:
        raise SystemExit(f'quipu {path} refused ({e.code}): {e.read().decode(errors="replace")[:500]}')
    # A SHACL refusal is HTTP 200 with conforms: false and nothing written.
    if body.get('conforms') is False:
        issues = '; '.join(f"{i.get('focus_node')} {i.get('path')}: {i.get('message')}" for i in body.get('issues', [])[:5])
        raise SystemExit(f'quipu {path} refused by the shapes ({body.get("violations")} violations): {issues}')
    return body


def load_rest(args, shapes, instance_nt, source, commit_time):
    out = {'shapes': quipu_post(args.quipu_url, '/shapes', {'action': 'load', 'name': args.shapes_name,
                                                             'turtle': shapes}, args.quipu_token_env)}
    knot = {'turtle': instance_nt, 'actor': args.actor, 'source': source}
    if commit_time:
        knot['valid_from'] = commit_time
    if args.graph:
        knot['graph'] = args.graph
    out['knot'] = quipu_post(args.quipu_url, '/knot', knot, args.quipu_token_env)
    return out


def load_cli(args, shapes_path, instance_path, instance_nt, source, commit_time):
    q = args.quipu_bin
    run = lambda cmd: subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
    out = {'shapes': run([q, 'shapes', 'load', args.shapes_name, shapes_path, '--db', args.quipu_db])}
    cmd = [q, 'ingest', instance_path, '--graph', args.graph or 'urn:terminusdb:import', '--format', 'nt',
           '--timestamp', commit_time or iso(dt.datetime.now().timestamp()),
           '--declare-count', str(instance_nt.count('\n')),
           '--declare-sha256', hashlib.sha256(instance_nt.encode()).hexdigest(),
           '--actor', args.actor, '--source', source, '--db', args.quipu_db]
    out['ingest'] = run(cmd)
    return out


def replay_log(args, tdb, db_path, rebase):
    """Post each commit's instance graph, oldest first, as a snapshot replacement."""
    log = sorted(tdb.log(db_path, args.branch, args.log_count), key=lambda c: float(c['timestamp']))
    results = []
    for commit in log:
        triples = rebase(parse_turtle(tdb.triples(f'{db_path}/local/commit/{commit["identifier"]}', 'instance')))
        when = iso(commit['timestamp'])
        knot = {'turtle': to_ntriples(triples), 'actor': args.actor,
                'source': commit_source(db_path, commit['identifier']),
                'timestamp': when, 'valid_from': when,
                'replace_snapshot': True, 'snapshot': f'terminusdb:{db_path}/local/branch/{args.branch}'}
        if args.graph:
            knot['graph'] = args.graph
        resp = quipu_post(args.quipu_url, '/knot', knot, args.quipu_token_env)
        results.append({'commit': commit['identifier'], 'time': when, 'triples': len(triples), 'quipu': resp})
    return results


# --------------------------------------------------------------------------

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    src = ap.add_argument_group('source (a live server, or exported Turtle files)')
    src.add_argument('--server', help='TerminusDB URL, e.g. http://localhost:6363 '
                     '(auth: TERMINUSDB_TOKEN, or TERMINUSDB_USER + TERMINUSDB_PASSWORD)')
    src.add_argument('--db', help='<org>/<db>, e.g. admin/mydb')
    src.add_argument('--branch', default='main')
    src.add_argument('--schema-ttl', help='schema graph Turtle (offline)')
    src.add_argument('--instance-ttl', help='instance graph Turtle (offline)')
    src.add_argument('--commit', help='origin commit id (offline; names the episode source)')
    src.add_argument('--commit-time', help='origin commit time, RFC 3339 or epoch seconds (offline)')
    ap.add_argument('--out', required=True, help='directory for shapes.ttl, instance.nt, import.json')
    ap.add_argument('--rebase-data', help=f'replace the {DEFAULT_DATA} prefix with this namespace')
    ap.add_argument('--rebase-schema', help=f'replace the {DEFAULT_SCHEMA} prefix with this namespace')
    dst = ap.add_argument_group('load into quipu (optional)')
    dst.add_argument('--quipu-url', help='quipu-server URL (POST /shapes, /knot)')
    dst.add_argument('--quipu-token-env', default='QUIPU_TOKEN', help='env var holding the bearer token')
    dst.add_argument('--quipu-db', help='local store path (quipu shapes load + quipu ingest)')
    dst.add_argument('--quipu-bin', default='quipu')
    dst.add_argument('--graph', help='target named graph IRI (must exist for REST; ingest creates it)')
    dst.add_argument('--shapes-name', default='terminusdb')
    dst.add_argument('--actor', default='terminusdb-import')
    dst.add_argument('--replay-log', action='store_true', help='replay /api/log as tx history (REST only)')
    dst.add_argument('--log-count', type=int, help='replay at most the N most recent commits')
    args = ap.parse_args(argv)

    rebase = rebaser([(DEFAULT_DATA, args.rebase_data), (DEFAULT_SCHEMA, args.rebase_schema)])
    tdb = None
    if args.server:
        if not args.db:
            ap.error('--server needs --db <org>/<db>')
        tdb = TerminusDB(args.server)
        head = tdb.log(args.db, args.branch, 1)
        if not head:
            raise SystemExit(f'{args.db} branch {args.branch} has no commits')
        commit_id, commit_time = head[0]['identifier'], iso(head[0]['timestamp'])
        resource = f'{args.db}/local/commit/{commit_id}'  # pinned: both graphs from the same commit
        schema_ttl, instance_ttl = tdb.triples(resource, 'schema'), tdb.triples(resource, 'instance')
        db_path = args.db
    elif args.schema_ttl and args.instance_ttl:
        schema_ttl, instance_ttl = (Path(p).read_text(encoding='utf-8') for p in (args.schema_ttl, args.instance_ttl))
        commit_id, db_path = args.commit or 'unknown', args.db or 'local/unknown'
        ct = args.commit_time
        commit_time = iso(ct) if ct and re.fullmatch(r'\d+(\.\d+)?', ct) else ct
    else:
        ap.error('give --server and --db, or --schema-ttl and --instance-ttl')

    schema_triples = rebase(parse_turtle(schema_ttl))
    instance_triples = rebase(parse_turtle(instance_ttl))
    classes, enums, notes = translate_schema(schema_triples)
    shapes = write_shapes(classes, enums)
    instance_nt = to_ntriples(instance_triples)
    source = commit_source(db_path, commit_id)

    os.makedirs(args.out, exist_ok=True)
    shapes_path, instance_path = os.path.join(args.out, 'shapes.ttl'), os.path.join(args.out, 'instance.nt')
    with open(shapes_path, 'w', encoding='utf-8') as f:
        f.write(shapes)
    with open(instance_path, 'w', encoding='utf-8') as f:
        f.write(instance_nt)
    report = {'source': source, 'commit': commit_id, 'commit_time': commit_time,
              'classes': len(classes), 'enums': len(enums), 'instance_triples': len(instance_triples),
              'lexical_keys': {c: i['key'][1] for c, i in classes.items() if i['key'] and i['key'][0] == 'Lexical'},
              'not_translated': notes}
    if args.quipu_url and args.replay_log:
        if not tdb:
            ap.error('--replay-log needs --server')
        quipu_post(args.quipu_url, '/shapes', {'action': 'load', 'name': args.shapes_name, 'turtle': shapes},
                   args.quipu_token_env)
        report['replay'] = replay_log(args, tdb, db_path, rebase)
    elif args.quipu_url:
        report['quipu'] = load_rest(args, shapes, instance_nt, source, commit_time)
    elif args.quipu_db:
        report['quipu'] = load_cli(args, shapes_path, instance_path, instance_nt, source, commit_time)
    with open(os.path.join(args.out, 'import.json'), 'w', encoding='utf-8') as f:
        json.dump(report, f, indent=2)
    print(json.dumps({k: v for k, v in report.items() if k not in ('replay', 'quipu')}, indent=2))
    for note in notes:
        print('note:', note, file=sys.stderr)


if __name__ == '__main__':
    main()
