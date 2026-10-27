#!/usr/bin/env python3
"""Tests for scripts/terminusdb-import.py.

Fixtures (test-fixtures/terminusdb/) are built from TerminusDB's documented
formats, not captured from a live server: the schema-graph triples follow
``json_schema_triple`` and its tests in src/core/document/json.pl, the
``/api/triples`` route is src/server/routes.pl:376-438, and the ``/api/log``
response shape is the "Query the commit log" example in
https://terminusdb.com/llms-full.txt (all at TerminusDB 0b5549ac). A stdlib
HTTP double serves those two routes so the ``--server`` and ``--replay-log``
paths run end to end.

When ``quipu`` / ``quipu-server`` are on PATH (or QUIPU_BIN / QUIPU_SERVER_BIN
point at them) the generated shapes are validated by the real engine and the
REST load + log replay run against a throwaway server; otherwise those tests skip.
"""
import contextlib
import http.server
import io
import importlib.util
import json
import os
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIX = HERE.parent / 'test-fixtures' / 'terminusdb'
spec = importlib.util.spec_from_file_location('tdb', HERE / 'terminusdb-import.py')
tdb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tdb)
S = 'terminusdb:///schema#'
D = 'terminusdb:///data/'
QUIPU = os.environ.get('QUIPU_BIN') or shutil.which('quipu')
QUIPU_SERVER = os.environ.get('QUIPU_SERVER_BIN') or shutil.which('quipu-server')


def run_main(argv):
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        tdb.main(argv)


def fixture(name):
    return (FIX / name).read_text(encoding='utf-8')


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

    def test_ntriples_round_trip(self):
        t = tdb.parse_turtle(fixture('instance.ttl'))
        self.assertEqual(sorted(tdb.parse_turtle(tdb.to_ntriples(t))), sorted(t))


class SchemaToShapes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.classes, cls.enums, cls.notes = tdb.translate_schema(tdb.parse_turtle(fixture('schema.ttl')))

    def test_every_class_becomes_a_shape_and_enums_are_not_classes(self):
        self.assertEqual(set(self.classes), {S + c for c in ('Entity', 'Person', 'Team', 'Contact')})
        self.assertEqual(self.enums, {S + 'Status': [S + 'Status/active', S + 'Status/retired']})

    def test_cardinality_families(self):
        p = self.classes[S + 'Person']['all_props']
        self.assertEqual(p[S + 'name'], (1, 1, {'datatype': tdb.XSD + 'string'}))      # inherited, mandatory
        self.assertEqual(p[S + 'age'], (0, 1, {'datatype': tdb.XSD + 'decimal'}))      # Optional
        self.assertEqual(p[S + 'friends'], (0, None, {'class': S + 'Person'}))         # Set
        self.assertEqual(p[S + 'emails'], (1, 2, {'datatype': tdb.XSD + 'string'}))    # Cardinality 1..2
        self.assertEqual(p[S + 'status'][2], {'in': self.enums[S + 'Status']})          # Enum -> sh:in
        self.assertTrue(any('List' in n for n in self.notes))

    def test_inherits_key_and_union(self):
        person = self.classes[S + 'Person']
        self.assertEqual(person['parents'], [S + 'Entity'])
        self.assertEqual(person['key'], ('Lexical', [S + 'name']))
        self.assertTrue(self.classes[S + 'Entity']['abstract'])
        self.assertTrue(self.classes[S + 'Contact']['union'])
        shapes = tdb.write_shapes(self.classes, self.enums)
        self.assertIn(f'<{S}Person> rdfs:subClassOf <{S}Entity> .', shapes)
        self.assertIn('sh:xone', shapes)
        self.assertIn(f'<{S}Person/Shape> a sh:NodeShape', shapes)

    def test_lexical_iri_reproduces_terminusdb_ids(self):
        subjects = {s[1] for s, _, _ in tdb.parse_turtle(fixture('instance.ttl'))}
        self.assertIn(tdb.lexical_iri(D, 'Person', ['Ada Lovelace']), subjects)
        self.assertIn(tdb.lexical_iri(D, 'Team', ['Analytical Engines']), subjects)
        self.assertEqual(tdb.lexical_iri(D, 'Pair', ['a+b', 'c/d']), D + 'Pair/a%2Bb+c%2Fd')

    def test_rebase(self):
        r = tdb.rebaser([(D, 'https://ex.org/id/'), (S, 'https://ex.org/ns#')])
        out = r(tdb.parse_turtle(fixture('instance.ttl')))
        self.assertFalse(any(t[0] == 'iri' and t[1].startswith('terminusdb:') for tr in out for t in tr))


@unittest.skipUnless(QUIPU, 'quipu binary not on PATH')
class QuipuValidates(unittest.TestCase):
    def run_import(self, instance):
        out = tempfile.mkdtemp()
        run_main(['--schema-ttl', str(FIX / 'schema.ttl'), '--instance-ttl', str(FIX / instance), '--out', out])
        return subprocess.run([QUIPU, 'validate', '--shapes', f'{out}/shapes.ttl', '--data', f'{out}/instance.nt'],
                              capture_output=True, text=True)

    def test_valid_export_conforms(self):
        r = self.run_import('instance.ttl')
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_cli_load_is_one_ingest_sourced_from_the_commit(self):
        work = tempfile.mkdtemp()
        run_main(['--schema-ttl', str(FIX / 'schema.ttl'), '--instance-ttl', str(FIX / 'instance.ttl'),
                  '--db', 'admin/people', '--commit', 'abc123', '--commit-time', '1714400000',
                  '--out', f'{work}/out', '--quipu-db', f'{work}/q.db', '--quipu-bin', QUIPU,
                  '--graph', 'urn:terminusdb:admin:people'])
        changes = json.loads(subprocess.run([QUIPU, 'changes', '--db', f'{work}/q.db'],
                                            capture_output=True, text=True, check=True).stdout)['records']
        self.assertEqual({(r['source'], r['tx']) for r in changes},
                         {('terminusdb:admin/people/local/commit/abc123', 1)})
        # 20 instance facts plus quipu ingest's own completion marker on the graph.
        self.assertEqual(sum(r['entity'].startswith('terminusdb:///') for r in changes), 20)
        self.assertIn('urn:quipu:ingest:complete', {r['attribute'] for r in changes})

    def test_terminusdb_violations_are_caught(self):
        r = self.run_import('instance-invalid.ttl')
        self.assertNotEqual(r.returncode, 0)
        for needle in ('MinCount(1)', 'MaxCount(2)', 'xsd:decimal', 'In constraint'):
            self.assertIn(needle, r.stdout)


# ---- an HTTP double for TerminusDB's /api/log and /api/triples ----------------

COMMITS = [  # newest first, as /api/log returns them
    {'@id': 'ValidCommit/c2', '@type': 'ValidCommit', 'identifier': 'c2', 'author': 'admin',
     'message': 'Ada turns 36; add Grace', 'parent': 'ValidCommit/c1', 'timestamp': 1714400100.0},
    {'@id': 'ValidCommit/c1', '@type': 'InitialCommit', 'identifier': 'c1', 'author': 'admin',
     'message': 'Add Ada', 'timestamp': 1714400000.0},
]
C1_INSTANCE = f'''@prefix schema: <{S}> .
<{D}Person/Ada%20Lovelace> a schema:Person ; schema:name "Ada Lovelace" ; schema:age "35"^^<http://www.w3.org/2001/XMLSchema#decimal> ;
  schema:status <{S}Status/active> ; schema:emails "ada@example.org" ;
  schema:nicknames <http://www.w3.org/1999/02/22-rdf-syntax-ns#nil> .
'''


class FakeTerminusDB(http.server.BaseHTTPRequestHandler):
    seen = []

    def do_GET(self):
        FakeTerminusDB.seen.append((self.path, self.headers.get('Authorization')))
        path = self.path.split('?')[0]
        if path == '/api/log/admin/people/local/branch/main':
            count = int(self.path.split('count=')[1]) if 'count=' in self.path else None
            return self.reply(json.dumps(COMMITS[:count]), 'application/json')
        graphs = {'schema': fixture('schema.ttl'), 'c2/instance': fixture('instance.ttl'), 'c1/instance': C1_INSTANCE}
        for suffix, body in graphs.items():
            if path.endswith(suffix) and path.startswith('/api/triples/admin/people/local/commit/'):
                if 'text/turtle' in self.headers.get('Accept', ''):
                    return self.reply(body, 'text/turtle')
                return self.reply(json.dumps(body), 'application/json')
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


class ServerMode(unittest.TestCase):
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
        self.assertEqual(report['source'], 'terminusdb:admin/people/local/commit/c2')
        self.assertEqual(report['commit_time'], '2024-04-29T14:15:00Z')
        paths = [p for p, _ in FakeTerminusDB.seen]
        self.assertIn('/api/triples/admin/people/local/commit/c2/schema', paths)
        self.assertTrue(all(a and a.startswith('Basic ') for _, a in FakeTerminusDB.seen))

    def quipu_server(self):
        """Start a throwaway quipu-server; returns its URL (stopped at test cleanup)."""
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

    @unittest.skipUnless(QUIPU_SERVER, 'quipu-server binary not on PATH')
    def test_rest_load_is_one_episode_sourced_from_the_commit(self):
        quipu, work = self.quipu_server()
        run_main(['--server', self.url, '--db', 'admin/people', '--out', f'{work}/load', '--quipu-url', quipu])
        knot = json.loads(Path(work, 'load', 'import.json').read_text())['quipu']['knot']
        self.assertEqual(knot['count'], 20)
        self.assertEqual(knot['valid_from'], '2024-04-29T14:15:00Z')
        names = self.select(quipu, f'SELECT ?n WHERE {{ ?s <{S}name> ?n }}')
        self.assertEqual(names, ['Ada Lovelace', 'Analytical Engines', 'Grace Hopper'])
        # The loaded shapes now gate writes: TerminusDB-invalid data is refused, nothing written.
        bad = tempfile.mkdtemp()
        with self.assertRaises(SystemExit) as refused:
            run_main(['--schema-ttl', str(FIX / 'schema.ttl'), '--instance-ttl', str(FIX / 'instance-invalid.ttl'),
                      '--out', bad, '--quipu-url', quipu])
        self.assertIn('refused by the shapes', str(refused.exception))
        self.assertEqual(self.select(quipu, f'SELECT ?s WHERE {{ <{D}Person/Nobody> ?p ?o }}'), [])

    @unittest.skipUnless(QUIPU_SERVER, 'quipu-server binary not on PATH')
    def test_log_replay_gives_commit_time_as_tx_and_valid_time(self):
        quipu, work = self.quipu_server()
        run_main(['--server', self.url, '--db', 'admin/people', '--out', f'{work}/replay',
                  '--quipu-url', quipu, '--replay-log'])
        replay = json.loads(Path(work, 'replay', 'import.json').read_text())['replay']
        self.assertEqual([r['commit'] for r in replay], ['c1', 'c2'])
        self.assertTrue(replay[1]['quipu']['replaced'])
        age = f'SELECT ?a WHERE {{ <{D}Person/Ada%20Lovelace> <{S}age> ?a }}'
        self.assertEqual(self.select(quipu, age, valid_at='2024-04-29T14:14:00Z'), ['35'])  # between c1 and c2
        self.assertEqual(self.select(quipu, age, valid_at='2024-04-29T14:16:00Z'), ['36'])  # after c2
        self.assertEqual(self.select(quipu, age, tx=replay[0]['quipu']['tx_id']), ['35'])   # as of c1's tx

if __name__ == '__main__':
    unittest.main()
