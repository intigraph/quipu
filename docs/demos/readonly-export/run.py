#!/usr/bin/env python3
"""Compare actual CLI export on a disposable cold fixture."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile

base = os.environ.get("BASE_BIN", "quipu")
candidate = os.environ.get("CANDIDATE_BIN", "quipu")
with tempfile.TemporaryDirectory(prefix="readonly-export-") as tmp:
    root = Path(tmp)
    env = {"HOME": tmp, "XDG_CONFIG_HOME": str(root / "config"), "PATH": "/usr/bin:/bin"}
    def run(binary, args):
        return subprocess.run([binary, *args], cwd=root, env=env, capture_output=True, text=True, check=False)
    (root / "input.nt").write_text("<urn:positive> <urn:predicate> <urn:object> .\n")
    setup = run(base, ["knot", "input.nt", "--db", "snapshot.db"])
    assert setup.returncode == 0, setup.stderr
    db = root / "snapshot.db"
    def snapshot():
        return db.read_bytes(), db.stat().st_mtime_ns, sorted(p.name for p in root.iterdir())
    before = snapshot()
    print("Disposable cold fixture: one positive RDF triple", flush=True)
    print("$ base export --read-only --db snapshot.db", flush=True)
    old = run(base, ["export", "--read-only", "--db", "snapshot.db"])
    print(f"exit={old.returncode}\n{old.stderr.strip()}", flush=True)
    assert old.returncode != 0
    print("$ candidate export --read-only --db snapshot.db", flush=True)
    new = run(candidate, ["export", "--read-only", "--db", "snapshot.db"])
    print(f"exit={new.returncode}\n{new.stdout.strip()}", flush=True)
    assert new.returncode == 0 and "<urn:positive>" in new.stdout, new.stderr
    after = snapshot()
    assert before == after
    print("bytes, mtime and directory entries unchanged: PASS", flush=True)
    print("snapshot SHA256=" + hashlib.sha256(after[0]).hexdigest(), flush=True)
    normal = run(candidate, ["export", "--db", "snapshot.db"])
    assert normal.returncode == 0 and normal.stdout == new.stdout
    print("default export returns identical RDF: PASS", flush=True)
