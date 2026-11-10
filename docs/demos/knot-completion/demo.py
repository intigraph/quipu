#!/usr/bin/env python3
"""Compare real ordinary-write completion lookups on two private servers."""
import contextlib
import json
import pathlib
import socket
import subprocess
import sys
import tempfile
import time
from urllib import error, request

WRITE_ID = "a" * 64
INPUT = {
    "turtle": '<urn:demo:completion> <http://www.w3.org/2000/01/rdf-schema#label> "control" .',
    "snapshot": "completion-demo",
    "replace_snapshot": True,
}


def call(url, path, body=None, write_id=None):
    headers = {"Content-Type": "application/json"}
    if write_id:
        headers["X-Quipu-Write-Id"] = write_id
    req = request.Request(
        url + path,
        data=None if body is None else json.dumps(body).encode(),
        headers=headers,
    )
    try:
        response = request.urlopen(req, timeout=5)
    except error.HTTPError as response:
        return response.code, json.loads(response.read() or b"null")
    with response:
        return response.status, json.loads(response.read() or b"null")


@contextlib.contextmanager
def server(binary):
    binary = str(pathlib.Path(binary).resolve())
    with tempfile.TemporaryDirectory(prefix="knot-completion-demo-") as root:
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        url = f"http://127.0.0.1:{port}"
        with open(pathlib.Path(root) / "server.log", "w") as log:
            child = subprocess.Popen(
                [binary, "--db", str(pathlib.Path(root) / "store.db"),
                 "--bind", f"127.0.0.1:{port}"],
                cwd=root, stdout=log, stderr=log,
            )
            try:
                for _ in range(100):
                    if child.poll() is not None:
                        raise RuntimeError("private server exited during startup")
                    try:
                        if call(url, "/health")[0] == 200:
                            break
                    except OSError:
                        time.sleep(0.05)
                else:
                    raise RuntimeError("private server did not become healthy")
                yield url
            finally:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


def observe(label, binary):
    with server(binary) as url:
        version = call(url, "/version")[1]
        write_status, write = call(url, "/knot", INPUT, WRITE_ID)
        status, receipt = call(url, "/knot/status", {"write_id": WRITE_ID, "request": INPUT})
        duplicate_status, _ = call(url, "/knot", INPUT, WRITE_ID)
        unknown_status, unknown = call(url, "/knot/status", {"write_id": "b" * 64, "request": INPUT})
        result = {
            "revision": version["git_sha"], "dirty": version["git_dirty"],
            "write_http": write_status, "native_tx_id": write.get("tx_id"),
            "status_http": status, "completion": receipt,
            "duplicate_http": duplicate_status,
            "never_seen_http": unknown_status, "never_seen": unknown,
        }
        print(label, json.dumps(result, sort_keys=True), flush=True)
        assert write_status == 200 and isinstance(write.get("tx_id"), int)
        if label == "BEFORE":
            assert status == 404 and duplicate_status == 200
        else:
            assert status == 200 and receipt["contract"] == "quipu.knot-completion/v1"
            assert receipt["state"] == "finished" and receipt["receipt"]["tx_id"] == write["tx_id"]
            assert duplicate_status == 400
            assert unknown_status == 200 and unknown["state"] == "unknown"


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: demo.py BEFORE_SERVER AFTER_SERVER")
    observe("BEFORE", sys.argv[1])
    observe("AFTER", sys.argv[2])
    print("Private fixtures only; no installation or live timeout recovery claimed.")
