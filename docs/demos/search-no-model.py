"""Compare fresh offline search on two explicitly selected CLI executables."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def invoke(binary, directory, args):
    environment = dict(os.environ, HOME=str(directory),
                       XDG_CONFIG_HOME=str(directory / "config"),
                       XDG_DATA_HOME=str(directory / "data"))
    return subprocess.run([binary, *args], cwd=directory, env=environment,
                          text=True, capture_output=True, check=False)


for label, variable in [("BASE", "QUIPU_DEMO_BASE"), ("CANDIDATE", "QUIPU_DEMO_HEAD")]:
    binary = str(Path(os.environ[variable]).resolve())
    with tempfile.TemporaryDirectory(prefix="offline-search-") as temporary:
        directory = Path(temporary)
        print(f"\n{label}: isolated fresh store, no model or configuration", flush=True)
        version = invoke(binary, directory, ["version"])
        assert version.returncode == 0, version.stderr
        print(version.stdout, end="", flush=True)
        body = b'<https://example.org/library> <http://www.w3.org/2000/01/rdf-schema#label> "Clockwork Library" .\n'
        (directory / "sample.ttl").write_bytes(body)
        print("$ quipu ingest sample.ttl --db fresh.db --graph https://example.org/sample --timestamp 2026-01-01T00:00:00Z --declare-count 1 --declare-sha256 <source digest>", flush=True)
        loaded = invoke(binary, directory, ["ingest", "sample.ttl", "--db", "fresh.db",
            "--graph", "https://example.org/sample", "--timestamp", "2026-01-01T00:00:00Z",
            "--declare-count", "1", "--declare-sha256", hashlib.sha256(body).hexdigest()])
        assert loaded.returncode == 0, loaded.stderr
        print("ingest exit=0", flush=True)
        print("$ quipu search Clockwork --db fresh.db", flush=True)
        result = invoke(binary, directory, ["search", "Clockwork", "--db", "fresh.db"])
        print(result.stdout, end="", flush=True)
        print(result.stderr, end="", flush=True)
        print(f"search exit={result.returncode}", flush=True)
        if label == "BASE":
            assert result.returncode != 0, "baseline unexpectedly succeeded"
        else:
            assert result.returncode == 0, result.stderr
            data = json.loads(result.stdout)
            assert data["count"] == 1 and data["ranking"] == "keyword"
            assert data["mode_reason"] == "no_embedding_provider"
            assert "similarity" not in data["results"][0]
            print("$ quipu doctor search --db fresh.db", flush=True)
            doctor = invoke(binary, directory, ["doctor", "search", "--db", "fresh.db"])
            assert doctor.returncode == 0, doctor.stderr
            print(doctor.stdout, end="", flush=True)
            assert json.loads(doctor.stdout)["semantic_ready"] is False

print("\nPASS: same fresh ingest/search changes from refusal to one keyword hit; semantic readiness remains false.", flush=True)
