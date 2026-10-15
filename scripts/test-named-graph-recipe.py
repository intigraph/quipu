#!/usr/bin/env python3
"""Execute the book's exact MCP calls on a new, private store and after restart."""
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[1]
STEPS = [json.loads(block) for block in re.findall(
    r'```json\n(.*?)\n```',
    (REPO / 'docs/book/src/recipes/named-graph-mcp.md').read_text(), re.S)]
GRAPH = STEPS[0]['arguments']['graph']
QUERY = STEPS[5]['arguments']['query']


def session(server, root, write):
    env = {k: os.environ[k] for k in ('PATH', 'LANG', 'LD_LIBRARY_PATH', 'ORT_DYLIB_PATH') if k in os.environ}
    env['HOME'] = str(root / 'home')
    process = subprocess.Popen([str(server), '--mcp-stdio', '--db', str(root / 'tutorial.db')],
                               cwd=root, env=env, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    ident = 0

    def send(method, params, notify=False):
        nonlocal ident
        message = {'jsonrpc': '2.0', 'method': method, 'params': params}
        if not notify:
            ident += 1
            message['id'] = ident
        process.stdin.write(json.dumps(message) + '\n')
        process.stdin.flush()
        if notify:
            return None
        if not selector.select(30):
            raise AssertionError(f'{method}: MCP response timed out')
        line = process.stdout.readline()
        assert line, f'{method}: MCP closed without a response'
        response = json.loads(line)
        assert 'error' not in response, response
        return response['result']

    def call(step, error=False):
        result = send('tools/call', step)
        assert bool(result.get('isError')) == error, result
        return json.loads(result['content'][0]['text'])

    try:
        send('initialize', {'protocolVersion': '2025-03-26', 'capabilities': {},
                            'clientInfo': {'name': 'docs-only-named-graph', 'version': '1'}})
        send('notifications/initialized', {}, notify=True)
        manifest = send('tools/list', {})['tools']
        names = {t['name'] for t in manifest}
        assert {s['name'] for s in STEPS} <= names
        for name in ('quipu_graph_create', 'quipu_graph_label'):
            tool = next(t for t in manifest if t['name'] == name)
            assert tool['annotations']['readOnlyHint'] is False
        if write:
            unknown = call({'name': 'quipu_knot', 'arguments': STEPS[4]['arguments']}, error=True)
            assert 'quipu_graph_create' in str(unknown)
            assert f"quipu graph create '{GRAPH}'" in unknown.get("error", ""), unknown
            results = [call(step) for step in STEPS]
            assert results[0]['created'] is True
            assert results[0]['creation'] == {'actor': STEPS[0]['arguments']['actor'],
                                              'timestamp': STEPS[0]['arguments']['timestamp']}
            assert 'https://example.org/Job' in results[3]['classes']
            assert results[5]['count'] == 1
            assert results[5]['rows'][0]['label'] == 'Research engineer'
            row = next(r for r in results[6]['graphs'] if r['iri'] == GRAPH)
            assert row['labels']['kind'] == 'operational'
            assert row['labels']['freshness'] == 'fresh'
            assert row['creation'] == results[0]['creation']
            bad_type = call({'name': 'quipu_knot', 'arguments': {'graph': GRAPH,
                'turtle': '<https://example.org/bad> a <https://example.org/UnknownClass> .'}}, error=True)
            assert 'vocabulary' in str(bad_type).lower(), bad_type
            wrong_graph = call({'name': 'quipu_query', 'arguments': {'graph': 'urn:example:missing',
                                                                  'query': QUERY}})
            assert wrong_graph['count'] == 0, wrong_graph
            print('PASS seven exact book calls; declared creation; scoped write/query; labels; unknown graph/type controls')
        else:
            repeated = dict(STEPS[0]['arguments'], actor='urn:example:other', timestamp='2026-10-15T12:00:00Z')
            result = call({'name': 'quipu_graph_create', 'arguments': repeated})
            assert result['created'] is False
            assert result['creation']['actor'] == STEPS[0]['arguments']['actor']
            assert result['creation']['timestamp'] == STEPS[0]['arguments']['timestamp']
            assert call(STEPS[5])['count'] == 1
            print('PASS restart retains graph, facts and original actor/time; repeat is idempotent')
        root_result = call({'name': 'quipu_query', 'arguments': {'query': QUERY}})
        assert root_result['count'] == 0, root_result
        print('PASS ROOT isolation paired with named-graph positive control')
    finally:
        selector.close()
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=5)
        process.stdout.close()


def cli_control(server, root):
    cli = server.with_name('quipu')
    env = {k: os.environ[k] for k in ('PATH', 'LANG', 'LD_LIBRARY_PATH', 'ORT_DYLIB_PATH') if k in os.environ}
    env['HOME'] = str(root / 'home')
    def run(*args):
        p = subprocess.run([str(cli), *args, '--db', str(root / 'cli.db')], cwd=root,
                           env=env, capture_output=True, text=True, timeout=30, check=True)
        return json.loads(p.stdout)
    first = run('graph', 'create', 'urn:example:cli', '--actor', 'urn:example:first',
                '--timestamp', '2026-10-14T14:00:00+02:00')
    assert first['created'] is True
    assert first['creation'] == {'actor': 'urn:example:first', 'timestamp': '2026-10-14T12:00:00Z'}
    repeat = run('graph', 'create', 'urn:example:cli', '--actor', 'urn:example:later')
    assert repeat['created'] is False and repeat['creation'] == first['creation']
    run('graph', 'label', 'urn:example:cli', '--label', '{"kind":"operational","freshness":"fresh"}')
    listing = run('graph', 'list', '--json')
    row = next(r for r in listing['graphs'] if r['iri'] == 'urn:example:cli')
    assert row['creation'] == first['creation']
    assert row['labels']['kind'] == 'operational'
    overlay = 'urn:example:cli-overlay'
    run('graph', 'overlay-create', '--input', json.dumps({'overlay': overlay, 'parent_branch': 'urn:example:cli'}))
    run('graph', 'overlay-write', '--input', json.dumps({'overlay': overlay, 'op': 'assert',
        'subject': 'urn:example:shadow', 'predicate': 'urn:example:p', 'object': 'fixture'}))
    composed = run('graph', 'overlay-compose', '--input', json.dumps({'overlay': overlay}))
    assert composed['count'] == 1 and composed['triples'][0]['subject'] == 'urn:example:shadow'
    print('PASS CLI creation/label/list/overlay lifecycle share the MCP handlers and normalize RFC3339 offsets')


if __name__ == '__main__':
    assert len(STEPS) == 7, 'recipe must contain the seven executable MCP examples'
    server = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix='quipu-named-graph-recipe-') as directory:
        root = Path(directory)
        (root / 'home').mkdir()
        session(server, root, True)
        session(server, root, False)
        cli_control(server, root)
