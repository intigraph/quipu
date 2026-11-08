#!/usr/bin/env python3
"""Tests for scripts/terminusdb-import.py.

Two fixture sets:

* test-fixtures/terminusdb/live/  REAL exports, captured from
  terminusdb/terminusdb-server:v12.0.8 (digest in live/README.md): the schema
  graph, the instance graph at every commit, and /api/log. These carry what
  TerminusDB's writer really emits: @base with relative IRIs, numbers in Turtle
  shorthand, sys:JSON nodes, Array cells, a schema-only first commit and
  sub-second commit times.
* test-fixtures/terminusdb/*.ttl  format-derived fixtures built from
  json_schema_triple and its tests in src/core/document/json.pl (TerminusDB
  0b5549ac), written with absolute IRIs, plus an invalid instance graph.

A stdlib HTTP double serves the live fixtures on /api/log and /api/triples so
the --server and --replay-log paths run offline.

When ``quipu`` / ``quipu-server`` are on PATH (or QUIPU_BIN / QUIPU_SERVER_BIN
point at them) the shapes are checked by the real engine and the REST load and
log replay run against a throwaway server; otherwise those tests skip.

With TERMINUSDB_LIVE_URL (+ TERMINUSDB_USER / TERMINUSDB_PASSWORD) set, the
LiveTerminusDB test seeds a fresh database on that server from
live/schema.json and imports it end to end:

    docker run -d --name tdb -p 127.0.0.1:6363:6363 -e TERMINUSDB_ADMIN_PASS=pw \\
        terminusdb/terminusdb-server:v12.0.8
    TERMINUSDB_LIVE_URL=http://127.0.0.1:6363 TERMINUSDB_USER=admin TERMINUSDB_PASSWORD=pw \\
        python3 scripts/test-terminusdb-import.py
"""
import base64
import contextlib
import http.server
import io
import importlib.util
import json
import os
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.request
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIX = HERE.parent / 'test-fixtures' / 'terminusdb'
LIVE = FIX / 'live'
spec = importlib.util.spec_from_file_location('tdb', HERE / 'terminusdb-import.py')
tdb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tdb)
S = 'terminusdb:///schema#'
D = 'terminusdb:///data/'
QUIPU = os.environ.get('QUIPU_BIN') or shutil.which('quipu')
QUIPU_SERVER = os.environ.get('QUIPU_SERVER_BIN') or shutil.which('quipu-server')
LOG = json.loads((LIVE / 'log.json').read_text())          # newest first, as /api/log returns it
HEAD = LOG[0]['identifier']
ADA, GRACE = D + 'Person/Ada%20Lovelace', D + 'Person/Grace%20Hopper'


def one_import_after_its_shape_settlement(changes, transactions, loaded, source, shapes_name):
    """The CLI load's transaction, identified by its own ingest completion
    marker rather than by position, after checking that the load is ONE write
    sourced from the commit and that the only other transaction is the shape
    load's settlement, named by its `shapes.loaded` receipt and carrying no
    facts. Raises AssertionError otherwise."""
    marked = {r['tx'] for r in changes if r['attribute'] == 'urn:quipu:ingest:complete'}
    if len(marked) != 1:
        raise AssertionError(f'expected one ingest completion transaction, got {sorted(marked)}')
    (import_tx,) = marked
    pairs = {(r['source'], r['tx']) for r in changes}
    if pairs != {(source, import_tx)}:
        raise AssertionError(f'every change must be the import transaction {import_tx} from {source}: {pairs}')
    if len(loaded) != 1:
        raise AssertionError(f'expected one shapes.loaded receipt, got {len(loaded)}')
    shape_tx, receipt = loaded[0]
    if (receipt.get('tx'), receipt.get('name'), receipt.get('registry')) != (shape_tx, shapes_name, 'shapes'):
        raise AssertionError(f'shapes.loaded receipt does not name its transaction {shape_tx}: {receipt}')
    expected = [(shape_tx, 'quipu:shape-mapping'), (import_tx, source)]
    if [tuple(t[:2]) for t in transactions] != expected:
        raise AssertionError(f'transactions must be the shape settlement then the import: {transactions}')
    return import_tx


def run_main(argv):
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        tdb.main(argv)


def fixture(name):
    return (FIX / name).read_text(encoding='utf-8')


def live_instance(commit=HEAD):
    return LIVE / f'instance-{commit}.ttl'


def validate(schema_ttl, instance_ttl):
    out = tempfile.mkdtemp()
    run_main(['--schema-ttl', str(schema_ttl), '--instance-ttl', str(instance_ttl), '--out', out])
    return subprocess.run([QUIPU, 'validate', '--shapes', f'{out}/shapes.ttl', '--data', f'{out}/instance.nt'],
                          capture_output=True, text=True)


class TurtleReader(unittest.TestCase):
    def test_prefixes_literals_and_escapes(self):
        t = tdb.parse_turtle('@prefix ex: <http://e/> . PREFIX y: <http://y/>\n'
                             'ex:a ex:p "x\\"y\\u00e9", \'\'\'multi\nline\'\'\' ; y:q 3, 2.5, true, "hi"@en-GB ; '
                             'a ex:C ; ex:r "7"^^<http://www.w3.org/2001/XMLSchema#integer> .')
        objs = [o for _, _, o in t]
        self.assertIn(tdb.lit('x"yé'), objs)
        self.assertIn(tdb.lit('multi\nline'), objs)
        self.assertIn(tdb.lit('3', tdb.XSD + 'integer'), objs)
        self.assertIn(tdb.lit('2.5', tdb.XSD + 'decimal'), objs)
        self.assertIn(tdb.lit('true', tdb.XSD + 'boolean'), objs)
        self.assertIn(tdb.lit('hi', lang='en-GB'), objs)
        self.assertIn((tdb.iri('http://e/a'), tdb.iri(tdb.TYPE), tdb.iri('http://e/C')), t)

    def test_collections_bnodes_base_and_escaped_local_names(self):
        t = tdb.parse_turtle('@base <http://b/> . @prefix s: <http://s#> .\n'
                             '<x> <p> ( <a> <b> ) ; <q> [ <r> s:Cons\\/1 ] .')
        self.assertEqual(tdb.Schema(t).rdf_list(next(o for s, p, o in t if p[1] == 'http://b/p')),
                         [tdb.iri('http://b/a'), tdb.iri('http://b/b')])
        self.assertIn(tdb.iri('http://s#Cons/1'), [o for _, _, o in t])

    def test_rfc3986_resolution_for_any_scheme(self):
        r = tdb.resolve_iri
        # RFC 3986 section 5.4 examples.
        for ref, want in [('g', 'http://a/b/c/g'), ('../../g', 'http://a/g'), ('?y', 'http://a/b/c/d;p?y'),
                          ('#s', 'http://a/b/c/d;p?q#s'), ('g/./h/../i', 'http://a/b/c/g/i'), ('//g', 'http://g'),
                          ('/./g', 'http://a/g'), ('', 'http://a/b/c/d;p?q')]:
            self.assertEqual(r('http://a/b/c/d;p?q', ref), want, ref)
        # The forms TerminusDB's writer emits (urllib.parse.urljoin leaves these relative).
        self.assertEqual(r('terminusdb:///data/', 'Person/Ada'), D + 'Person/Ada')
        self.assertEqual(r('terminusdb:///data/', '../schema#Status/active'), S + 'Status/active')
        self.assertEqual(r('terminusdb:///schema#', 'schema#Person'), S + 'Person')
        self.assertEqual(r('terminusdb:///schema#', 'data/'), D)

    def test_real_export_resolves_every_relative_iri(self):
        for name in ('schema.ttl', f'instance-{HEAD}.ttl'):
            terms = [t for tr in tdb.parse_turtle((LIVE / name).read_text()) for t in tr if t[0] == 'iri']
            self.assertTrue(terms)
            self.assertTrue(all(':' in t[1].split('/')[0] for t in terms), name)
            self.assertFalse([t for t in terms if t[1].startswith(('Person', 'schema#', '../'))], name)
        inst = tdb.parse_turtle(live_instance().read_text())
        subjects = {s[1] for s, _, _ in inst}
        self.assertIn(ADA, subjects)
        self.assertIn('terminusdb:///json/JSON/SHA1/6805b33bf51197e9e3ddee6e2e37da02ba62c3d8', subjects)
        self.assertIn(tdb.iri(S + 'Status/active'), [o for _, _, o in inst])

    def test_ntriples_round_trip(self):
        for text in (fixture('instance.ttl'), live_instance().read_text()):
            t = tdb.parse_turtle(text)
            self.assertEqual(sorted(tdb.parse_turtle(tdb.to_ntriples(t))), sorted(t))


class SchemaToShapes(unittest.TestCase):
    def check(self, classes, enums):
        self.assertEqual(set(classes), {S + c for c in ('Entity', 'Person', 'Team', 'Contact')})
        self.assertEqual(enums, {S + 'Status': [S + 'Status/active', S + 'Status/retired']})
        p = classes[S + 'Person']['all_props']
        self.assertEqual(p[S + 'name'], (1, 1, {'datatype': tdb.XSD + 'string'}))       # inherited, mandatory
        self.assertEqual(p[S + 'age'], (0, 1, {'datatype': tdb.XSD + 'decimal'}))       # Optional
        self.assertEqual(p[S + 'friends'], (0, None, {'class': S + 'Person'}))          # Set
        self.assertEqual(p[S + 'emails'][:2], (1, 2))                                   # Set/Cardinality 1..2
        self.assertEqual(p[S + 'status'][2], {'in': enums[S + 'Status']})                # Enum -> sh:in
        self.assertEqual(p[S + 'homepage'][2], {'datatype': tdb.XDD + 'url'})            # xdd: is a datatype
        self.assertEqual(p[S + 'profile'][2], {'class': tdb.SYS + 'JSON'})               # JSON is a node
        self.assertEqual(classes[S + 'Contact']['props'][S + 'none'][2], {'in': [tdb.RDF + 'nil']})  # Unit
        self.assertEqual(classes[S + 'Entity']['subclasses'], [S + 'Person'])
        person = classes[S + 'Person']
        self.assertEqual(person['parents'], [S + 'Entity'])
        self.assertEqual(person['key'], ('Lexical', [S + 'name']))
        self.assertTrue(classes[S + 'Entity']['abstract'])
        self.assertTrue(classes[S + 'Contact']['union'])

    def test_format_derived_schema(self):
        classes, enums, notes = tdb.translate_schema(tdb.parse_turtle(fixture('schema.ttl')))
        self.check(classes, enums)
        self.assertTrue(any('sys:JSON' in n for n in notes))

    def test_real_schema_export(self):
        classes, enums, notes = tdb.translate_schema(tdb.parse_turtle((LIVE / 'schema.ttl').read_text()))
        self.check(classes, enums)
        self.assertEqual(classes[S + 'Team']['all_props'][S + 'members'][:2], (1, 5))
        self.assertTrue(any('Array' in n for n in notes))

    def test_parent_class_range_accepts_subclasses(self):
        classes, enums, _ = tdb.translate_schema(tdb.parse_turtle((LIVE / 'schema.ttl').read_text()))
        shapes = tdb.write_shapes(classes, enums)
        self.assertIn(f'sh:or ( [ sh:class <{S}Entity> ] [ sh:class <{S}Person> ] )', shapes)
        self.assertIn(f'<{S}Person> rdfs:subClassOf <{S}Entity> .', shapes)
        self.assertIn('sh:xone', shapes)

    def test_unknown_range_checks_cardinality_only(self):
        schema = tdb.parse_turtle(f'@prefix sys: <{tdb.SYS}> . <{S}A> a sys:Class ; <{S}p> <http://x/Unknown> .')
        classes, _, notes = tdb.translate_schema(schema)
        self.assertEqual(classes[S + 'A']['props'][S + 'p'], (1, 1, {}))
        self.assertTrue(any('only its cardinality' in n for n in notes))

    def test_numbers_get_the_declared_datatype_back(self):
        classes, _, _ = tdb.translate_schema(tdb.parse_turtle((LIVE / 'schema.ttl').read_text()))
        fixed, count = tdb.retype_numbers(tdb.parse_turtle(live_instance().read_text()), classes)
        self.assertEqual(count, 1)                       # age 36 -> xsd:decimal; Array ints already integer
        self.assertIn((tdb.iri(ADA), tdb.iri(S + 'age'), tdb.lit('36', tdb.XSD + 'decimal')), fixed)
        self.assertIn(tdb.lit('2', tdb.XSD + 'integer'), [o for _, _, o in fixed])   # JSON contents untouched

    def test_lexical_iri_reproduces_terminusdb_ids(self):
        subjects = {s[1] for s, _, _ in tdb.parse_turtle(live_instance().read_text())}
        self.assertIn(tdb.lexical_iri(D, 'Person', ['Ada Lovelace']), subjects)
        self.assertIn(tdb.lexical_iri(D, 'Team', ['Analytical Engines']), subjects)
        self.assertEqual(tdb.lexical_iri(D, 'Pair', ['a+b', 'c/d']), D + 'Pair/a%2Bb+c%2Fd')

    def test_rebase(self):
        r = tdb.rebaser([(D, 'https://ex.org/id/'), (S, 'https://ex.org/ns#')])
        out = r(tdb.parse_turtle(fixture('instance.ttl')))
        self.assertFalse(any(t[0] == 'iri' and t[1].startswith(('terminusdb:///data', 'terminusdb:///schema'))
                             for tr in out for t in tr))


@unittest.skipUnless(QUIPU, 'quipu binary not on PATH')
class QuipuValidates(unittest.TestCase):
    def test_real_export_conforms(self):
        r = validate(LIVE / 'schema.ttl', live_instance())
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_every_real_commit_conforms(self):
        for commit in LOG:
            r = validate(LIVE / 'schema.ttl', live_instance(commit['identifier']))
            self.assertEqual(r.returncode, 0, commit['message'] + r.stdout + r.stderr)

    def test_format_derived_export_conforms(self):
        r = validate(FIX / 'schema.ttl', FIX / 'instance.ttl')
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_violations_are_caught(self):
        r = validate(FIX / 'schema.ttl', FIX / 'instance-invalid.ttl')
        self.assertNotEqual(r.returncode, 0)
        for needle in ('MinCount(1)', 'MaxCount(2)', 'xsd:decimal', 'In constraint', 'OR not satisfied'):
            self.assertIn(needle, r.stdout)

    def test_cli_load_is_one_ingest_sourced_from_the_commit(self):
        work = tempfile.mkdtemp()
        run_main(['--schema-ttl', str(LIVE / 'schema.ttl'), '--instance-ttl', str(live_instance()),
                  '--db', 'admin/people', '--commit', HEAD, '--commit-time', str(LOG[0]['timestamp']),
                  '--out', f'{work}/out', '--quipu-db', f'{work}/q.db', '--quipu-bin', QUIPU,
                  '--graph', 'urn:terminusdb:admin:people'])
        changes = json.loads(subprocess.run([QUIPU, 'changes', '--db', f'{work}/q.db'],
                                            capture_output=True, text=True, check=True).stdout)['records']
        db = sqlite3.connect(f'file:{work}/q.db?mode=ro', uri=True)
        try:
            transactions = db.execute('SELECT id, source, timestamp FROM transactions ORDER BY id').fetchall()
            loaded = [(tx, json.loads(payload)) for tx, payload in db.execute(
                "SELECT tx_id, payload FROM events WHERE type = 'shapes.loaded' ORDER BY offset")]
        finally:
            db.close()
        import_tx = one_import_after_its_shape_settlement(
            changes, transactions, loaded, f'terminusdb:admin/people/local/commit/{HEAD}', 'terminusdb')
        # transaction time and valid time are both the commit time
        commit_time = tdb.iso(LOG[0]['timestamp'])
        self.assertEqual(dict((t[0], t[2]) for t in transactions)[import_tx], commit_time)
        self.assertEqual({r['valid_from'] for r in changes}, {commit_time})
        expected = len(tdb.parse_turtle(live_instance().read_text()))
        # every instance fact, plus quipu ingest's own completion marker on the graph
        self.assertEqual(sum(r['entity'].startswith('terminusdb:///') for r in changes), expected)
        self.assertIn('urn:quipu:ingest:complete', {r['attribute'] for r in changes})


class ImportTransactionIdentity(unittest.TestCase):
    """The checker the CLI load test relies on, refused on each wrong shape."""
    SOURCE = 'terminusdb:admin/people/local/commit/c'

    def good(self):
        changes = [{'source': self.SOURCE, 'tx': 2, 'attribute': 'urn:x:name'},
                   {'source': self.SOURCE, 'tx': 2, 'attribute': 'urn:quipu:ingest:complete'}]
        transactions = [(1, 'quipu:shape-mapping', 't1'), (2, self.SOURCE, 't2')]
        loaded = [(1, {'tx': 1, 'name': 'terminusdb', 'registry': 'shapes'})]
        return changes, transactions, loaded

    def check(self, changes, transactions, loaded):
        return one_import_after_its_shape_settlement(changes, transactions, loaded, self.SOURCE, 'terminusdb')

    def test_accepts_the_settlement_then_the_import(self):
        self.assertEqual(self.check(*self.good()), 2)

    def test_refuses_a_fact_in_the_shape_settlement(self):
        changes, transactions, loaded = self.good()
        changes.append({'source': 'quipu:shape-mapping', 'tx': 1, 'attribute': 'urn:x:name'})
        self.assertRaises(AssertionError, self.check, changes, transactions, loaded)

    def test_refuses_an_import_split_over_two_transactions(self):
        changes, transactions, loaded = self.good()
        changes.append({'source': self.SOURCE, 'tx': 3, 'attribute': 'urn:x:name'})
        transactions.append((3, self.SOURCE, 't3'))
        self.assertRaises(AssertionError, self.check, changes, transactions, loaded)

    def test_refuses_a_receipt_naming_another_transaction(self):
        changes, transactions, _ = self.good()
        self.assertRaises(AssertionError, self.check, changes, transactions,
                          [(1, {'tx': 2, 'name': 'terminusdb', 'registry': 'shapes'})])

    def test_refuses_an_unexplained_extra_transaction(self):
        changes, transactions, loaded = self.good()
        transactions.insert(1, (2, 'other', 't'))
        transactions[2] = (3, self.SOURCE, 't3')
        for r in changes:
            r['tx'] = 3
        self.assertRaises(AssertionError, self.check, changes, transactions, loaded)

    def test_refuses_an_import_without_its_completion_marker(self):
        changes, transactions, loaded = self.good()
        changes.pop()
        self.assertRaises(AssertionError, self.check, changes, transactions, loaded)


# ---- an HTTP double serving the live fixtures on /api/log and /api/triples ----

class FakeTerminusDB(http.server.BaseHTTPRequestHandler):
    seen = []

    def do_GET(self):
        FakeTerminusDB.seen.append((self.path, self.headers.get('Authorization')))
        path = self.path.split('?')[0]
        if path == '/api/log/admin/people/local/branch/main':
            count = int(self.path.split('count=')[1]) if 'count=' in self.path else None
            return self.reply(json.dumps(LOG[:count]), 'application/json')
        prefix = '/api/triples/admin/people/local/commit/'
        if path.startswith(prefix):
            commit, graph = path[len(prefix):].split('/')
            body = (LIVE / 'schema.ttl').read_text() if graph == 'schema' else live_instance(commit).read_text()
            if 'text/turtle' in self.headers.get('Accept', ''):
                return self.reply(body, 'text/turtle')
            return self.reply(json.dumps(body), 'application/json')   # what the route does without Accept
        self.send_error(404)

    def reply(self, body, ctype):
        data = body.encode()
        self.send_response(200)
        self.send_header('Content-Type', ctype)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *a):
        pass


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


class QuipuServerMixin:
    def quipu_server(self):
        """Start a throwaway quipu-server; returns (url, workdir), stopped at cleanup."""
        work = tempfile.mkdtemp()
        port = free_port()
        proc = subprocess.Popen([QUIPU_SERVER, '--db', f'{work}/q.db', '--bind', f'127.0.0.1:{port}'],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(lambda: (proc.terminate(), proc.wait(timeout=10)))
        url = f'http://127.0.0.1:{port}'
        for _ in range(60):
            try:
                urllib.request.urlopen(url + '/health', timeout=1)
                return url, work
            except OSError:
                time.sleep(0.25)
        self.fail('quipu-server did not start')

    def select(self, quipu, query, **extra):
        body = json.dumps({'query': query, **extra}).encode()
        req = urllib.request.Request(quipu + '/query', data=body, headers={'Content-Type': 'application/json'})
        rows = json.loads(urllib.request.urlopen(req).read())['rows']
        return sorted(v['value'] if isinstance(v, dict) else v for r in rows for v in r.values())


class ServerMode(QuipuServerMixin, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.httpd = http.server.ThreadingHTTPServer(('127.0.0.1', 0), FakeTerminusDB)
        threading.Thread(target=cls.httpd.serve_forever, daemon=True).start()
        cls.url = f'http://127.0.0.1:{cls.httpd.server_port}'

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()

    def test_pins_both_graphs_to_the_head_commit(self):
        FakeTerminusDB.seen.clear()
        out = tempfile.mkdtemp()
        os.environ.update(TERMINUSDB_USER='u', TERMINUSDB_PASSWORD='p')
        try:
            run_main(['--server', self.url, '--db', 'admin/people', '--out', out])
        finally:
            os.environ.pop('TERMINUSDB_USER'), os.environ.pop('TERMINUSDB_PASSWORD')
        report = json.loads(Path(out, 'import.json').read_text())
        self.assertEqual(report['source'], f'terminusdb:admin/people/local/commit/{HEAD}')
        self.assertEqual(report['commit_time'], tdb.iso(LOG[0]['timestamp']))
        paths = [p for p, _ in FakeTerminusDB.seen]
        self.assertIn(f'/api/triples/admin/people/local/commit/{HEAD}/schema', paths)
        self.assertIn(f'/api/triples/admin/people/local/commit/{HEAD}/instance', paths)
        self.assertTrue(all(a and a.startswith('Basic ') for _, a in FakeTerminusDB.seen))

    @unittest.skipUnless(QUIPU_SERVER, 'quipu-server binary not on PATH')
    def test_rest_load_is_one_episode_sourced_from_the_commit(self):
        quipu, work = self.quipu_server()
        run_main(['--server', self.url, '--db', 'admin/people', '--out', f'{work}/load', '--quipu-url', quipu])
        knot = json.loads(Path(work, 'load', 'import.json').read_text())['quipu']['knot']
        self.assertEqual(knot['count'], len(tdb.parse_turtle(live_instance().read_text())))
        names = self.select(quipu, f'SELECT ?n WHERE {{ ?s <{S}name> ?n }}')
        self.assertEqual(names, ['Ada Lovelace', 'Analytical Engines', 'Grace Hopper'])
        self.assertEqual(self.select(quipu, f'SELECT ?l WHERE {{ ?t <{S}lead> ?l }}'), [ADA])
        # The loaded shapes now gate writes: invalid data is refused and nothing is written.
        with self.assertRaises(SystemExit) as refused:
            run_main(['--schema-ttl', str(FIX / 'schema.ttl'), '--instance-ttl', str(FIX / 'instance-invalid.ttl'),
                      '--out', tempfile.mkdtemp(), '--quipu-url', quipu])
        self.assertIn('refused by the shapes', str(refused.exception))
        self.assertEqual(self.select(quipu, f'SELECT ?p WHERE {{ <{D}Person/Nobody> ?p ?o }}'), [])

    @unittest.skipUnless(QUIPU_SERVER, 'quipu-server binary not on PATH')
    def test_log_replay_orders_sub_second_commits_and_skips_schema_only(self):
        quipu, work = self.quipu_server()
        run_main(['--server', self.url, '--db', 'admin/people', '--out', f'{work}/replay',
                  '--quipu-url', quipu, '--replay-log'])
        replay = json.loads(Path(work, 'replay', 'import.json').read_text())['replay']
        self.assertEqual([r['commit'] for r in replay], [c['identifier'] for c in reversed(LOG)])
        self.assertEqual(replay[0].get('skipped'), 'empty instance graph')        # the schema commit
        first = int(float(LOG[-2]['timestamp']))                                    # "Add Ada"
        self.assertEqual([r['time'] for r in replay[1:]], [tdb.iso(first + i) for i in range(3)])
        self.assertTrue(all(r.get('time_adjusted') for r in replay[2:]))           # all four share a second
        age = f'SELECT ?a WHERE {{ <{ADA}> <{S}age> ?a }}'
        self.assertEqual(self.select(quipu, age, valid_at=tdb.iso(first)), ['35'])
        self.assertEqual(self.select(quipu, age, valid_at=tdb.iso(first + 1)), ['36'])
        self.assertEqual(self.select(quipu, age, tx=replay[1]['quipu']['tx_id']), ['35'])
        grace = f'SELECT ?n WHERE {{ <{GRACE}> <{S}name> ?n }}'
        self.assertEqual(self.select(quipu, grace, valid_at=tdb.iso(first + 1)), [])
        self.assertEqual(self.select(quipu, grace, valid_at=tdb.iso(first + 2)), ['Grace Hopper'])


# ---- optional: a real TerminusDB server -----------------------------------------

def seed_live(url, user, password, db):
    """Create <db> on a real server from live/schema.json with the fixture's three data commits."""
    auth = 'Basic ' + base64.b64encode(f'{user}:{password}'.encode()).decode()

    def call(method, path, body):
        req = urllib.request.Request(url + path, method=method, data=json.dumps(body).encode(),
                                     headers={'Authorization': auth, 'Content-Type': 'application/json'})
        urllib.request.urlopen(req).read()
    call('POST', f'/api/db/admin/{db}', {'label': db, 'comment': 'quipu terminusdb-import test', 'schema': True})
    call('POST', f'/api/document/admin/{db}?graph_type=schema&author=admin&message=schema&full_replace=true',
         json.loads((LIVE / 'schema.json').read_text()))
    ada = {'@type': 'Person', 'name': 'Ada Lovelace', 'age': 35, 'status': 'active', 'emails': ['ada@example.org'],
           'nicknames': ['The Enchantress of Numbers'], 'scores': [3, 1, 4],
           'homepage': 'https://example.org/ada', 'profile': {'some': {'random': 'stuff', 'that': 2.0}}}
    call('POST', f'/api/document/admin/{db}?author=admin&message=Add+Ada', [ada])
    call('PUT', f'/api/document/admin/{db}?author=admin&message=Ada+turns+36', [{**ada, 'age': 36}])
    grace = {'@type': 'Person', 'name': 'Grace Hopper', 'status': 'retired',
             'emails': ['grace@example.org', 'amazing.grace@example.org'], 'nicknames': [], 'scores': [],
             'friends': ['Person/Ada%20Lovelace'], 'contact': {'@type': 'Contact', 'none': []}}
    team = {'@type': 'Team', 'name': 'Analytical Engines', 'lead': 'Person/Ada%20Lovelace',
            'members': ['Person/Ada%20Lovelace', 'Person/Grace%20Hopper']}
    call('POST', f'/api/document/admin/{db}?author=admin&message=Add+Grace+and+team', [grace, team])


@unittest.skipUnless(os.environ.get('TERMINUSDB_LIVE_URL') and QUIPU_SERVER,
                     'set TERMINUSDB_LIVE_URL (and quipu-server) to run against a real TerminusDB')
class LiveTerminusDB(QuipuServerMixin, unittest.TestCase):
    def test_seed_import_and_replay_against_a_real_server(self):
        url = os.environ['TERMINUSDB_LIVE_URL'].rstrip('/')
        db = 'quipu_import_' + uuid.uuid4().hex[:8]
        seed_live(url, os.environ.get('TERMINUSDB_USER', 'admin'), os.environ.get('TERMINUSDB_PASSWORD', ''), db)
        quipu, work = self.quipu_server()
        run_main(['--server', url, '--db', f'admin/{db}', '--out', f'{work}/load', '--quipu-url', quipu])
        report = json.loads(Path(work, 'load', 'import.json').read_text())
        self.assertGreater(report['quipu']['knot']['count'], 30)
        self.assertEqual(self.select(quipu, f'SELECT ?l WHERE {{ ?t <{S}lead> ?l }}'), [ADA])
        quipu2, _ = self.quipu_server()
        run_main(['--server', url, '--db', f'admin/{db}', '--out', f'{work}/replay', '--quipu-url', quipu2,
                  '--replay-log'])
        replay = json.loads(Path(work, 'replay', 'import.json').read_text())['replay']
        self.assertEqual(len(replay), 4)


if __name__ == '__main__':
    unittest.main()
