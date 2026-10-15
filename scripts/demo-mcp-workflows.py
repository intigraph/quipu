#!/usr/bin/env python3
"""Record the same actual MCP call on two private-store server binaries."""
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import tempfile


def demonstrate(binary, label):
    print(label, flush=True)
    print(subprocess.check_output([binary, '--version'], text=True).strip(), flush=True)
    with tempfile.TemporaryDirectory(prefix='mcp-parity-demo-') as directory:
        root = Path(directory)
        env = {k: os.environ[k] for k in ('PATH', 'LANG', 'LD_LIBRARY_PATH', 'ORT_DYLIB_PATH') if k in os.environ}
        env['HOME'] = str(root)
        process = subprocess.Popen([binary, '--mcp-stdio', '--db', str(root / 'demo.db')],
                                   cwd=root, env=env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        ident = 0

        def send(method, params):
            nonlocal ident
            ident += 1
            process.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': ident, 'method': method, 'params': params}) + '\n')
            process.stdin.flush()
            assert selector.select(15), 'MCP response timed out'
            return json.loads(process.stdout.readline())

        def call(name, arguments):
            return send('tools/call', {'name': name, 'arguments': arguments})

        try:
            send('initialize', {'protocolVersion': '2025-03-26', 'capabilities': {},
                               'clientInfo': {'name': 'portable-demo', 'version': '1'}})
            process.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n')
            process.stdin.flush()
            written = call('quipu_knot', {'turtle': '<urn:demo:item> <urn:demo:value> "present" .'})
            assert written['result'].get('isError') is not True, written
            control = call('quipu_query', {'query': 'SELECT ?v WHERE {<urn:demo:item> <urn:demo:value> ?v}'})
            assert json.loads(control['result']['content'][0]['text'])['count'] == 1
            print('Positive control: same store query count=1', flush=True)
            print('tools/call quipu_events {"since":0,"limit":1}', flush=True)
            result = call('quipu_events', {'since': 0, 'limit': 1})
            if label == 'BEFORE':
                assert 'error' in result, result
            else:
                content = json.loads(result['result']['content'][0]['text'])
                assert result['result'].get('isError') is not True and len(content['events']) == 1, result
            print(json.dumps(result, ensure_ascii=False), flush=True)
        finally:
            selector.close()
            process.terminate()
            process.wait(timeout=10)


if __name__ == '__main__':
    demonstrate(str(Path(sys.argv[1]).resolve()), 'BEFORE')
    demonstrate(str(Path(sys.argv[2]).resolve()), 'AFTER')
