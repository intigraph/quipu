"""Use the installed native MCP command to register queries in an owned store."""

import json
import select
import subprocess


def load_queries(binary, database, queries):
    """No listener, live endpoint, or production database is involved."""
    with subprocess.Popen(
        [binary, "mcp", "--db", str(database)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True,
    ) as process:
        def rpc(number, method, params):
            process.stdin.write(json.dumps({
                "jsonrpc": "2.0", "id": number, "method": method, "params": params,
            }) + "\n")
            process.stdin.flush()
            while True:
                if not select.select([process.stdout], [], [], 30)[0]:
                    raise RuntimeError("Native query-registry response deadline")
                line = process.stdout.readline()
                if not line:
                    raise RuntimeError("Native query-registry transport closed")
                response = json.loads(line)
                if response.get("id") == number:
                    if "error" in response or response.get("result", {}).get("isError"):
                        raise RuntimeError("Native query registry refused: " + line)
                    return response["result"]

        try:
            rpc(1, "initialize", {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "embedded-artifact-builder", "version": "1"},
            })
            process.stdin.write(json.dumps({
                "jsonrpc": "2.0", "method": "notifications/initialized",
            }) + "\n")
            process.stdin.flush()
            for number, query in enumerate(queries, 2):
                rpc(number, "tools/call", {
                    "name": "quipu_queries", "arguments": {"action": "load", **query},
                })
        finally:
            process.stdin.close()
            process.wait(timeout=30)
        if process.returncode:
            raise RuntimeError("Native query-registry command failed")
