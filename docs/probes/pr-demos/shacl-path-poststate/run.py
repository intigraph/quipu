"""Replay class/path admission against two caller-provided local adapters."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

SHAPES = """@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/path/> .
ex:S a sh:PropertyShape; sh:targetNode ex:i;
 sh:path ( [sh:inversePath ex:p] [sh:inversePath ex:p] ); sh:class ex:C.
"""
BAD = """@prefix ex: <http://example.org/path/> .
ex:j ex:p ex:i. ex:k ex:p ex:j. ex:l ex:p ex:j.
"""

def run(binary, data):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "shapes.ttl").write_text(SHAPES)
        (root / "data.ttl").write_text(data)
        result = subprocess.run([binary, "--write-path", "--shapes",
                                 str(root / "shapes.ttl"), "--data", str(root / "data.ttl")],
                                capture_output=True, text=True, check=True, timeout=60)
        return json.loads(result.stdout)

before = run(os.environ["BEFORE_ADAPTER"], BAD)
after = run(os.environ["AFTER_ADAPTER"], BAD)
print("Input: three edges completing an inverse path to untyped values.")
print("BEFORE whole graph conforms:", before["batch"]["conforms"])
print("BEFORE every per-subject write accepted:", before["modes"]["per_subject"]["conforms"])
print("AFTER every per-subject write accepted:", after["modes"]["per_subject"]["conforms"])
valid = run(os.environ["AFTER_ADAPTER"], BAD + "ex:k a ex:C. ex:l a ex:C.\n")
print("CONTROL complete valid graph accepted:", valid["modes"]["single"]["conforms"])
assert before["batch"]["conforms"] is False
assert before["modes"]["per_subject"]["conforms"] is True
assert after["modes"]["per_subject"]["conforms"] is False
assert after["modes"]["single"]["conforms"] is False
assert valid["modes"]["single"]["conforms"] is True
print("PASS: a stored path cannot silently complete an invalid post-state.")
