#!/usr/bin/env python3
"""Compare actual native adapters; all writes stay in disposable stores."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


PREFIX = """@prefix ex: <https://example.org/demo/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
"""
LOCAL = PREFIX + """ex:S a sh:NodeShape; sh:targetClass ex:T;
sh:property [sh:path ex:p; sh:or ([sh:datatype xsd:integer] [sh:datatype xsd:boolean])].
"""
EQUAL = PREFIX + """ex:S a sh:NodeShape; sh:targetClass ex:T;
sh:property [sh:path ex:p; sh:equals ex:q].
"""
CASES = [
    ("invalid late value", LOCAL, 'ex:a ex:p "bad"; a ex:T .', False),
    ("valid late value", LOCAL, 'ex:a ex:p 1; a ex:T .', True),
    ("equality completion", EQUAL,
     'ex:a ex:p "x"; ex:q "y"; a ex:T . ex:a ex:p "y"; ex:q "x" .', True),
]


def main():
    for phase, variable in [("before", "SHACL_DEMO_BASE"), ("after", "SHACL_DEMO_CANDIDATE")]:
        binary = Path(os.environ[variable]).resolve()
        print(f"{phase} native adapter sha256={hashlib.sha256(binary.read_bytes()).hexdigest()}", flush=True)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, shapes, data, expected in CASES:
                (root / "shapes.ttl").write_text(shapes)
                (root / "data.ttl").write_text(PREFIX + data)
                result = subprocess.run(
                    [str(binary), "--write-path", "--shapes", str(root / "shapes.ttl"),
                     "--data", str(root / "data.ttl")],
                    capture_output=True, text=True, check=True, timeout=30,
                )
                verdict = json.loads(result.stdout)
                modes = verdict["modes"]
                assert len(modes) == 7 and all(v["conforms"] is not None for v in modes.values())
                assert verdict["batch"]["conforms"] is expected
                accepted = sum(v["conforms"] for v in modes.values())
                want = (5 if phase == "before" else 7) if not expected else 0
                assert 7 - accepted == want, (phase, name, modes)
                print(f"  {name}: batch={expected}, split accepted={accepted}/7, rejected={7-accepted}/7", flush=True)


if __name__ == "__main__":
    main()
