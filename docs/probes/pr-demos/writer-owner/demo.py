"""Record actual owner CLI calls against disposable local stores."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

base, head, destination = map(Path, sys.argv[1:])
started = time.monotonic()
events = []


def emit(text):
    print(text, end="", flush=True)
    events.append([round(time.monotonic() - started, 6), "o", text.replace("\n", "\r\n")])


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)

    def run(binary, label, args, success=True):
        emit(f"$ {label} {' '.join(args)}\n")
        result = subprocess.run(
            [str(binary), *args], cwd=root,
            env={"HOME": directory, "PATH": os.defpath},
            capture_output=True, text=True, check=False,
        )
        emit(result.stdout + result.stderr)
        emit(f"exit={result.returncode}\n")
        assert (result.returncode == 0) == success
        return result

    for binary, label in [(base, "base"), (head, "head")]:
        run(binary, label, ["--version"])
        run(binary, label, ["graph", "create", "urn:demo:target", "--db", label + ".db"])
    probe = ["graph", "writer-instance", "urn:demo:target", "--db"]
    run(base, "base", probe + ["base.db"], False)
    observed = run(head, "head", probe + ["head.db"])
    grant = {
        "version": 1, "id": "demo-owner", "principal": "urn:demo:curator",
        "credential_id": "demo-credential", "instance": json.loads(observed.stdout),
        "operations": ["update"], "valid_from": 0,
        "valid_until": int(time.time()) + 3600, "revision": 1, "revoked": False,
    }
    file = root / "grant.json"
    file.write_text(json.dumps(grant))
    command = ["graph", "writer-grant", "grant.json", "--db", "head.db"]
    emit("Grant fixture: one observed graph instance, Update only; no bearer.\n")
    run(head, "head", command)
    emit("Reusing revision 1 must fail.\n")
    run(head, "head", command, False)
    grant.update(revision=2, revoked=True)
    file.write_text(json.dumps(grant))
    emit("Withdraw the grant with revision 2.\n")
    run(head, "head", command)
    file.write_text("private-demo-canary-invalid-json")
    malformed = run(head, "head", command, False)
    assert "invalid writer grant JSON" in malformed.stderr
    assert "private-demo-canary" not in malformed.stderr
    emit("Malformed grant refused without echoing its contents.\n")
header = {"version": 2, "width": 110, "height": 32, "timestamp": int(time.time()), "env": {"TERM": "xterm-256color"}}
destination.write_text("\n".join(json.dumps(x) for x in [header, *events]) + "\n")
