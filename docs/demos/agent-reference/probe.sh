#!/usr/bin/env bash
# arming: agent-invoked; isolated source checks and committed generated man output.
set -euo pipefail
ROOT=$(git rev-parse --show-toplevel)
BASE=${BASE:-d707ec62fe105f3296143321e681aec8daa46f28}
python3 - "$ROOT" "$BASE" <<'PY'
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile

root, base = Path(sys.argv[1]), sys.argv[2]
path = "packaging/man/quipu-search.1"
before = subprocess.check_output(["git", "-C", str(root), "show", base + ":" + path], text=True)
after = (root / path).read_text()
for label, text in (("base", before), ("candidate", after)):
    lines = text.splitlines()
    i = next(i for i, line in enumerate(lines) if line.startswith(r"\fB\-\-limit\fR"))
    description = lines[i + 1]
    print(label + " man search --limit: " + (description or "<blank>"), flush=True)
    assert description == ("" if label == "base" else "Maximum number of results")
spec = importlib.util.spec_from_file_location("builder", root / "scripts/build-repository-share.py")
builder = importlib.util.module_from_spec(spec)
sys.dont_write_bytecode = True
spec.loader.exec_module(builder)
paths = subprocess.check_output(["git", "-C", str(root), "ls-files", "-z"], text=True).split("\0")
required = builder.agent_references(root, paths)
print("candidate required tracked agent sources: " + str(len(required)), flush=True)
with tempfile.TemporaryDirectory() as folder:
    copied = Path(folder)
    for path in required:
        target = copied / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((root / path).read_bytes())
    builder.agent_references(copied, required)
    print("candidate complete source fixture: ACCEPTED", flush=True)
    for path in required:
        target = copied / path
        original = target.read_bytes()
        target.unlink()
        try:
            builder.agent_references(copied, [p for p in required if p != path])
        except ValueError:
            print("candidate missing " + path + ": REFUSED", flush=True)
        else:
            raise AssertionError("missing reference admitted")
        target.write_bytes(original)
print("No release publication or runtime operation", flush=True)
PY
