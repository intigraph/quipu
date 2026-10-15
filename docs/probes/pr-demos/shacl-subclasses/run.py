"""Replay the same private RDF fixtures with unchanged before/after CLI copies."""
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent
for label, variable in (("before", "BEFORE_QUIPU"), ("after", "AFTER_QUIPU")):
    binary = os.environ[variable]
    print(f"=== {label}: private CLI fixture, no server or production data ===", flush=True)
    version = subprocess.run([binary, "--version"], text=True, capture_output=True, check=True)
    print(version.stdout, end="", flush=True)
    for fixture in ("subclass-missing", "direct-missing", "subclass-valid"):
        args = ["validate", "--shapes", "shapes.ttl", "--data", fixture + ".ttl"]
        print("$ quipu " + " ".join(args), flush=True)
        result = subprocess.run([binary, *args], cwd=root, text=True, capture_output=True)
        print(result.stdout, end="", flush=True)
        if result.stderr:
            raise RuntimeError("unexpected validator diagnostic; inspect privately")
        print(f"exit={result.returncode}", flush=True)
        expected = 1 if fixture == "direct-missing" or (fixture == "subclass-missing" and label == "after") else 0
        if result.returncode != expected:
            raise RuntimeError("fixture result differs from the demonstrated before/after contract")
