#!/usr/bin/env python3
"""Use an explicitly selected candidate binary and disposable local databases."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile

binary = os.environ.get("QUIPU_DEMO_BIN")
if not binary or not Path(binary).is_file():
    raise SystemExit("Set QUIPU_DEMO_BIN to the exact qualified candidate executable")

with tempfile.TemporaryDirectory(prefix="store-ownership-demo-") as scratch:
    root = Path(scratch)

    def run(args, expected=0):
        print("$ quipu " + " ".join(args), flush=True)
        result = subprocess.run([binary, *args], cwd=root, capture_output=True, text=True)
        print(result.stdout or result.stderr, end="", flush=True)
        if result.returncode != expected:
            raise SystemExit(f"unexpected exit: {result.returncode}, expected {expected}")
        return result.stdout

    print("Candidate source only; isolated SQLite fixtures; no service or installation")
    run(["read", "SELECT ?x WHERE { VALUES ?x { 1 } }", "--db", "original.db"])
    source = sqlite3.connect(root / "original.db")
    destination = sqlite3.connect(root / "copy.db")
    source.backup(destination)
    source.close()
    destination.close()
    print("SQLite backup copied database facts/identity, without adopting the new path")
    root.joinpath("fact.ttl").write_text('<urn:example:copy> <urn:example:title> "owned" .\n')
    run(["knot", "fact.ttl", "--db", "copy.db"], expected=1)
    first = json.loads(run(["store", "adopt", "--db", "copy.db"]))
    run(["knot", "fact.ttl", "--db", "copy.db"])
    second = json.loads(run(["store", "adopt", "--db", "copy.db"]))
    assert first["log_epoch"] != second["log_epoch"]
    print("PASS: copy write refused; explicit adopt permits reopened writes; each adopt minted a fresh epoch")
