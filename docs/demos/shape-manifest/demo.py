"""Synthetic local classification transition; no registry or network access."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
with tempfile.TemporaryDirectory() as directory:
    shapes = Path(directory) / "shapes"
    shutil.copytree(ROOT / "shapes", shapes)
    (shapes / "fixture-new-profile.ttl").write_text("@prefix ex: <https://example.invalid/> .\n")
    spec = importlib.util.spec_from_file_location("old_invariants", ROOT / "shapes/verify_shape_invariants.py")
    old = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(old)
    old.SHAPES = shapes / "aegis-ontology.shapes.ttl"
    with contextlib.redirect_stdout(io.StringIO()):
        try:
            old.main()
            code = 0
        except SystemExit as exc:
            code = exc.code or 0
    print(f"Before: existing invariant checker, new unclassified profile => exit {code}", flush=True)
    assert code == 0

    def check(expected):
        result = subprocess.run([sys.executable, str(shapes / "verify_manifest.py"), "--json"],
                                capture_output=True, text=True, check=False)
        assert result.returncode == expected, result.stderr
        if expected:
            print(result.stderr.strip(), flush=True)
        else:
            plan = json.loads(result.stdout)
            print("Manifest accepted; fixture load selected:",
                  any(r["name"] == "fixture-new" for r in plan["load"]), flush=True)

    print("After: mandatory classification checker, SAME unclassified file:", flush=True)
    check(1)
    manifest = json.loads((shapes / "MANIFEST").read_text())
    manifest["files"].append({"file": "fixture-new-profile.ttl", "action": "exclude", "reason": "fixture remains inactive"})
    (shapes / "MANIFEST").write_text(json.dumps(manifest))
    print("Classify as exclude:", flush=True)
    check(0)
    manifest["files"][-1].update(action="load", name="fixture-new")
    (shapes / "MANIFEST").write_text(json.dumps(manifest))
    print("Classify as load with a stable name:", flush=True)
    check(0)
