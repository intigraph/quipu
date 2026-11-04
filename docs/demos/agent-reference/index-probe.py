#!/usr/bin/env python3
"""Private five-document index/export/import probe; no publisher or live store.

# arming: agent-invoked; requires explicit native CLI/server, indexer and runtime.
"""

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

if len(sys.argv) != 4:
    raise SystemExit("usage: index-probe.py <source-checkout> <quipu-bin> <bobbin-bin>")
source, binary, bobbin = (Path(p).resolve() for p in sys.argv[1:])
spec = importlib.util.spec_from_file_location(
    "builder", source / "scripts/build-repository-share.py"
)
builder = importlib.util.module_from_spec(spec)
sys.dont_write_bytecode = True
spec.loader.exec_module(builder)
paths = builder.command(["git", "-C", source, "ls-files", "-z"]).split("\0")
required = builder.agent_references(source, paths)

with tempfile.TemporaryDirectory(prefix="agent-reference-index-") as folder:
    private = Path(folder)
    corpus, index, database = private / "source", private / "index", private / "indexed.db"
    for path in (*required, "README.md"):
        target = corpus / path
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / path, target)
    (corpus / ".bobbin").mkdir()
    (corpus / ".bobbin/config.toml").write_text(
        "[quipu]\nstore_path = " + json.dumps(str(database)) + "\n"
    )
    (index / ".bobbin").mkdir(parents=True)
    (index / ".bobbin/config.toml").write_text(builder.INDEX_CONFIG)
    arguments = [
        bobbin, "index", index, "--source", corpus, "--repo", "quipu",
        "--force", "--skip-calibrate", "--json",
    ]
    environment = builder.isolated_environment(private)
    # Deterministic private CPU proof; never provision or select a GPU.
    environment["BOBBIN_GPU"] = "0"
    stdout, stderr = private / "index.stdout", private / "index.stderr"
    try:
        with stdout.open("w") as out, stderr.open("w") as err:
            subprocess.run(
                [str(value) for value in arguments],
                env=environment, cwd=private,
                stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                check=True, text=True, timeout=240,
            )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        print("native index FAILED: " + stderr.read_text().split("Error:")[-1].strip(), file=sys.stderr)
        raise
    assert database.is_file()
    builder.verify_agent_references(binary, database, ("README.md", *required))
    print("native index: README + all 4 actual reference pointers and exact paths PRESENT", flush=True)

    for shape in sorted((source / "shapes").glob("*.ttl")):
        builder.command([binary, "shapes", "load", shape.stem, shape, "--db", database], cwd=private)
    stage = private / "share"
    query = (source / "queries/repository-share-quipu.rq").read_text()
    builder.command([
        binary, "share", "--construct", query, "--destination", "internal",
        "--no-queries", "--output", stage, "--db", database,
    ], cwd=private)
    receiver = private / "receiver.db"
    builder.command([
        binary, "shapes", "load", "fixture", stage / "shapes.ttl", "--db", receiver,
    ], cwd=private)
    imported = json.loads(builder.command([binary, "import", stage, "--db", receiver], cwd=private))
    assert imported["outcome"] == "staged" and imported["promotion"]["eligible"] is True, imported
    builder.command([binary, "import", "promote", imported["share_id"], "--db", receiver], cwd=private)
    builder.verify_agent_references(binary, receiver, ("README.md", *required))
    print("native private share/import: README + all 4 exact reference pointers PRESENT", flush=True)
    print("scope=five-real-documents destination=internal published=NONE live_store_operations=NONE")
